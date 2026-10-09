//! DB・設定・yt-dlp のシステム系コマンド。

use tauri::{AppHandle, Emitter, State};

use crate::db::Db;
use crate::error::UiError;
use crate::model::{DbStatus, SettingChanged, YtDlpStatus};
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
pub fn settings_set(
    app: AppHandle,
    db: State<'_, Db>,
    key: String,
    value: String,
) -> Result<(), UiError> {
    validate_setting_key(&key)?;
    if value.len() > MAX_SETTING_VALUE_LEN {
        return Err(UiError::invalid_input(format!(
            "value が長すぎます（{MAX_SETTING_VALUE_LEN} バイト上限）"
        )));
    }
    db.setting_set(&key, &value)?;
    // コミット済みの値を載せて変更を通知する。設定値を保持する画面は
    // 再読み取りなしで同期できる。購読者がいなくても正常なので失敗は捨てる
    let _ = app.emit("settings://changed", SettingChanged { key, value });
    Ok(())
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
