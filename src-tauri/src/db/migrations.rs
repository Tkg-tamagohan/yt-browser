//! バージョン付きスキーママイグレーション（技術方針 K）。
//! `version` は連番で増やすだけにし、適用済みのものは変更しない。

pub struct Migration {
    pub version: u32,
    pub name: &'static str,
    pub sql: &'static str,
}

/// 設計書 §8 の DDL をフェーズごとに分割投入する。
/// Phase 0 は `settings`、Phase 1 は `watch_history`（実装計画の受け入れ条件）。
pub const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "settings",
        sql: "CREATE TABLE settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
              );",
    },
    Migration {
        version: 2,
        name: "watch_history",
        sql: "CREATE TABLE watch_history (
                video_id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                channel_id TEXT,
                channel_title TEXT,
                position_sec INTEGER NOT NULL DEFAULT 0,
                duration_sec INTEGER,
                last_watched_at TEXT NOT NULL DEFAULT (datetime('now')),
                completed INTEGER NOT NULL DEFAULT 0
              );
              CREATE INDEX idx_history_recent ON watch_history(last_watched_at DESC);",
    },
];
