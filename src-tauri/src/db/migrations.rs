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
    // Phase 4: 購読フィードの基盤（設計書 §8 の DDL から該当分）
    Migration {
        version: 3,
        name: "feed",
        sql: "CREATE TABLE categories (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                sort_order INTEGER NOT NULL DEFAULT 0
              );
              CREATE TABLE channels (
                channel_id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                thumbnail_url TEXT,
                category_id INTEGER REFERENCES categories(id) ON DELETE SET NULL,
                subscribed_at TEXT NOT NULL DEFAULT (datetime('now')),
                last_polled_at TEXT,
                rss_etag TEXT,
                rss_last_modified TEXT
              );
              CREATE TABLE blocked_channels (
                channel_id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
              );
              CREATE TABLE videos (
                video_id TEXT PRIMARY KEY,
                channel_id TEXT NOT NULL,
                channel_title TEXT,
                title TEXT NOT NULL,
                thumbnail_url TEXT,
                published_at TEXT,
                duration_sec INTEGER,
                kind TEXT NOT NULL DEFAULT 'video'
                  CHECK (kind IN ('video','short','live','upcoming')),
                is_read INTEGER NOT NULL DEFAULT 1,
                first_seen_at TEXT NOT NULL DEFAULT (datetime('now'))
              );
              CREATE INDEX idx_videos_channel_pub ON videos(channel_id, published_at DESC);
              CREATE INDEX idx_videos_unread ON videos(is_read, published_at DESC);",
    },
];
