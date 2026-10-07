//! Tauri invoke ハンドラ（設計書 §2, §3.1）。
//! 入力検証と UI 向けの直列化に徹し、実処理は各モジュールへ委譲する。

use tauri::State;

use crate::db::Db;
use crate::error::UiError;
use crate::model::{normalize_video_id, DbStatus, PlayerAction, WatchHistory, YtDlpStatus};
use crate::mpv::PlayerManager;
use crate::yt::{self, YtDlpResolver};

const MAX_SETTING_KEY_LEN: usize = 128;
const MAX_SETTING_VALUE_LEN: usize = 16 * 1024;

fn validate_setting_key(key: &str) -> Result<(), UiError> {
    if key.is_empty() {
        return Err(UiError::invalid_input("key が空です"));
    }
    if key.len() > MAX_SETTING_KEY_LEN {
        return Err(UiError::invalid_input(format!(
            "key が長すぎます（{MAX_SETTING_KEY_LEN} 文字上限）"
        )));
    }
    Ok(())
}

/// 入力（URL 各形式または動画 ID）を動画 ID へ正規化する共通処理。
fn parse_video_id(input: &str) -> Result<String, UiError> {
    normalize_video_id(input).ok_or_else(|| {
        UiError::invalid_input("YouTube の動画 URL または 11 文字の動画 ID を入力してください")
    })
}

/// DB の生存確認。起動直後のヘルスチェック用。
#[tauri::command]
pub fn db_status(db: State<'_, Db>) -> Result<DbStatus, UiError> {
    Ok(DbStatus {
        schema_version: db.schema_version()?,
    })
}

#[tauri::command]
pub fn settings_get(db: State<'_, Db>, key: String) -> Result<Option<String>, UiError> {
    validate_setting_key(&key)?;
    Ok(db.setting_get(&key)?)
}

#[tauri::command]
pub fn settings_set(db: State<'_, Db>, key: String, value: String) -> Result<(), UiError> {
    validate_setting_key(&key)?;
    if value.len() > MAX_SETTING_VALUE_LEN {
        return Err(UiError::invalid_input(format!(
            "value が長すぎます（{MAX_SETTING_VALUE_LEN} バイト上限）"
        )));
    }
    Ok(db.setting_set(&key, &value)?)
}

/// `play_video`（設計書 §3.1）。`video_id` は URL 各形式も受け取り正規化する。
/// `resume` が true のとき、未完了の履歴位置から再開する。
#[tauri::command]
pub async fn play_video(
    video_id: String,
    resume: bool,
    players: State<'_, PlayerManager>,
    db: State<'_, Db>,
) -> Result<u32, UiError> {
    let id = parse_video_id(&video_id)?;
    let start_sec = if resume {
        db.history_get(&id)?
            .filter(|h| !h.completed)
            .map(|h| h.position_sec as f64)
            .unwrap_or(0.0)
    } else {
        0.0
    };
    let ytdl_format = db.setting_get(yt::SETTING_QUALITY_FORMAT)?;
    players
        .play(&id, start_sec, ytdl_format)
        .await
        .map_err(UiError::from)
}

/// `player_control`（設計書 §3.1）。操作は `PlayerAction` のタグ付き列挙で受け取る。
#[tauri::command]
pub async fn player_control(
    instance_id: u32,
    action: PlayerAction,
    players: State<'_, PlayerManager>,
) -> Result<(), UiError> {
    players
        .control(instance_id, &action)
        .await
        .map_err(UiError::from)
}

/// `player_close`（設計書 §3.1）。
#[tauri::command]
pub async fn player_close(
    instance_id: u32,
    players: State<'_, PlayerManager>,
) -> Result<(), UiError> {
    players.close(instance_id).await.map_err(UiError::from)
}

/// 動画 1 件分の視聴履歴。`video_id` は `play_video` と同じ正規化を経る。
#[tauri::command]
pub fn history_get(video_id: String, db: State<'_, Db>) -> Result<Option<WatchHistory>, UiError> {
    let id = parse_video_id(&video_id)?;
    Ok(db.history_get(&id)?)
}

/// yt-dlp の解決パスとバージョン（設計書 §5 の運用確認用）。
#[tauri::command]
pub async fn ytdlp_status(
    db: State<'_, Db>,
    resolver: State<'_, YtDlpResolver>,
) -> Result<YtDlpStatus, UiError> {
    let (path, version) = resolver.status(&db).await;
    Ok(YtDlpStatus { path, version })
}

/// `yt-dlp -U` による更新（設計書 §5 の更新機構）。
/// システム管理パスでは権限不足で失敗し得る。その場合もエラーをそのまま返す。
#[tauri::command]
pub async fn ytdlp_update(
    db: State<'_, Db>,
    resolver: State<'_, YtDlpResolver>,
) -> Result<String, UiError> {
    let path = resolver.resolve(&db).await.ok_or(yt::YtError::NotFound)?;
    Ok(yt::update(&path).await?)
}
