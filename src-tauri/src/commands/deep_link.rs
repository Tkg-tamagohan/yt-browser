//! deep link 系の Tauri コマンド（FR-17、仕様決定 AC）。
//! 受信と保留の実体は `crate::deep_link` にある。

use tauri::State;

use crate::deep_link::{OpenUrlItem, PendingOpenUrls};
use crate::error::UiError;

/// 保留中の open URL を取り出して空にする。フロントの初期化時に 1 回呼ぶ。
/// 呼び出しで `ready` が立ち、以後の URL はバッファへ積まず emit だけで届ける。
/// フロントは listen 完了後に呼ぶこと（先に ready が立つと、リスナー不在の
/// 間に届いた URL がイベントだけでは届かず失われる）。
#[tauri::command]
pub fn take_open_urls(pending: State<'_, PendingOpenUrls>) -> Result<Vec<OpenUrlItem>, UiError> {
    Ok(pending.drain())
}
