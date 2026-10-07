//! UI とやり取りする共通型。

use serde::Serialize;

/// `db_status` コマンドの戻り値。DB の生存確認に使う。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DbStatus {
    /// 適用済みマイグレーションの最新バージョン。未適用なら 0。
    pub schema_version: u32,
}
