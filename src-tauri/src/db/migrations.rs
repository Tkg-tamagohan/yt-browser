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
    // Phase 6: ライブチャットの保存基盤（設計書 §8 の DDL から該当分）。
    // chat_logs は削除アクションや未正規化イベントも原文で残し、
    // 表示側の絞り込みは NG フィルタ（filters）が担う。
    Migration {
        version: 5,
        name: "chat",
        sql: "CREATE TABLE chat_logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                video_id TEXT NOT NULL,
                posted_at_usec INTEGER NOT NULL,
                author_channel_id TEXT,
                author_name TEXT,
                kind TEXT NOT NULL DEFAULT 'text'
                  CHECK (kind IN ('text','superchat','membership','deleted','other')),
                message TEXT NOT NULL,
                amount_display TEXT,
                raw_json TEXT NOT NULL
              );
              CREATE INDEX idx_chat_video_ts ON chat_logs(video_id, posted_at_usec);
              -- 日本語の部分文字列検索に対応させるため trigram トークナイザを使う
              -- （unicode61 では「こんにちは世界」全体が 1 語になり、
              --   「こんにちは」で検索できない）。3 文字未満の検索語は
              --   部分一致に掛からない（trigram の制約）。
              CREATE VIRTUAL TABLE chat_logs_fts USING fts5(
                message, author_name, content='chat_logs', content_rowid='id',
                tokenize='trigram'
              );
              CREATE TRIGGER chat_logs_ai AFTER INSERT ON chat_logs BEGIN
                INSERT INTO chat_logs_fts(rowid, message, author_name)
                VALUES (new.id, new.message, new.author_name);
              END;
              CREATE TRIGGER chat_logs_ad AFTER DELETE ON chat_logs BEGIN
                INSERT INTO chat_logs_fts(chat_logs_fts, rowid, message, author_name)
                VALUES ('delete', old.id, old.message, old.author_name);
              END;
              CREATE TABLE filters (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                target TEXT NOT NULL CHECK (target IN
                  ('video_title','video_desc','channel_title','channel_id',
                   'chat_text','chat_author')),
                kind TEXT NOT NULL CHECK (kind IN ('literal','regex')),
                pattern TEXT NOT NULL,
                enabled INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
              );",
    },
    // Phase 7: お気に入りとカスタムプレイリスト（設計書 §8 の DDL から該当分、FR-7）。
    // 動画のメタ情報は videos を台帳として JOIN で取り、
    // favorites / playlist_items は video_id と並びだけを持つ。
    Migration {
        version: 6,
        name: "localdata",
        sql: "CREATE TABLE favorites (
                video_id TEXT PRIMARY KEY,
                added_at TEXT NOT NULL DEFAULT (datetime('now'))
              );
              CREATE TABLE playlists (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                sort_order INTEGER NOT NULL DEFAULT 0
              );
              CREATE TABLE playlist_items (
                playlist_id INTEGER NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,
                video_id TEXT NOT NULL,
                position INTEGER NOT NULL,
                PRIMARY KEY (playlist_id, video_id)
              );",
    },
    // Phase 7 レビュー対応: ライブラリ登録で先にできた videos 行（プレースホルダ）と
    // フィード投入済みの行を published_at の有無では判別できない（投稿日なしの
    // RSS エントリが毎回新着扱いになる）。独立したフラグ列を追加する。
    // バックフィルの出自判定は「投稿日」または「お気に入り・プレイリストからの
    // 未参照」で行う。プレースホルダは favorite_add / playlist_add の
    // トランザクションでのみ作られるため必ず参照を持ち、未参照で
    // 投稿日なしの行はフィード由来しかあり得ないので 1 に残す
    // （未参照のまま 0 にすると feed_list の ingested=1 フィルタで
    // アップグレード後に既存のフィード動画が非表示になる）。
    // 「投稿日なしかつ参照あり」の行だけは両方であり得るため 0 に揃える。
    // 実際に日付を欠いたフィード行だった場合は次回の投入で ingested=1 に
    // 確定し、一度だけ未読へ戻る（プレースホルダを 1 に誤認した場合の
    // 初回到達補完の永続的喪失より影響が小さい）。
    // その誤分類を速やかに解消するため、条件付き取得（ETag/Last-Modified）の
    // 状態をクリアし、全チャンネルで一度だけ無条件の再取得を強制する。
    Migration {
        version: 7,
        name: "videos_ingested_flag",
        sql: "ALTER TABLE videos
                ADD COLUMN ingested INTEGER NOT NULL DEFAULT 0;
              UPDATE videos SET ingested = 1
              WHERE published_at IS NOT NULL
                 OR video_id NOT IN (SELECT video_id FROM favorites
                                     UNION SELECT video_id FROM playlist_items);
              UPDATE channels SET rss_etag = NULL, rss_last_modified = NULL;",
    },
];
