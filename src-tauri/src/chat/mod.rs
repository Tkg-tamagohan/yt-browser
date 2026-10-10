//! ライブチャットのポーリングと正規化（設計書 §6.2〜§7、FR-6、FR-9）。
//!
//! `chat_start` で動画ごとのポーリングタスクを立て、watch ページの
//! `ytInitialData` から初期継続トークンを取って `get_live_chat` を繰り返す。
//! 応答 1 回分のアクションを `ChatEvent` に正規化し、NG 判定と重複除去を経て
//! 1 トランザクションで `chat_logs` へ保存したうえで `chat://message` に流す。
//! 原文は raw_json として残し、削除アクションや未知 renderer も `other` /
//! `deleted` で記録する（設計書 §6.3 の「保存は別レイヤ、表示は制御する」方針）。

mod normalize;
mod poller;

pub use poller::ChatPoller;

/// 設定キー: チャットポップアップ窓を最前面で開くか（FR-27、仕様決定 AT）。
/// 値は "on" / "off"。未設定・その他の値は "on"（最前面）として扱う。
pub(crate) const SETTING_CHAT_POPUP_ONTOP: &str = "chat.popup_ontop";
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
