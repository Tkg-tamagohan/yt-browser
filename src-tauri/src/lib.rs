//! yt-browser アプリケーション本体（設計書 §1.1）。
//! WebView スレッドは UI 描画と入力だけを持ち、重い処理は Tokio ワーカー側へ置く。

mod commands;
mod db;
mod error;
mod model;

use std::path::PathBuf;

use tauri::Manager;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{fmt, EnvFilter};

/// 設計書 §9.3: stderr と日次ローリングファイルの双方へ出力する。
/// Windows リリース（GUI サブシステム）ではコンソールが無いため、ファイル出力が必須になる。
/// ただしログ保存先の障害でアプリ自体を起動不能にはしない。
/// ファイル出力の初期化に失敗した場合は stderr のみにフォールバックする。
/// 返す WorkerGuard はアプリ終了まで保持すること。drop するとファイルへ残ログが届かない。
fn init_tracing(log_dir: Option<PathBuf>) -> Option<WorkerGuard> {
    // build() は当日のログファイルを append で実際に開くため、
    // ディレクトリ作成可否だけではなく実際の書き込み可否まで検証できる
    let file = log_dir.and_then(|dir| {
        match RollingFileAppender::builder()
            .rotation(Rotation::DAILY)
            .filename_prefix("yt-browser.log")
            .build(&dir)
        {
            Ok(appender) => {
                let (writer, guard) = tracing_appender::non_blocking(appender);
                Some((writer, guard))
            }
            Err(e) => {
                eprintln!("ログファイル出力を初期化できません: {e}。stderr のみに出力します");
                None
            }
        }
    });

    // ファイル側は ANSI エスケープを無効化し、プレーンテキストで残す
    let (file_layer, guard) = match file {
        Some((w, g)) => (Some(fmt::layer().with_writer(w).with_ansi(false)), Some(g)),
        None => (None, None),
    };

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::new(if cfg!(debug_assertions) {
            "debug"
        } else {
            "info"
        })
    });
    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_writer(std::io::stderr))
        .with(file_layer)
        .init();
    guard
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let log_dir = app.path().app_log_dir().ok();
            let log_guard = init_tracing(log_dir);
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
