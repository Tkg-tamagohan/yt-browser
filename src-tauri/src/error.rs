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
}

impl From<crate::db::DbError> for UiError {
    fn from(e: crate::db::DbError) -> Self {
        Self::new("db", e.to_string())
    }
}
