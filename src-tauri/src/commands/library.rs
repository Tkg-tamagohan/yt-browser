//! 履歴・お気に入り・プレイリスト系コマンド（FR-7）。

use super::parse_video_id;
use crate::db::Db;
use crate::error::UiError;
use crate::model::{FavoriteEntry, Playlist, PlaylistEntry, VideoRef, WatchHistory};
use tauri::State;

/// 動画 1 件分の視聴履歴。`video_id` は `play_video` と同じ正規化を経る。
#[tauri::command]
pub fn history_get(video_id: String, db: State<'_, Db>) -> Result<Option<WatchHistory>, UiError> {
    let id = parse_video_id(&video_id)?;
    Ok(db.history_get(&id)?)
}

/// 履歴一覧の既定・最大件数。
const HISTORY_LIST_LIMIT: u32 = 500;
/// プレイリスト名の最大長。
const MAX_PLAYLIST_NAME_LEN: usize = 100;
/// 動画タイトル等の上限（`videos.title` の入力値）。
const MAX_VIDEO_FIELD_LEN: usize = 1024;

/// `history_list`（FR-7）。新しく見た順。`limit` 省略時は 500、上限 1000。
#[tauri::command]
pub fn history_list(limit: Option<u32>, db: State<'_, Db>) -> Result<Vec<WatchHistory>, UiError> {
    let limit = limit.unwrap_or(HISTORY_LIST_LIMIT).min(1000);
    Ok(db.history_list(limit)?)
}

/// `history_remove`（FR-7、仕様決定 I の手動削除）。
#[tauri::command]
pub fn history_remove(video_id: String, db: State<'_, Db>) -> Result<(), UiError> {
    let id = parse_video_id(&video_id)?;
    Ok(db.history_remove(&id)?)
}

/// `VideoRef` の入力検証と動画 ID 正規化。
fn validate_video_ref(v: &VideoRef) -> Result<String, UiError> {
    let id = parse_video_id(&v.video_id)?;
    if v.title.chars().count() > MAX_VIDEO_FIELD_LEN {
        return Err(UiError::invalid_input("タイトルが長すぎます"));
    }
    Ok(id)
}

/// `favorite_add`（FR-7）。動画メタは `videos` 台帳へ登録される。
#[tauri::command]
pub fn favorite_add(video: VideoRef, db: State<'_, Db>) -> Result<(), UiError> {
    let id = validate_video_ref(&video)?;
    let mut v = video;
    v.video_id = id;
    Ok(db.favorite_add(&v)?)
}

/// `favorite_remove`（FR-7）。
#[tauri::command]
pub fn favorite_remove(video_id: String, db: State<'_, Db>) -> Result<(), UiError> {
    let id = parse_video_id(&video_id)?;
    Ok(db.favorite_remove(&id)?)
}

/// `favorite_list`（FR-7）。追加が新しい順。
#[tauri::command]
pub fn favorite_list(db: State<'_, Db>) -> Result<Vec<FavoriteEntry>, UiError> {
    Ok(db.favorite_list()?)
}

/// `playlist_list`（FR-7）。件数集計付き。
#[tauri::command]
pub fn playlist_list(db: State<'_, Db>) -> Result<Vec<Playlist>, UiError> {
    Ok(db.playlist_list()?)
}

/// プレイリスト名の検証（トリム・非空・長さ）。
fn validate_playlist_name(name: &str) -> Result<String, UiError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(UiError::invalid_input("プレイリスト名が空です"));
    }
    if name.chars().count() > MAX_PLAYLIST_NAME_LEN {
        return Err(UiError::invalid_input(format!(
            "プレイリスト名が長すぎます（{MAX_PLAYLIST_NAME_LEN} 文字上限）"
        )));
    }
    Ok(name.to_string())
}

/// `playlist_create`（FR-7）。
#[tauri::command]
pub fn playlist_create(name: String, db: State<'_, Db>) -> Result<Playlist, UiError> {
    let name = validate_playlist_name(&name)?;
    Ok(db.playlist_create(&name)?)
}

/// `playlist_rename`（FR-7）。
#[tauri::command]
pub fn playlist_rename(playlist_id: i64, name: String, db: State<'_, Db>) -> Result<(), UiError> {
    if playlist_id <= 0 {
        return Err(UiError::invalid_input("playlist_id が不正です"));
    }
    let name = validate_playlist_name(&name)?;
    Ok(db.playlist_rename(playlist_id, &name)?)
}

/// `playlist_delete`（FR-7）。中身は CASCADE で消える。
#[tauri::command]
pub fn playlist_delete(playlist_id: i64, db: State<'_, Db>) -> Result<(), UiError> {
    if playlist_id <= 0 {
        return Err(UiError::invalid_input("playlist_id が不正です"));
    }
    Ok(db.playlist_delete(playlist_id)?)
}

/// `playlist_items`（FR-7）。position 昇順。
#[tauri::command]
pub fn playlist_items(playlist_id: i64, db: State<'_, Db>) -> Result<Vec<PlaylistEntry>, UiError> {
    if playlist_id <= 0 {
        return Err(UiError::invalid_input("playlist_id が不正です"));
    }
    Ok(db.playlist_items(playlist_id)?)
}

/// `playlist_add`（FR-7）。末尾への追加、重複は位置を維持して無視。
#[tauri::command]
pub fn playlist_add(playlist_id: i64, video: VideoRef, db: State<'_, Db>) -> Result<(), UiError> {
    if playlist_id <= 0 {
        return Err(UiError::invalid_input("playlist_id が不正です"));
    }
    let id = validate_video_ref(&video)?;
    let mut v = video;
    v.video_id = id;
    Ok(db.playlist_add(playlist_id, &v)?)
}

/// `playlist_remove`（FR-7）。
#[tauri::command]
pub fn playlist_remove(
    playlist_id: i64,
    video_id: String,
    db: State<'_, Db>,
) -> Result<(), UiError> {
    if playlist_id <= 0 {
        return Err(UiError::invalid_input("playlist_id が不正です"));
    }
    let id = parse_video_id(&video_id)?;
    Ok(db.playlist_remove(playlist_id, &id)?)
}
