//! キューの永続化・プレイリスト保存と拡張からのメタ補完
//! （FR-29・FR-30・FR-47・FR-48、仕様決定 AX・AY・BX・BY）。

use std::sync::Arc;

use super::{parse_video_id, validate_playlist_name, validate_video_ref};
use crate::db::Db;
use crate::error::UiError;
use crate::model::{Playlist, PlaylistQueueState, QueueItem, QueueSnapshot, VideoRef};
use tauri::{AppHandle, Emitter, State};

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

/// `queue_save_as_playlist`（FR-47、仕様決定 BX）。選択中キューの項目を
/// 順序どおり新規プレイリストへ複製する。複製は `playlist_add_many`
/// の規則に従い、同一 video_id の重複は位置を維持して無視される
/// （`playlist_items` の主キー制約上、同一動画の重複は持てない）。
/// 作成から項目登録までをここで済ませ、項目登録の失敗時は空の
/// プレイリストを残さない（`playlist_import` と同じ後始末）。
/// 成功時は `library://playlists_changed` で作成したプレイリストを通知し、
/// 表示中の /library が画面外からの追加を検知できるようにする
#[tauri::command]
pub fn queue_save_as_playlist(
    name: String,
    items: Vec<VideoRef>,
    db: State<'_, Db>,
    app: AppHandle,
) -> Result<Playlist, UiError> {
    let name = validate_playlist_name(&name)?;
    if items.is_empty() {
        return Err(UiError::invalid_input("キューが空です"));
    }
    if items.len() > MAX_QUEUE_ITEMS {
        return Err(UiError::invalid_input("キューの項目数が上限を超えています"));
    }
    let mut items = items;
    for v in items.iter_mut() {
        v.video_id = validate_video_ref(v)?;
    }
    let mut playlist = db.playlist_create(&name)?;
    if let Err(e) = db.playlist_add_many(playlist.id, &items) {
        // 項目登録の失敗で空の一覧を残さない
        let _ = db.playlist_delete(playlist.id);
        return Err(e.into());
    }
    // INSERT OR IGNORE で重複は位置維持されるため実件数を取り直す
    playlist.item_count = db.playlist_items(playlist.id)?.len() as i64;
    let _ = app.emit("library://playlists_changed", &playlist);
    Ok(playlist)
}

/// `queue_playlist_save`（FR-48、仕様決定 BY）。プレイリストキューの
/// 現在位置を playlist_id 行へ write-through で保存する
/// （項目列は playlist_items を正として保存しない）
#[tauri::command]
pub fn queue_playlist_save(
    playlist_id: i64,
    current_index: i64,
    db: State<'_, Db>,
) -> Result<(), UiError> {
    if playlist_id <= 0 {
        return Err(UiError::invalid_input("playlist_id が不正です"));
    }
    Ok(db.queue_playlist_save(playlist_id, current_index)?)
}

/// `queue_playlist_clear`（FR-48）。保存済みのプレイリストキュー行を消す
/// （キュー消滅時）。未保存でも正常終了とする
#[tauri::command]
pub fn queue_playlist_clear(playlist_id: i64, db: State<'_, Db>) -> Result<(), UiError> {
    if playlist_id <= 0 {
        return Err(UiError::invalid_input("playlist_id が不正です"));
    }
    Ok(db.queue_playlist_clear(playlist_id)?)
}

/// `queue_playlist_load`（FR-48）。起動時のプレイリストキュー復元用。
/// 永続化されたプレイリストキュー（playlist_id・名前・保存位置）の一覧を返す
#[tauri::command]
pub fn queue_playlist_load(db: State<'_, Db>) -> Result<Vec<PlaylistQueueState>, UiError> {
    Ok(db.queue_playlist_load()?)
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
