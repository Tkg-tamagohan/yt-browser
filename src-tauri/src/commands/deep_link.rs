//! deep link 系の Tauri コマンド（FR-17、仕様決定 AC）。
//! 受信と保留の実体は `crate::deep_link` にある。

use tauri::State;

use crate::deep_link::PendingOpenUrls;
use crate::error::UiError;

/// 保留中の open URL を取り出して空にする。フロントの初期化時に 1 回呼ぶ。
/// 呼び出しで `ready` が立ち、以後の URL はバッファへ積まず emit だけで届ける。
#[tauri::command]
pub fn take_open_urls(pending: State<'_, PendingOpenUrls>) -> Result<Vec<String>, UiError> {
    // ロック中に ready を立てる。ここから drain までの間に届いた URL は
    // バッファを通らず emit で処理される（重複させないための順序）
    let mut urls = pending.urls.lock().unwrap();
    pending.mark_ready();
    Ok(urls.drain(..).collect())
}
