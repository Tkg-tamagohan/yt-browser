//! 診断情報のエクスポートコマンド（FR-82、仕様決定 DG）。

use tauri::{AppHandle, Manager, State};

use crate::db::Db;
use crate::diag::{export_bundle, DiagContext};
use crate::error::UiError;
use crate::yt::YtDlpResolver;

/// `diag_export`（FR-82、仕様決定 DG）。アプリログ・設定・環境情報を
/// マスク済みで 1 つの zip にまとめて `path` へ書き出す。
/// `path` はフロント側の保存ダイアログで選ばせた値をそのまま受け取る。
#[tauri::command]
pub async fn diag_export(
    path: String,
    app: AppHandle,
    db: State<'_, Db>,
    resolver: State<'_, YtDlpResolver>,
) -> Result<(), UiError> {
    let path = path.trim().to_string();
    if path.is_empty() {
        return Err(UiError::invalid_input("保存先パスが空です"));
    }
    let ctx = DiagContext {
        db: &db,
        ytdlp: &resolver,
        log_dir: app.path().app_log_dir().ok(),
        data_dir: app.path().app_data_dir().ok(),
        config_dir: app.path().config_dir().ok(),
        home_dir: app.path().home_dir().ok(),
        windowing: crate::mpv::windowing_system(),
    };
    export_bundle(std::path::Path::new(&path), &ctx).await?;
    Ok(())
}
