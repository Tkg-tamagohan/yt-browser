//! カスタムスキーム `yt-browser://open?url=<encoded>` の受け取り
//! （FR-17、仕様決定 AC）。
//!
//! deep link での起動・URL 転送は 2 経路ある:
//!
//! - 未起動での起動（cold start）: OS が新しいプロセスを立て、argv に
//!   URL が入る。`deep_link().get_current()` で拾う
//! - 起動中での転送（warm start）: 2 つ目のプロセスが argv に URL を
//!   持って起動するが single-instance プラグインが抑制し、コールバックの
//!   argv 経由で既存プロセスへ届く
//!
//! どちらの経路でも URL は `PendingOpenUrls` に積み、`app://open_url`
//! イベントでフロントへ転送する。フロントのリスナー登録前に届いた分は
//! `take_open_urls` の初期ドレインで回収する。

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex,
};

use tauri::{AppHandle, Emitter, Manager};

/// 受け取ったがまだフロントへ渡していない open URL。
/// `ready` が立つまでは emit だけでは届かない可能性がある（リスナー未登録）
/// のでバッファへも積む。`take_open_urls` の初回ドレインで `ready` が立ち、
/// 以後は emit 経路だけを使う（両方へ渡すと重複処理になる）。
pub struct PendingOpenUrls {
    pub urls: Mutex<Vec<String>>,
    ready: AtomicBool,
}

impl PendingOpenUrls {
    pub fn new() -> Self {
        Self {
            urls: Mutex::new(Vec::new()),
            ready: AtomicBool::new(false),
        }
    }

    /// フロント側のドレイン完了を記録する（以後はバッファへ積まない）。
    pub fn mark_ready(&self) {
        self.ready.store(true, Ordering::Release);
    }
}

/// URL がこのアプリのスキームか。`yt-browser:` 始まりだけを受理する
/// （single-instance の argv には実行パス等の他引数も混ざるため）。
pub fn is_app_url(arg: &str) -> bool {
    arg.starts_with("yt-browser:")
}

/// URL をイベントでフロントへ転送する。リスナー登録前（`ready` 未設定）の
/// 受信は保留バッファへも積み、初期ドレインで回収する。emit 自体の失敗は
/// ドレインで回収するためエラーにはしない。
pub fn handle_open_url(app: &AppHandle, url: &str) {
    if !is_app_url(url) {
        return;
    }
    let url = url.to_string();
    // PendingOpenUrls は setup 内で manage 済み（single-instance の
    // コールバックが呼ばれるのも setup 完了後）なので state() で取る
    let pending = app.state::<PendingOpenUrls>();
    if !pending.ready.load(Ordering::Acquire) {
        pending.urls.lock().unwrap().push(url.clone());
    }
    let _ = app.emit("app://open_url", &url);
}

/// argv 内のスキーム URL をすべて処理する（single-instance コールバック用）。
pub fn handle_argv(app: &AppHandle, argv: &[String]) {
    for arg in argv {
        handle_open_url(app, arg);
    }
}
