//! モジュール横断で使う同期系の共有ユーティリティ。

use std::sync::{Mutex, MutexGuard};

/// Mutex のポイズンを握りつぶしてロックする。ロック保持中にパニックしない
/// 実装なので安全側に倒す。
pub(crate) fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}
