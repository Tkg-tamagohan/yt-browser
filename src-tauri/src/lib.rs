//! yt-browser アプリケーション本体（設計書 §1.1）。
//! WebView スレッドは UI 描画と入力だけを持ち、重い処理は Tokio ワーカー側へ置く。

mod commands;
mod db;
mod error;
mod model;

use tauri::Manager;
use tracing_subscriber::EnvFilter;

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::new(if cfg!(debug_assertions) {
            "debug"
        } else {
            "info"
        })
    });
    tracing_subscriber::fmt().with_env_filter(filter).init();
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    init_tracing();
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
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
