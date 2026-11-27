//! yt-browser アプリケーション本体（設計書 §1.1）。
//! WebView スレッドは UI 描画と入力だけを持ち、重い処理は Tokio ワーカー側へ置く。

mod chat;
mod commands;
mod db;
mod deep_link;
mod deps;
mod diag;
mod error;
mod feed;
mod filter;
mod innertube;
mod livewait;
mod metaresolve;
mod model;
mod mpv;
pub mod native_host;
mod sponsor;
mod ui;
mod util;
mod yt;

use std::path::PathBuf;

use tauri::Manager;
use tauri_plugin_deep_link::DeepLinkExt;
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
        // app_log_dir は作成を保証しないため先に作る（未作成だと build() が失敗し
        // ファイル出力全体が無効になって stderr フォールバックへ落ちる）
        if let Err(e) = std::fs::create_dir_all(&dir) {
            eprintln!("ログディレクトリを作成できません: {e}。stderr のみに出力します");
            return None;
        }
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
    let app = tauri::Builder::default()
        // single-instance は他プラグインより先に登録する。
        // deep link 経由の 2 つ目の起動はここで抑制し、argv の URL を
        // 既存プロセスへ転送する（FR-17、仕様決定 AC）
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            deep_link::handle_argv(app, &argv);
        }))
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        // クリップボードへの書き込み（FR-34、仕様決定 BC）。
        // capabilities では write-text のみ許可し読み取りは付けない
        .plugin(tauri_plugin_clipboard_manager::init())
        // 新着のデスクトップ通知（FR-58、仕様決定 CI）。
        // 通知はネイティブ側（フィードポーラー）から出すため
        // WebView 向けの notification 権限は付けない
        .plugin(tauri_plugin_notification::init())
        // エクスポート/インポートのファイル選択（FR-62、仕様決定 CM）。
        // capabilities では open/save/message（確認ダイアログ）のみ許可する
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let log_dir = app.path().app_log_dir().ok();
            let log_guard = init_tracing(log_dir.clone());
            app.manage(log_guard);
            let db = db::Db::connect(&dir.join("yt-browser.db"))
                .map_err(|e| -> Box<dyn std::error::Error> { Box::new(e) })?;
            tracing::info!(path = %dir.display(), "DB 接続を確立");
            app.manage(db.clone());

            // Chrome 拡張の Native Messaging ホスト定義を冪等に自己登録する（仕様決定 AH・DM）
            let suppress_nm = cfg!(debug_assertions)
                && std::env::var_os("YB_DEV_NO_NM").is_some_and(|v| v == "1");
            if suppress_nm {
                tracing::info!("YB_DEV_NO_NM=1 のため Native Messaging 登録を省略する");
            } else {
                native_host::register(app.path().config_dir().ok(), &dir);
            }

            // mpv の IPC ソケット置き場。runtime_dir が取れない環境では app_data 配下に退避する
            let socket_dir = app.path().runtime_dir().unwrap_or_else(|_| dir.join("run"));
            std::fs::create_dir_all(&socket_dir)?;

            // ホイール分岐スクリプトを app_data へ書き出す（設計書 §4.2、--script で読ませる）。
            // 埋め込みソースと実体ファイルを同一バージョンに保つため起動ごとに上書きする。
            // 書き出しに失敗してもアプリは起動させる（ホイール分岐だけが無効になる）。
            let script_dir = dir.join("mpv");
            let wheel_script = match std::fs::create_dir_all(&script_dir)
                .and_then(|()| std::fs::write(script_dir.join("wheel.lua"), mpv::WHEEL_LUA))
            {
                Ok(()) => Some(script_dir.join("wheel.lua")),
                Err(e) => {
                    tracing::warn!(error = %e, "wheel.lua の書き出しに失敗");
                    None
                }
            };

            // InnerTube クライアント（設計書 §6.1）。関連動画・チャット・履歴チャンネル補完が共用する。
            let innertube = std::sync::Arc::new(innertube::InnerTube::new());
            app.manage(innertube.clone());

            let ytdlp_resolver = yt::YtDlpResolver::new(app.path().resource_dir().ok());
            app.manage(ytdlp_resolver.clone());
            // mpv から chat への直接依存を避けるため、動画離脱時のポップアップ自動クローズは
            // ここでコールバックとして PlayerManager へ注入する（FR-93、仕様決定 DV）
            let popup_close_hook: mpv::VideoEndHook = {
                let app_handle = app.handle().clone();
                let hook_db = db.clone();
                std::sync::Arc::new(
                    move |video_id, player_closed, video_changed, has_other_player| {
                        chat::popup::handle_player_video_end(
                            &app_handle,
                            &hook_db,
                            video_id,
                            player_closed,
                            video_changed,
                            has_other_player,
                        );
                    },
                )
            };
            // mpv から innertube 等への直接依存を避けるため、未解決動画の履歴チャンネル補完は
            // コールバックフックとして PlayerManager へ注入する（仕様決定 DY）。
            // 同一 video_id の並列重複リクエストを InFlightSet で抑止し、
            // DB の watch_history.channel_id が空のときだけ外部解決する。
            let video_start_hook: mpv::VideoStartHook = {
                let hook_db = db.clone();
                let hook_innertube = innertube.clone();
                let hook_resolver = ytdlp_resolver.clone();
                let in_flight = InFlightSet::new();
                std::sync::Arc::new(move |video_id| {
                    let Some(guard) = in_flight.try_acquire(video_id) else {
                        return;
                    };
                    let vid = video_id.to_string();
                    let db = hook_db.clone();
                    let innertube = std::sync::Arc::clone(&hook_innertube);
                    let resolver = hook_resolver.clone();
                    tauri::async_runtime::spawn(async move {
                        let _guard = guard;
                        if db.is_history_suppressed(&vid) {
                            return;
                        }
                        let has_channel = match db.history_get(&vid) {
                            Ok(Some(h)) => h
                                .channel_id
                                .as_deref()
                                .map(str::trim)
                                .is_some_and(|s| !s.is_empty()),
                            Ok(None) => false,
                            Err(e) => {
                                tracing::warn!(video_id = %vid, error = %e, "履歴チャンネル確認に失敗");
                                return;
                            }
                        };
                        if has_channel {
                            return;
                        }
                        let resolved =
                            commands::feed::resolve_video_channel(&vid, &db, &innertube, &resolver)
                                .await;
                        if resolved.channel_id.is_some() || resolved.channel_title.is_some() {
                            if let Err(e) = db.history_set_channel(
                                &vid,
                                resolved.channel_id.as_deref(),
                                resolved.channel_title.as_deref(),
                                resolved.title.as_deref(),
                            ) {
                                tracing::warn!(video_id = %vid, error = %e, "履歴チャンネルの書き戻しに失敗");
                            }
                        }
                    });
                })
            };
            app.manage(mpv::PlayerManager::new(
                app.handle().clone(),
                db.clone(),
                socket_dir,
                ytdlp_resolver,
                wheel_script,
                log_dir,
                popup_close_hook,
                video_start_hook,
            ));

            // NG 評価器の共有所有（chat 表示だけでなくフィード・検索・
            // 関連動画の動画系 NG 判定も通るため chat ではなく filter 配下に置く）。
            let ng = std::sync::Arc::new(filter::NgMatcher::new(db.clone(), app.handle().clone()));
            if let Err(e) = ng.refresh() {
                tracing::warn!(error = %e, "NG フィルタの初期読み込みに失敗");
            }
            app.manage(ng.clone());

            // ライブチャットのポーラー（設計書 §6.2）。
            let chat = std::sync::Arc::new(chat::ChatPoller::new(
                app.handle().clone(),
                innertube.clone(),
                ng,
            ));
            app.manage(chat);

            // 購読フィードのポーラー（設計書 §1.2）。TICK ごとに期限の来た
            // チャンネルの RSS を条件付き取得する。
            let poller = feed::FeedPoller::new(db.clone(), app.handle().clone());
            app.manage(poller.clone());
            // 起動時の自動既読（FR-64、仕様決定 CO）。設定が無効なら何もしない
            poller.apply_auto_read();
            tauri::async_runtime::spawn(poller.run());

            // メタの遅延解決ワーカー（Phase 62、FR-53・FR-56・FR-57、
            // 仕様決定 CD・CG・CH）。投稿日時・kind の欠けた videos 行を
            // InnerTube `player` 応答で後から埋める。フィードの新規投入時に
            // 即時起こされる。
            let meta_resolver =
                metaresolve::MetaResolver::new(db.clone(), app.handle().clone(), innertube.clone());
            app.manage(meta_resolver.clone());
            tauri::async_runtime::spawn(meta_resolver.run());

            // ライブ配信待機のポーラー（FR-44、仕様決定 BT）。
            // DB の待機リストを復元し、予定時刻に応じた間隔で InnerTube
            // `player` の放送状態を巡回する。再起動後もそのまま再開される
            let live_wait = livewait::LiveWaitPoller::new(db, app.handle().clone(), innertube);
            app.manage(live_wait.clone());
            tauri::async_runtime::spawn(live_wait.run());

            // deep link の保留バッファと cold start 分の取り込み。
            // 起動中への転送は single-instance のコールバック側が担う。
            // PendingOpenUrls 登録後に Native Messaging 向けローカル IPC を待ち受ける
            app.manage(deep_link::PendingOpenUrls::new());
            native_host::start_ipc_listener(app.handle().clone(), &dir);
            if let Ok(Some(urls)) = app.deep_link().get_current() {
                for url in urls {
                    deep_link::handle_open_url(app.handle(), url.as_str());
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::db_status,
            commands::settings_get,
            commands::settings_set,
            commands::monitor_list,
            commands::windowing_system,
            commands::play_video,
            commands::player_list,
            commands::player_control,
            commands::player_close,
            commands::audio_device_list,
            commands::player_set_queue,
            commands::player_switch,
            commands::player_screenshot,
            commands::history_get,
            commands::history_list,
            commands::history_remove,
            commands::history_restore,
            commands::history_stats,
            commands::favorite_add,
            commands::favorite_remove,
            commands::favorite_restore,
            commands::favorite_list,
            commands::playlist_list,
            commands::playlist_create,
            commands::playlist_rename,
            commands::playlist_delete,
            commands::playlist_items,
            commands::playlist_add,
            commands::playlist_remove,
            commands::playlist_item_restore,
            commands::playlist_import,
            commands::playlist_reorder,
            commands::playlist_reorder_lists,
            commands::playlist_sort,
            commands::playlist_reverse,
            commands::watch_later_add,
            commands::watch_later_remove,
            commands::watch_later_list,
            commands::library_search,
            commands::take_open_urls,
            commands::queue_save,
            commands::queue_load,
            commands::queue_save_as_playlist,
            commands::queue_playlist_save,
            commands::queue_playlist_clear,
            commands::queue_playlist_load,
            commands::video_meta_lookup,
            commands::video_meta_resolve,
            commands::ytdlp_status,
            commands::ytdlp_update,
            commands::deps_status,
            commands::setup_deps_pending,
            commands::subscribe_channel,
            commands::playing_channel,
            commands::unsubscribe_channel,
            commands::list_channels,
            commands::set_channel_category,
            commands::list_categories,
            commands::create_category,
            commands::category_reorder,
            commands::list_feed,
            commands::mark_read,
            commands::feed_refresh,
            commands::feed_backfill,
            commands::search,
            commands::get_related,
            commands::get_description,
            commands::block_channel,
            commands::unblock_channel,
            commands::blocked_channels,
            commands::chat_start,
            commands::chat_stop,
            commands::chat_popup_open,
            commands::chat_popup_return,
            commands::filter_add,
            commands::filter_remove,
            commands::filter_list,
            commands::live_wait_add,
            commands::live_wait_remove,
            commands::live_wait_list,
            commands::bookmark_add,
            commands::bookmark_list,
            commands::bookmark_remove,
            commands::bookmark_set_label,
            commands::data_export,
            commands::data_import,
            commands::db_size,
            commands::db_vacuum,
            commands::db_cleanup_orphans,
            commands::diag_export,
            commands::diag_open_log_dir,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    // アプリ終了時に全 mpv インスタンスを停止する
    app.run(|handle, event| {
        if matches!(event, tauri::RunEvent::Exit) {
            if let Some(manager) = handle.try_state::<mpv::PlayerManager>() {
                tauri::async_runtime::block_on(manager.close_all());
            }
            if let Some(poller) = handle.try_state::<std::sync::Arc<chat::ChatPoller>>() {
                poller.stop_all();
            }
            // 正常終了ではローカル IPC のソケット残骸を残さない
            if let Ok(dir) = handle.path().app_data_dir() {
                native_host::cleanup_ipc_endpoint(&dir);
            }
        }
    });
}

/// 同一 `video_id` の並列重複解決を抑止する RAII ガード付き集合（仕様決定 DY）。
#[derive(Clone)]
struct InFlightSet {
    inner: std::sync::Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
}

struct InFlightGuard {
    inner: std::sync::Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
    video_id: String,
}

impl Drop for InFlightGuard {
    fn drop(&mut self) {
        util::lock(&self.inner).remove(&self.video_id);
    }
}

impl InFlightSet {
    fn new() -> Self {
        Self {
            inner: std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())),
        }
    }

    fn try_acquire(&self, video_id: &str) -> Option<InFlightGuard> {
        let vid = video_id.to_string();
        if !util::lock(&self.inner).insert(vid.clone()) {
            return None;
        }
        Some(InFlightGuard {
            inner: std::sync::Arc::clone(&self.inner),
            video_id: vid,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn in_flight_set_deduplicates_and_releases_on_drop() {
        let set = InFlightSet::new();
        let g1 = set.try_acquire("vid00000001");
        assert!(g1.is_some());
        assert!(set.try_acquire("vid00000001").is_none());
        let g2 = set.try_acquire("vid00000002");
        assert!(g2.is_some());
        drop(g1);
        assert!(set.try_acquire("vid00000001").is_some());
        drop(g2);
    }
}
