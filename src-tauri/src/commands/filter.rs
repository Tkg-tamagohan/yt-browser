//! NG フィルタ・チャンネルブロック系コマンド（設計書 §3.1、FR-5・FR-9）。

use std::sync::Arc;

use tauri::State;

use crate::db::Db;
use crate::error::UiError;

/// `block_channel`（設計書 §3.1、FR-5）。検索・関連・フィードの全一覧から除外される。
#[tauri::command]
pub fn block_channel(channel_id: String, title: String, db: State<'_, Db>) -> Result<(), UiError> {
    let channel_id = channel_id.trim();
    if channel_id.is_empty() {
        return Err(UiError::invalid_input("channel_id が空です"));
    }
    db.blocked_add(channel_id, title.trim())?;
    Ok(())
}

/// `unblock_channel`（FR-5）。設定画面のブロック一覧から解除する。
#[tauri::command]
pub fn unblock_channel(channel_id: String, db: State<'_, Db>) -> Result<(), UiError> {
    db.blocked_remove(channel_id.trim())?;
    Ok(())
}

/// ブロック中チャンネル一覧（設定画面用）。
#[tauri::command]
pub fn blocked_channels(db: State<'_, Db>) -> Result<Vec<crate::model::BlockedChannel>, UiError> {
    Ok(db.blocked_list()?)
}

/// `filter_add`（設計書 §3.1、FR-9）。target/kind/pattern を検証して登録し、
/// 稼働中の NG 評価器を作り直す。
#[tauri::command]
pub fn filter_add(
    target: String,
    kind: String,
    pattern: String,
    db: State<'_, Db>,
    ng: State<'_, Arc<crate::filter::NgMatcher>>,
) -> Result<crate::model::Filter, UiError> {
    let target = target.trim().to_string();
    let kind = kind.trim().to_string();
    let pattern = pattern.trim().to_string();
    if !crate::model::FILTER_TARGETS.contains(&target.as_str()) {
        return Err(UiError::invalid_input(format!(
            "target が不正です: {target}"
        )));
    }
    if !crate::model::FILTER_KINDS.contains(&kind.as_str()) {
        return Err(UiError::invalid_input("kind は literal または regex です"));
    }
    if pattern.is_empty() {
        return Err(UiError::invalid_input("pattern が空です"));
    }
    if pattern.len() > 512 {
        return Err(UiError::invalid_input(
            "pattern が長すぎます（512 文字上限）",
        ));
    }
    if kind == "regex" {
        regex::Regex::new(&pattern)
            .map_err(|e| UiError::invalid_input(format!("正規表現が不正です: {e}")))?;
    }
    let f = db.filter_add(&target, &kind, &pattern)?;
    ng.refresh()?;
    Ok(f)
}

/// `filter_remove`。削除して NG 評価器を作り直す。
#[tauri::command]
pub fn filter_remove(
    id: i64,
    db: State<'_, Db>,
    ng: State<'_, Arc<crate::filter::NgMatcher>>,
) -> Result<(), UiError> {
    db.filter_remove(id)?;
    ng.refresh()?;
    Ok(())
}

/// `filter_list`。登録済み NG フィルタの一覧。
#[tauri::command]
pub fn filter_list(db: State<'_, Db>) -> Result<Vec<crate::model::Filter>, UiError> {
    Ok(db.filter_list()?)
}
