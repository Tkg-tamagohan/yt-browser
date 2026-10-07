//! Tauri invoke ハンドラ（設計書 §2, §3.1）。
//! 入力検証と UI 向けの直列化に徹し、実処理は各モジュールへ委譲する。

use tauri::State;

use crate::db::Db;
use crate::error::UiError;
use crate::model::DbStatus;

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
