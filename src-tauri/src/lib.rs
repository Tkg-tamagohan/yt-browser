//! yt-browser アプリケーション本体（設計書 §1.1）。
//! WebView スレッドは UI 描画と入力だけを持ち、重い処理は Tokio ワーカー側へ置く。

mod commands;
mod db;
mod error;
mod model;

use std::path::Path;

use tauri::Manager;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::fmt::writer::MakeWriterExt;
use tracing_subscriber::EnvFilter;

/// 設計書 §9.3: stderr と日次ローリングファイルの双方へ出力する。
/// Windows リリース（GUI サブシステム）ではコンソールが無いため、ファイル出力が必須になる。
/// 返す WorkerGuard はアプリ終了まで保持すること。drop するとファイルへ残ログが届かない。
fn init_tracing(log_dir: &Path) -> WorkerGuard {
    let file_appender = tracing_appender::rolling::daily(log_dir, "yt-browser.log");
    let (file_writer, guard) = tracing_appender::non_blocking(file_appender);
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::new(if cfg!(debug_assertions) {
            "debug"
        } else {
            "info"
        })
    });
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr.and(file_writer))
        .init();
    guard
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            let log_dir = app.path().app_log_dir()?;
            std::fs::create_dir_all(&dir)?;
            std::fs::create_dir_all(&log_dir)?;
            let log_guard = init_tracing(&log_dir);
            app.manage(log_guard);
            let db = db::Db::connect(&dir.join("yt-browser.db"))
                .map_err(|e| -> Box<dyn std::error::Error> { Box::new(e) })?;
            tracing::info!(path = %dir.display(), "DB 接続を確立");
            app.manage(db);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::db_status,
            commands::settings_get,
            commands::settings_set,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
