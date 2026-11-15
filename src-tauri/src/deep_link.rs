//! カスタムスキーム `yt-browser://<action>?url=<encoded>` の受け取り
//! （FR-17・FR-29、仕様決定 AC・AX）。アクション（`open` / `queue`）の
//! 解釈はフロント側で行い、ここではスキーム URL をそのまま転送する。
//!
//! deep link での起動・URL 転送は 2 経路ある:
//!
//! - 未起動での起動（cold start）: OS が新しいプロセスを立て、argv に
//!   URL が入る。`deep_link().get_current()` で拾う
//! - 起動中での転送（warm start）: 2 つ目のプロセスが argv に URL を
//!   持って起動するが single-instance プラグインが抑制し、コールバックの
//!   argv 経由で既存プロセスへ届く
//!
//! 配送は `app://open_url` イベントで行い、リスナー未登録の間に届いた分は
//! `PendingOpenUrls` に保留して `take_open_urls` の初期ドレインで回収する。
//! 「保留に積むか emit だけにするか」の判定とドレインは同じ Mutex 内で行い、
//! ready 切替と挿入の間に URL が消える競合を避ける。
//! 各配送には `seq` を振り、保留分とイベントで同一配送が二度届く場合に
//! フロントが seq で重複除去できるようにする（URL ではなく配送単位で
//! 識別するので、同じリンクを後で再度開く操作は常に処理される）。

use std::sync::{
    atomic::{AtomicU64, Ordering},
    Mutex,
};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::util::lock;

/// フロントへ届ける 1 配送。`seq` はプロセス内で単調増加する配送識別子で、
/// 初期ドレインとイベントで同じ配送が二度届いたときの重複除去に使う。
#[derive(Clone, Serialize)]
pub struct OpenUrlItem {
    pub seq: u64,
    pub url: String,
}

/// 受け取ったがまだフロントへ渡していない open URL。
/// `ready` が立つまでは emit だけでは届かない可能性がある（リスナー未登録）
/// のでバッファへも積む。`take_open_urls` の初回ドレインで `ready` が立ち、
/// 以後は emit 経路だけを使う。判定とドレインは同じロック内で行うため、
/// ready 切替中に届いた URL が置き去りにならない。
pub struct PendingOpenUrls {
    inner: Mutex<PendingInner>,
    next_seq: AtomicU64,
}

struct PendingInner {
    items: Vec<OpenUrlItem>,
    ready: bool,
}

impl PendingOpenUrls {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(PendingInner {
                items: Vec::new(),
                ready: false,
            }),
            next_seq: AtomicU64::new(1),
        }
    }

    /// フロント側のドレインを行う。呼び出しで `ready` が立ち、
    /// 以後の URL はバッファへ積まず emit だけで届ける。
    /// ロック内で ready 切替と drain を一括で行う。
    pub fn drain(&self) -> Vec<OpenUrlItem> {
        let mut inner = lock(&self.inner);
        inner.ready = true;
        std::mem::take(&mut inner.items)
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
    let Some(pending) = app.try_state::<PendingOpenUrls>() else {
        tracing::warn!("PendingOpenUrls 未管理のため deep link を無視: {url}");
        return;
    };
    let item = OpenUrlItem {
        seq: pending.next_seq.fetch_add(1, Ordering::Relaxed),
        url: url.to_string(),
    };
    {
        let mut inner = lock(&pending.inner);
        if !inner.ready {
            inner.items.push(item.clone());
        }
    }
    let _ = app.emit("app://open_url", &item);
}

/// argv 内のスキーム URL をすべて処理する（single-instance コールバック用）。
pub fn handle_argv(app: &AppHandle, argv: &[String]) {
    for arg in argv {
        handle_open_url(app, arg);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn drain_marks_ready_and_empties_buffer() {
        let pending = PendingOpenUrls::new();
        {
            let mut inner = lock(&pending.inner);
            inner.items.push(OpenUrlItem {
                seq: 1,
                url: "yt-browser://open?url=a".to_string(),
            });
        }
        let first = pending.drain();
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].seq, 1);
        assert!(lock(&pending.inner).ready);
        assert!(pending.drain().is_empty());
    }

    #[test]
    fn drain_recovers_from_poisoned_mutex() {
        let pending = Arc::new(PendingOpenUrls::new());
        {
            let mut inner = lock(&pending.inner);
            inner.items.push(OpenUrlItem {
                seq: 7,
                url: "yt-browser://open?url=b".to_string(),
            });
        }
        let cloned = Arc::clone(&pending);
        let _ = std::thread::spawn(move || {
            let _guard = cloned.inner.lock().unwrap();
            panic!("intentional poison for test");
        })
        .join();
        let items = pending.drain();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].seq, 7);
    }
}
