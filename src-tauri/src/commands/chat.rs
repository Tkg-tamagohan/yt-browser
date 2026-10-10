//! ライブチャット系コマンド（設計書 §3.1、FR-6）。

use std::sync::Arc;

use tauri::State;

use super::parse_video_id;
use crate::db::Db;
use crate::error::UiError;

/// `chat_start`（設計書 §3.1、FR-6）。指定動画のライブチャット取得を
/// バックグラウンドで開始する。見つからない・失敗した場合の通知は
/// `chat://status` イベントに流れる。
/// `instance_id` はリプレイの同期先を固定するためのパネル起票インスタンス
/// （FR-24。未指定なら同じ動画を再生中のいずれかの位置で同期する）
#[tauri::command]
pub fn chat_start(
    video_id: String,
    instance_id: Option<u32>,
    poller: State<'_, Arc<crate::chat::ChatPoller>>,
) -> Result<(), UiError> {
    let id = parse_video_id(&video_id)?;
    poller.start(&id, instance_id);
    Ok(())
}

/// `chat_stop`。指定動画のチャット取得を止める。
#[tauri::command]
pub fn chat_stop(
    video_id: String,
    poller: State<'_, Arc<crate::chat::ChatPoller>>,
) -> Result<(), UiError> {
    let id = parse_video_id(&video_id)?;
    poller.stop(&id);
    Ok(())
}

/// `chat_history_search`（FR-6 の保存・検索要件）。本文・投稿者名の FTS5 AND 検索。
#[tauri::command]
pub fn chat_history_search(
    video_id: Option<String>,
    query: String,
    limit: Option<u32>,
    db: State<'_, Db>,
) -> Result<Vec<crate::model::ChatEvent>, UiError> {
    let q = query.trim().to_string();
    if q.is_empty() {
        return Err(UiError::invalid_input("検索語が空です"));
    }
    if q.len() > 256 {
        return Err(UiError::invalid_input("検索語が長すぎます"));
    }
    let vid = match &video_id {
        Some(v) if !v.trim().is_empty() => Some(parse_video_id(v)?),
        _ => None,
    };
    Ok(db.chat_search(vid.as_deref(), &q, limit.unwrap_or(100).min(1000))?)
}
