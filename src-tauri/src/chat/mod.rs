//! ライブチャットのポーリングと正規化（設計書 §6.2〜§7、FR-6、FR-9）。
//!
//! `chat_start` で動画ごとのポーリングタスクを立て、watch ページの
//! `ytInitialData` から初期継続トークンを取って `get_live_chat` を繰り返す。
//! 応答 1 回分のアクションを `ChatEvent` に正規化し、NG 判定と重複除去を経て
//! `chat://message` に流す。チャットは DB へ保存しない（仕様決定 AV）。
//! 削除アクションや未知 renderer も `deleted` / `other` のイベントとして記録する
//! （設計書 §6.3）。

mod normalize;
mod poller;
pub(crate) mod popup;

pub use poller::ChatPoller;

/// 設定キー: チャットポップアップ窓を最前面で開くか（FR-27、仕様決定 AT）。
/// 値は "on" / "off"。未設定・その他の値は "on"（最前面）として扱う。
pub(crate) const SETTING_CHAT_POPUP_ONTOP: &str = "chat.popup_ontop";
/// 設定キー: チャットポップアップ窓の位置とサイズ（FR-67、仕様決定 CR）。
/// 値は `WxH+x+y`（論理ピクセル、座標は符号付き）。未設定・形式外は
/// 既定の位置とサイズで開く。
pub(crate) const SETTING_CHAT_POPUP_GEOMETRY: &str = "chat.popup_geometry";
/// 設定キー: チャットポップアップに投稿者名を表示するか
/// （FR-68、仕様決定 CS）。値は "on" / "off"。
/// 未設定・その他の値は "off"（非表示）として扱う。
/// フロントが settings_get で読む表示側だけの設定でバックエンドでは
/// 参照しないが、設定キー表との両方向照合（check_docs_consistency）の
/// ため定数として保持する。
#[allow(dead_code)]
pub(crate) const SETTING_CHAT_POPUP_SHOW_AUTHOR: &str = "chat.popup_show_author";
/// 設定キー: チャットポップアップの文字サイズ px（FR-68、仕様決定 CS）。
/// 10〜32 の数値だけを受理し、未設定・非数・範囲外は既定 14。
/// フロント側だけで読む設定（理由は上と同じ）。
#[allow(dead_code)]
pub(crate) const SETTING_CHAT_POPUP_FONT_SIZE: &str = "chat.popup_font_size";
/// 設定キー: チャットポップアップ内容の不透明度（FR-68、仕様決定 CS）。
/// 0.2〜1.0 の数値だけを受理し、未設定・非数・範囲外は既定 1.0。
/// フロント側だけで読む設定（理由は上と同じ）。
#[allow(dead_code)]
pub(crate) const SETTING_CHAT_POPUP_OPACITY: &str = "chat.popup_opacity";
/// ポップアップ窓の利用者キー（動画単位に 1 窓のため固定値）。
pub(crate) const POPUP_CONSUMER: &str = "popup";

/// チャットポップアップ窓のラベル。動画単位に 1 窓のため video_id から導く
/// （capability の `chat-popup-*` パターンと対応）。
pub fn popup_label(video_id: &str) -> String {
    format!("chat-popup-{video_id}")
}

/// 埋め込みパネルの利用者キー（起票インスタンス単位）。
pub fn panel_consumer(instance_id: Option<u32>) -> String {
    instance_id
        .map(|i| format!("panel:{i}"))
        .unwrap_or_else(|| "panel:?".to_string())
}
