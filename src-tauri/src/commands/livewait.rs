//! ライブ配信待機のコマンド（FR-44、仕様決定 BT、設計書 §3.1）。

use std::sync::Arc;

use tauri::{AppHandle, State};

use super::parse_video_id;
use crate::db::Db;
use crate::error::UiError;
use crate::innertube::InnerTube;
use crate::livewait::{
    classify_broadcast, emit_changed, epoch_ms, LiveWaitPoller, WaitBroadcast,
};
use crate::model::{LiveWait, LiveWaitAddOutcome, VideoRef};

/// 待機項目の各文字列の上限（queue_save と同じ扱い）。
const MAX_WAIT_TEXT_LEN: usize = 1024;

/// `live_wait_add`（仕様決定 BT(1)）。行のメタとともに待機登録する。
/// 登録前に InnerTube `player` の放送状態を確認し、開始前（ロビー）の
/// ものだけを `added` として登録する。放送中・終了済み・非放送は
/// それぞれの区分を返し、登録はしない（呼び出し側が案内を出す）。
/// 同一動画の重複登録は `duplicate` を返して抑止する。
#[tauri::command]
pub async fn live_wait_add(
    video: VideoRef,
    db: State<'_, Db>,
    innertube: State<'_, Arc<InnerTube>>,
    poller: State<'_, Arc<LiveWaitPoller>>,
    app: AppHandle,
) -> Result<LiveWaitAddOutcome, UiError> {
    let video_id = parse_video_id(&video.video_id)?;
    let too_long = |s: &Option<String>| {
        s.as_deref().unwrap_or("").chars().count() > MAX_WAIT_TEXT_LEN
    };
    if video.title.chars().count() > MAX_WAIT_TEXT_LEN
        || too_long(&video.channel_id)
        || too_long(&video.channel_title)
        || too_long(&video.thumbnail_url)
    {
        return Err(UiError::invalid_input("待機項目の文字列が長すぎます"));
    }
    // 重複抑止は InnerTube 確認より先に判定する（確認不要なので通信を省く）
    if db.live_wait_get(&video_id)?.is_some() {
        return Ok(LiveWaitAddOutcome::Duplicate);
    }
    // 登録時の放送状態確認（仕様決定 BT(1)）。開始前のみ登録へ進む
    let status = innertube.broadcast_status(&video_id).await?;
    let scheduled_ms = match classify_broadcast(&status) {
        WaitBroadcast::Upcoming { scheduled_ms } => scheduled_ms,
        WaitBroadcast::Live => return Ok(LiveWaitAddOutcome::Live),
        WaitBroadcast::Ended => return Ok(LiveWaitAddOutcome::Ended),
        WaitBroadcast::NotBroadcast => return Ok(LiveWaitAddOutcome::NotBroadcast),
    };
    // 行のタイトルが空のときだけ `videoDetails` から補完する（追加の往復は稀な経路に限定）
    let mut title = video.title.trim().to_string();
    let mut channel_id = video.channel_id.clone();
    let mut channel_title = video.channel_title.clone();
    let mut thumbnail_url = video.thumbnail_url.clone();
    if title.is_empty() {
        if let Ok(Some(meta)) = innertube.video_meta(&video_id).await {
            title = meta.title;
            channel_id = channel_id.or(meta.channel_id);
            channel_title = channel_title.or(meta.channel_title);
            thumbnail_url = thumbnail_url.or(meta.thumbnail_url);
        }
    }
    if title.is_empty() {
        title = video_id.clone();
    }
    let entry = LiveWait {
        video_id: video_id.clone(),
        title,
        channel_id,
        channel_title,
        thumbnail_url,
        scheduled_start_ms: scheduled_ms,
        registered_ms: epoch_ms(),
    };
    // 確認から挿入までの競合は INSERT OR IGNORE が吸収する
    if !db.live_wait_insert(&entry)? {
        return Ok(LiveWaitAddOutcome::Duplicate);
    }
    emit_changed(&app, Some(&video_id));
    poller.wake_now();
    Ok(LiveWaitAddOutcome::Added {
        scheduled_start_ms: scheduled_ms,
    })
}

/// `live_wait_remove`（仕様決定 BT(2)）。待機の個別解除。
/// 未登録の動画 ID でも正常終了とする（冪等）。
#[tauri::command]
pub fn live_wait_remove(
    video_id: String,
    db: State<'_, Db>,
    app: AppHandle,
) -> Result<(), UiError> {
    let id = parse_video_id(&video_id)?;
    db.live_wait_remove(&id)?;
    emit_changed(&app, Some(&id));
    Ok(())
}

/// `live_wait_list`（仕様決定 BT(2)(5)）。待機一覧の復元・表示用。
#[tauri::command]
pub fn live_wait_list(db: State<'_, Db>) -> Result<Vec<LiveWait>, UiError> {
    Ok(db.live_wait_list()?)
}
