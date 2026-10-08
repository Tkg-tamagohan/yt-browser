//! プレイヤー系コマンド（設計書 §3.1）。

use super::parse_video_id;
use crate::db::Db;
use crate::error::UiError;
use crate::model::{PlayerAction, PlayerState};
use crate::mpv::PlayerManager;
use crate::yt;
use tauri::State;

/// `play_video`（設計書 §3.1）。`video_id` は URL 各形式も受け取り正規化する。
/// `resume` が true のとき、未完了の履歴位置から再開する。
/// `pip` が true のとき、最前面・枠なしの小窓で起動する（設計書 §4.5）。
#[tauri::command]
pub async fn play_video(
    video_id: String,
    resume: bool,
    pip: Option<bool>,
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
        .play(&id, start_sec, ytdl_format, pip.unwrap_or(false))
        .await
        .map_err(UiError::from)
}

/// `player_list`。稼働中インスタンスのスナップショット一覧を返す。
/// ページ再読み込み後にカードを復元するため、イベントだけでは
/// 再通知されない一時停止中インスタンスの状態もここで拾う。
#[tauri::command]
pub fn player_list(players: State<'_, PlayerManager>) -> Vec<PlayerState> {
    players.list()
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
