//! Tauri invoke ハンドラ（設計書 §2, §3.1）。
//! 入力検証と UI 向けの直列化に徹し、実処理は各モジュールへ委譲する。
//! コマンド本体はドメイン別の子モジュールに置き、ここで再エクスポートする。

mod chat;
mod deep_link;
mod feed;
mod filter;
mod library;
mod player;
mod search;
mod system;

pub use chat::*;
pub use deep_link::*;
pub use feed::*;
pub use filter::*;
pub use library::*;
pub use player::*;
pub use search::*;
pub use system::*;

use crate::error::UiError;
use crate::model::normalize_video_id;

/// 入力（URL 各形式または動画 ID）を動画 ID へ正規化する共通処理。
fn parse_video_id(input: &str) -> Result<String, UiError> {
    normalize_video_id(input).ok_or_else(|| {
        UiError::invalid_input("YouTube の動画 URL または 11 文字の動画 ID を入力してください")
    })
}
