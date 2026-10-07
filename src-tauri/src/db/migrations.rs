//! バージョン付きスキーママイグレーション（技術方針 K）。
//! `version` は連番で増やすだけにし、適用済みのものは変更しない。

pub struct Migration {
    pub version: u32,
    pub name: &'static str,
    pub sql: &'static str,
}

/// 設計書 §8 の DDL をフェーズごとに分割投入する。
/// Phase 0 は `settings` のみ（実装計画 Phase 0 の受け入れ条件）。
pub const MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    name: "settings",
    sql: "CREATE TABLE settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
          );",
}];
