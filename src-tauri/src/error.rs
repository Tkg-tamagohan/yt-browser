//! UI へ返すエラーの直列化形式（技術方針 L）。
//! モジュール境界の thiserror enum を commands 層で `{ code, message }` に畳み込む。

use serde::Serialize;

/// フロントエンドへ返すエラー。`{ code, message }` に直列化される。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UiError {
    pub code: String,
    pub message: String,
}

impl UiError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }

    /// 入力検証エラー。コマンド引数の検証に使う。
    pub fn invalid_input(message: impl Into<String>) -> Self {
        Self::new("invalid_input", message)
    }

    /// サーバ側の内部不整合。UI 側の分岐を持たない汎用エラー。
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new("internal", message)
    }
}

impl From<crate::db::DbError> for UiError {
    fn from(e: crate::db::DbError) -> Self {
        Self::new("db", e.to_string())
    }
}

impl From<crate::mpv::MpvError> for UiError {
    fn from(e: crate::mpv::MpvError) -> Self {
        // コードは UI 側の分岐用に細分する（未導入/ソケット不成立/IPC/対象なし）
        let code = match &e {
            crate::mpv::MpvError::Spawn(_) => "mpv_spawn",
            crate::mpv::MpvError::SocketTimeout => "mpv_socket_timeout",
            crate::mpv::MpvError::Ipc(_) => "mpv_ipc",
            crate::mpv::MpvError::NoSuchInstance(_) => "mpv_no_instance",
        };
        Self::new(code, e.to_string())
    }
}

impl From<crate::feed::FeedError> for UiError {
    fn from(e: crate::feed::FeedError) -> Self {
        let code = match &e {
            crate::feed::FeedError::NotFound => "feed_not_found",
            crate::feed::FeedError::Db(_) => "db",
            _ => "feed",
        };
        Self::new(code, e.to_string())
    }
}

impl From<crate::yt::YtError> for UiError {
    fn from(e: crate::yt::YtError) -> Self {
        let code = match &e {
            crate::yt::YtError::NotFound => "ytdlp_not_found",
            crate::yt::YtError::Timeout => "ytdlp_timeout",
            _ => "ytdlp",
        };
        Self::new(code, e.to_string())
    }
}
