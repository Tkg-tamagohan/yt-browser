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
    // Phase 4 修正: 初期実装が feed 直下の UC プレフィックスなし channelId を
    // 保存していた環境の修復。旧形式は `UC` を欠いた 22 文字の識別子
    // （先頭が `UC`/`uc` の場合もあるため NOT LIKE ではなく長さで判定）。
    // 正規 `UC` 付き行が既にある衝突側は、削除前にカテゴリ・購読日時・
    // ポーリングメタの欠落分を UC 付き行へ引き継いでから重複を消す。
    Migration {
        version: 4,
        name: "channel_id_uc_normalize",
        sql: "UPDATE channels
              SET category_id = COALESCE(
                    category_id,
                    (SELECT c2.category_id FROM channels c2
                     WHERE c2.channel_id = substr(channels.channel_id, 3))),
                  subscribed_at = MIN(
                    subscribed_at,
                    (SELECT c2.subscribed_at FROM channels c2
                     WHERE c2.channel_id = substr(channels.channel_id, 3))),
                  last_polled_at = COALESCE(
                    last_polled_at,
                    (SELECT c2.last_polled_at FROM channels c2
                     WHERE c2.channel_id = substr(channels.channel_id, 3))),
                  rss_etag = COALESCE(
                    rss_etag,
                    (SELECT c2.rss_etag FROM channels c2
                     WHERE c2.channel_id = substr(channels.channel_id, 3))),
                  rss_last_modified = COALESCE(
                    rss_last_modified,
                    (SELECT c2.rss_last_modified FROM channels c2
                     WHERE c2.channel_id = substr(channels.channel_id, 3)))
              WHERE channel_id LIKE 'UC%' AND length(channel_id) = 24
                AND EXISTS (
                  SELECT 1 FROM channels c2
                  WHERE c2.channel_id = substr(channels.channel_id, 3)
                    AND length(c2.channel_id) = 22
                );
              DELETE FROM channels
              WHERE length(channel_id) = 22
                AND EXISTS (
                  SELECT 1 FROM channels c2
                  WHERE c2.channel_id = 'UC' || channels.channel_id
                );
              UPDATE channels SET channel_id = 'UC' || channel_id
              WHERE length(channel_id) = 22;
              UPDATE OR IGNORE blocked_channels SET channel_id = 'UC' || channel_id
              WHERE length(channel_id) = 22;
              DELETE FROM blocked_channels WHERE length(channel_id) = 22;
              UPDATE videos SET channel_id = 'UC' || channel_id
              WHERE length(channel_id) = 22;
              UPDATE watch_history SET channel_id = 'UC' || channel_id
              WHERE length(channel_id) = 22;",
    },
];
