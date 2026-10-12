//! 一時キューの永続化と拡張からのメタ補完（FR-29・FR-30、仕様決定 AX・AY）。

use std::sync::Arc;

use super::parse_video_id;
use crate::db::Db;
use crate::error::UiError;
use crate::model::{QueueItem, QueueSnapshot, VideoRef};
use tauri::State;

/// `queue_save` に受け付ける項目数の上限。UI 上の積み上げ運用を
/// 大きく超える入力は誤りとして弾く
const MAX_QUEUE_ITEMS: usize = 1000;
/// キュー項目の各文字列の上限（`videos.title` の入力値と同じ扱い）。
const MAX_QUEUE_TEXT_LEN: usize = 1024;

/// `queue_save`（FR-30、仕様決定 AY）。一時キューの項目列と現在位置を
/// 全置換で保存する。呼び出しはフロントの全ミューテーションごと
/// （write-through）。
#[tauri::command]
pub fn queue_save(
    items: Vec<QueueItem>,
    current_index: i64,
    db: State<'_, Db>,
) -> Result<(), UiError> {
    if items.len() > MAX_QUEUE_ITEMS {
        return Err(UiError::invalid_input("キューの項目数が上限を超えています"));
    }
    let mut items = items;
    for item in items.iter_mut() {
        item.video_id = parse_video_id(&item.video_id)?;
        let too_long =
            |s: &Option<String>| s.as_deref().unwrap_or("").chars().count() > MAX_QUEUE_TEXT_LEN;
        if item.title.chars().count() > MAX_QUEUE_TEXT_LEN
            || too_long(&item.channel_title)
            || too_long(&item.thumbnail_url)
        {
            return Err(UiError::invalid_input("キュー項目の文字列が長すぎます"));
        }
    }
    Ok(db.queue_save(&items, current_index)?)
}

/// `queue_load`（FR-30、仕様決定 AY）。起動時の一時キュー復元用。
#[tauri::command]
pub fn queue_load(db: State<'_, Db>) -> Result<QueueSnapshot, UiError> {
    Ok(db.queue_load()?)
}

/// `video_meta_lookup`（FR-29、仕様決定 AX）。拡張からのキュー追加で
/// 動画 ID だけ判明する項目の表示メタを `videos` 台帳 → `watch_history`
/// の順に補完する。どちらにも無ければ None。
#[tauri::command]
pub fn video_meta_lookup(video_id: String, db: State<'_, Db>) -> Result<Option<VideoRef>, UiError> {
    let id = parse_video_id(&video_id)?;
    Ok(db.video_meta_lookup(&id)?)
}

/// `video_meta_resolve`（FR-29、仕様決定 BD）。台帳・履歴の補完が外れた
/// キュー項目の表示メタを InnerTube `player` の `videoDetails` から
/// 遅延解決する。タイトルが取れなければ None。
#[tauri::command]
pub async fn video_meta_resolve(
    video_id: String,
    innertube: State<'_, Arc<crate::innertube::InnerTube>>,
) -> Result<Option<VideoRef>, UiError> {
    let id = parse_video_id(&video_id)?;
    Ok(innertube.video_meta(&id).await?)
}
