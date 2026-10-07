//! SQLite 接続とバージョン管理されたマイグレーション（技術方針 K）。
//!
//! rusqlite は同期 API のため、接続は `Mutex<Connection>` 1 本に集約する（設計書 §1.2）。
//! 書き込みは短いトランザクションに収め、接続プールは持たない。

mod migrations;

use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

use rusqlite::Connection;
use thiserror::Error;

use crate::model::{Category, Channel, FeedFilter, FeedItem, WatchHistory};

/// `videos` への新規挿入 1 件分（`video_insert_new` の引数）。
#[derive(Debug, Clone)]
pub struct NewVideo<'a> {
    pub video_id: &'a str,
    pub channel_id: &'a str,
    pub channel_title: &'a str,
    pub title: &'a str,
    pub thumbnail_url: Option<&'a str>,
    pub published_at: Option<&'a str>,
    /// 'video' | 'short' | 'live' | 'upcoming'（videos.kind の CHECK 制約）。
    pub kind: &'a str,
}

/// `feed_ingest` / `feed_subscribe` の戻り値。
#[derive(Debug, Clone, Copy, Default)]
pub struct IngestOutcome {
    /// 新たに INSERT された動画数。
    pub inserted: usize,
    /// 既存行の既読フラグを未読に戻した数（初回購読投入でのみ発生）。
    pub unread_changed: usize,
}

impl IngestOutcome {
    /// UI 更新が必要な変化（新規挿入または未読への復帰）があったか。
    pub fn touched(&self) -> bool {
        self.inserted + self.unread_changed > 0
    }
}

/// `feed_subscribe` の引数一式。
#[derive(Debug)]
pub struct SubscribeArgs<'a> {
    pub channel_id: &'a str,
    pub title: &'a str,
    pub thumbnail_url: Option<&'a str>,
    pub category_id: Option<i64>,
    pub entries: &'a [NewVideo<'a>],
    pub etag: Option<&'a str>,
    pub last_modified: Option<&'a str>,
}

/// ポーラーが逐次処理するチャンネルの条件付き取得メタ（設計書 §8 channels 表のサブセット）。
#[derive(Debug, Clone)]
pub struct PollTarget {
    pub channel_id: String,
    pub title: String,
    pub rss_etag: Option<String>,
    pub rss_last_modified: Option<String>,
}

#[derive(Debug, Error)]
pub enum DbError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("接続ミューテックスがポイズンされた")]
    Poisoned,
    #[error("マイグレーション v{version} ({name}) の適用に失敗: {source}")]
    Migration {
        version: u32,
        name: &'static str,
        source: rusqlite::Error,
    },
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// アプリ全体で共有する DB 接続。`Clone` は同一接続を共有する
/// （`Mutex<Connection>` は設計書 §1.2 の通り 1 本のみ）。
#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
}

impl Db {
    /// `path` のデータベースを開き、未適用マイグレーションをすべて適用する。
    pub fn connect(path: &Path) -> Result<Self, DbError> {
        let conn = Connection::open(path)?;
        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        db.init_pragmas()?;
        db.migrate()?;
        Ok(db)
    }

    /// テスト用のインメモリ接続。
    #[cfg(test)]
    fn connect_in_memory() -> Result<Self, DbError> {
        let conn = Connection::open_in_memory()?;
        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        db.init_pragmas()?;
        db.migrate()?;
        Ok(db)
    }

    fn lock(&self) -> Result<MutexGuard<'_, Connection>, DbError> {
        self.conn.lock().map_err(|_| DbError::Poisoned)
    }

    fn init_pragmas(&self) -> Result<(), DbError> {
        let conn = self.lock()?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        Ok(())
    }

    fn migrate(&self) -> Result<(), DbError> {
        let mut conn = self.lock()?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_migrations (
               version INTEGER PRIMARY KEY,
               applied_at TEXT NOT NULL DEFAULT (datetime('now'))
             );",
        )?;
        let current: u32 = conn.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
            [],
            |row| row.get(0),
        )?;
        for m in migrations::MIGRATIONS
            .iter()
            .filter(|m| m.version > current)
        {
            let tx = conn.transaction()?;
            tx.execute_batch(m.sql)
                .map_err(|source| DbError::Migration {
                    version: m.version,
                    name: m.name,
                    source,
                })?;
            tx.execute(
                "INSERT INTO schema_migrations (version) VALUES (?1)",
                [m.version],
            )
            .map_err(|source| DbError::Migration {
                version: m.version,
                name: m.name,
                source,
            })?;
            tx.commit()?;
            tracing::info!(version = m.version, name = m.name, "マイグレーション適用");
        }
        Ok(())
    }

    /// 現在のスキーマバージョン（適用済みの最大 version）。
    pub fn schema_version(&self) -> Result<u32, DbError> {
        let conn = self.lock()?;
        let v = conn.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
            [],
            |row| row.get(0),
        )?;
        Ok(v)
    }

    pub fn setting_get(&self, key: &str) -> Result<Option<String>, DbError> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare("SELECT value FROM settings WHERE key = ?1")?;
        let mut rows = stmt.query([key])?;
        match rows.next()? {
            Some(row) => Ok(Some(row.get(0)?)),
            None => Ok(None),
        }
    }

    pub fn setting_set(&self, key: &str, value: &str) -> Result<(), DbError> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            rusqlite::params![key, value],
        )?;
        Ok(())
    }

    /// 再生開始時に履歴行を確保する。既存行は位置や完了状態を壊さない。
    pub fn history_upsert(&self, video_id: &str) -> Result<(), DbError> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO watch_history (video_id, title) VALUES (?1, '')
             ON CONFLICT (video_id) DO NOTHING",
            [video_id],
        )?;
        Ok(())
    }

    /// 再生中・終了時の進捗保存。タイトルは空文字なら既存値を維持する
    /// （media-title 未確定の早い保存で取れたタイトルを消さないため）。
    /// `completed` が true のときは位置を 0 に戻す（次回は先頭から再生）。
    pub fn history_update_progress(
        &self,
        video_id: &str,
        title: &str,
        position_sec: f64,
        duration_sec: Option<i64>,
        completed: bool,
    ) -> Result<(), DbError> {
        let position = if completed {
            0
        } else {
            position_sec.max(0.0) as i64
        };
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO watch_history
               (video_id, title, position_sec, duration_sec, last_watched_at, completed)
             VALUES (?1, ?2, ?3, ?4, datetime('now'), ?5)
             ON CONFLICT (video_id) DO UPDATE SET
               title = CASE WHEN excluded.title <> '' THEN excluded.title
                            ELSE watch_history.title END,
               position_sec = excluded.position_sec,
               duration_sec = COALESCE(excluded.duration_sec, watch_history.duration_sec),
               last_watched_at = excluded.last_watched_at,
               completed = excluded.completed",
            rusqlite::params![video_id, title, position, duration_sec, completed as i64,],
        )?;
        Ok(())
    }

    pub fn channel_get(&self, channel_id: &str) -> Result<Option<Channel>, DbError> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT channel_id, title, thumbnail_url, category_id,
                    subscribed_at, last_polled_at
             FROM channels WHERE channel_id = ?1",
        )?;
        let mut rows = stmt.query([channel_id])?;
        match rows.next()? {
            Some(row) => Ok(Some(Channel {
                channel_id: row.get(0)?,
                title: row.get(1)?,
                thumbnail_url: row.get(2)?,
                category_id: row.get(3)?,
                subscribed_at: row.get(4)?,
                last_polled_at: row.get(5)?,
            })),
            None => Ok(None),
        }
    }

    /// 購読一覧（UI 用）。
    pub fn channel_list(&self) -> Result<Vec<Channel>, DbError> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT channel_id, title, thumbnail_url, category_id,
                    subscribed_at, last_polled_at
             FROM channels ORDER BY title COLLATE NOCASE",
        )?;
        let mut rows = stmt.query([])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(Channel {
                channel_id: row.get(0)?,
                title: row.get(1)?,
                thumbnail_url: row.get(2)?,
                category_id: row.get(3)?,
                subscribed_at: row.get(4)?,
                last_polled_at: row.get(5)?,
            });
        }
        Ok(out)
    }

    /// ポーラー用: 全購読チャンネルの条件付き取得メタ。
    pub fn channel_poll_targets(&self) -> Result<Vec<PollTarget>, DbError> {
        let conn = self.lock()?;
        let mut stmt =
            conn.prepare("SELECT channel_id, title, rss_etag, rss_last_modified FROM channels")?;
        let mut rows = stmt.query([])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(PollTarget {
                channel_id: row.get(0)?,
                title: row.get(1)?,
                rss_etag: row.get(2)?,
                rss_last_modified: row.get(3)?,
            });
        }
        Ok(out)
    }

    /// 304 応答時: ポーリング時刻だけを更新する。
    pub fn channel_mark_polled(&self, channel_id: &str) -> Result<(), DbError> {
        let conn = self.lock()?;
        conn.execute(
            "UPDATE channels SET last_polled_at = datetime('now') WHERE channel_id = ?1",
            [channel_id],
        )?;
        Ok(())
    }

    /// フィード取得結果の投入。チャンネルの存在確認〜エントリ挿入〜取得メタ更新を
    /// 一トランザクションで行い、ポーリング中の購読解除（`channel_delete`）との競合を防ぐ。
    /// 戻り値は `Some(IngestOutcome)`。チャンネルが既に存在しなければ `None`（ロールバック）。
    /// なお ETag / Last-Modified は 200 応答に無ければ NULL で上書きする
    /// （欠落した validator を残すと次回以降の条件付き取得が腐る）。
    pub fn feed_ingest(
        &self,
        channel_id: &str,
        entries: &[NewVideo<'_>],
        etag: Option<&str>,
        last_modified: Option<&str>,
    ) -> Result<Option<IngestOutcome>, DbError> {
        let mut conn = self.lock()?;
        let tx = conn.transaction()?;
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM channels WHERE channel_id = ?1)",
            [channel_id],
            |r| r.get(0),
        )?;
        if !exists {
            return Ok(None);
        }
        let out = ingest_rows(&tx, channel_id, entries, false, etag, last_modified)?;
        tx.commit()?;
        Ok(Some(out))
    }

    /// 購読登録＋初回投入を一トランザクションで行う。
    /// 「channels に行が無い」= 新規購読（または解除済みの再購読）の場合だけ
    /// 既存動画の既読フラグを未読へ戻す（初回投入は未読の仕様）。
    /// 新規判定がトランザクション内なので、並行して走った二つの購読要求の
    /// 後着側は `is_new=false` になり、既読済み動画を未読に戻さない。
    pub fn feed_subscribe(&self, a: &SubscribeArgs<'_>) -> Result<IngestOutcome, DbError> {
        let mut conn = self.lock()?;
        let tx = conn.transaction()?;
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM channels WHERE channel_id = ?1)",
            [a.channel_id],
            |r| r.get(0),
        )?;
        tx.execute(
            "INSERT INTO channels (channel_id, title, thumbnail_url)
             VALUES (?1, ?2, ?3)
             ON CONFLICT (channel_id) DO UPDATE SET
               title = excluded.title,
               thumbnail_url = COALESCE(excluded.thumbnail_url, channels.thumbnail_url)",
            rusqlite::params![a.channel_id, a.title, a.thumbnail_url],
        )?;
        if a.category_id.is_some() {
            tx.execute(
                "UPDATE channels SET category_id = ?2 WHERE channel_id = ?1",
                rusqlite::params![a.channel_id, a.category_id],
            )?;
        }
        let out = ingest_rows(
            &tx,
            a.channel_id,
            a.entries,
            !exists,
            a.etag,
            a.last_modified,
        )?;
        tx.commit()?;
        Ok(out)
    }

    /// 購読解除。チャンネル行を消し、そのチャンネルの未読フィードを既読にする
    /// （購読管理の対象外になった動画が未読一覧に残らないようにする暫定仕様）。
    pub fn channel_delete(&self, channel_id: &str) -> Result<(), DbError> {
        let conn = self.lock()?;
        conn.execute("DELETE FROM channels WHERE channel_id = ?1", [channel_id])?;
        conn.execute(
            "UPDATE videos SET is_read = 1 WHERE channel_id = ?1 AND is_read = 0",
            [channel_id],
        )?;
        Ok(())
    }

    pub fn channel_set_category(
        &self,
        channel_id: &str,
        category_id: Option<i64>,
    ) -> Result<(), DbError> {
        let conn = self.lock()?;
        conn.execute(
            "UPDATE channels SET category_id = ?2 WHERE channel_id = ?1",
            rusqlite::params![channel_id, category_id],
        )?;
        Ok(())
    }

    /// `list_feed`（設計書 §3.1）。ブロックチャンネルの動画は常に除外する（FR-5）。
    pub fn feed_list(&self, filter: &FeedFilter) -> Result<Vec<FeedItem>, DbError> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT v.video_id, v.channel_id, v.channel_title, v.title,
                    v.thumbnail_url, v.published_at, v.kind, v.is_read
             FROM videos v
             JOIN channels c ON c.channel_id = v.channel_id
             WHERE v.channel_id NOT IN (SELECT channel_id FROM blocked_channels)
               AND (?1 = 0 OR v.is_read = 0)
               AND (?2 IS NULL
                    OR (?2 = 0 AND c.category_id IS NULL)
                    OR c.category_id = ?2)
               AND (?3 IS NULL
                    OR datetime(v.published_at) >= datetime('now', '-' || ?3 || ' days'))
             ORDER BY v.published_at DESC
             LIMIT 500",
        )?;
        let mut rows = stmt.query(rusqlite::params![
            filter.unread_only as i64,
            filter.category_id,
            filter.days.map(|d| d as i64),
        ])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(FeedItem {
                video_id: row.get(0)?,
                channel_id: row.get(1)?,
                channel_title: row.get(2)?,
                title: row.get(3)?,
                thumbnail_url: row.get(4)?,
                published_at: row.get(5)?,
                kind: row.get(6)?,
                is_read: row.get::<_, i64>(7)? != 0,
            });
        }
        Ok(out)
    }

    /// 個別既読（設計書 §3.1 の `mark_read`）。
    pub fn videos_mark_read(&self, video_ids: &[String]) -> Result<(), DbError> {
        if video_ids.is_empty() {
            return Ok(());
        }
        let conn = self.lock()?;
        let mut stmt = conn.prepare("UPDATE videos SET is_read = 1 WHERE video_id = ?1")?;
        for id in video_ids {
            stmt.execute([id])?;
        }
        Ok(())
    }

    /// 一括既読。
    pub fn videos_mark_all_read(&self) -> Result<u64, DbError> {
        let conn = self.lock()?;
        let n = conn.execute("UPDATE videos SET is_read = 1 WHERE is_read = 0", [])?;
        Ok(n as u64)
    }

    pub fn category_create(&self, name: &str) -> Result<Category, DbError> {
        let conn = self.lock()?;
        conn.execute("INSERT INTO categories (name) VALUES (?1)", [name])?;
        let id = conn.last_insert_rowid();
        Ok(Category {
            id,
            name: name.to_string(),
            sort_order: 0,
        })
    }

    pub fn category_list(&self) -> Result<Vec<Category>, DbError> {
        let conn = self.lock()?;
        let mut stmt =
            conn.prepare("SELECT id, name, sort_order FROM categories ORDER BY sort_order, id")?;
        let mut rows = stmt.query([])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(Category {
                id: row.get(0)?,
                name: row.get(1)?,
                sort_order: row.get(2)?,
            });
        }
        Ok(out)
    }

    /// 動画 1 件分の履歴。レジューム可否の判定に使う。
    pub fn history_get(&self, video_id: &str) -> Result<Option<WatchHistory>, DbError> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT video_id, title, channel_id, channel_title,
                    position_sec, duration_sec, last_watched_at, completed
             FROM watch_history WHERE video_id = ?1",
        )?;
        let mut rows = stmt.query([video_id])?;
        match rows.next()? {
            Some(row) => Ok(Some(WatchHistory {
                video_id: row.get(0)?,
                title: row.get(1)?,
                channel_id: row.get(2)?,
                channel_title: row.get(3)?,
                position_sec: row.get(4)?,
                duration_sec: row.get(5)?,
                last_watched_at: row.get(6)?,
                completed: row.get::<_, i64>(7)? != 0,
            })),
            None => Ok(None),
        }
    }
}

/// `feed_ingest` / `feed_subscribe` 共通の投入処理。呼び出し側のトランザクション内で
/// 実行する前提で、チャンネルの存在確認はここでは行わない。
/// `reset_unread` が真のとき、既存行（解除済み購読の残骸など）の既読も未読に戻し、
/// 実際に戻した件数を `unread_changed` で返す。
fn ingest_rows(
    tx: &rusqlite::Transaction,
    channel_id: &str,
    entries: &[NewVideo<'_>],
    reset_unread: bool,
    etag: Option<&str>,
    last_modified: Option<&str>,
) -> Result<IngestOutcome, DbError> {
    let mut out = IngestOutcome::default();
    for v in entries {
        out.inserted += tx.execute(
            "INSERT OR IGNORE INTO videos
               (video_id, channel_id, channel_title, title, thumbnail_url,
                published_at, kind, is_read)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0)",
            rusqlite::params![
                v.video_id,
                v.channel_id,
                v.channel_title,
                v.title,
                v.thumbnail_url,
                v.published_at,
                v.kind
            ],
        )?;
    }
    if reset_unread {
        for v in entries {
            out.unread_changed += tx.execute(
                "UPDATE videos SET is_read = 0 WHERE video_id = ?1 AND is_read != 0",
                [v.video_id],
            )?;
        }
    }
    tx.execute(
        "UPDATE channels SET
           last_polled_at = datetime('now'),
           rss_etag = ?2,
           rss_last_modified = ?3
         WHERE channel_id = ?1",
        rusqlite::params![channel_id, etag, last_modified],
    )?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrate_applies_all_and_is_idempotent() {
        let db = Db::connect_in_memory().unwrap();
        let v = db.schema_version().unwrap();
        assert_eq!(v, migrations::MIGRATIONS.last().unwrap().version);
        // 2 回目の migrate は何も適用しない
        db.migrate().unwrap();
        assert_eq!(db.schema_version().unwrap(), v);
    }

    #[test]
    fn settings_roundtrip() {
        let db = Db::connect_in_memory().unwrap();
        assert_eq!(db.setting_get("missing").unwrap(), None);
        db.setting_set("k", "v1").unwrap();
        assert_eq!(db.setting_get("k").unwrap().as_deref(), Some("v1"));
        db.setting_set("k", "v2").unwrap();
        assert_eq!(db.setting_get("k").unwrap().as_deref(), Some("v2"));
    }

    #[test]
    fn reopen_preserves_data() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");
        {
            let db = Db::connect(&path).unwrap();
            db.setting_set("persist", "yes").unwrap();
        }
        let db = Db::connect(&path).unwrap();
        assert_eq!(db.setting_get("persist").unwrap().as_deref(), Some("yes"));
    }

    #[test]
    fn history_upsert_then_progress_roundtrip() {
        let db = Db::connect_in_memory().unwrap();
        db.history_upsert("abc123def45").unwrap();
        let h = db.history_get("abc123def45").unwrap().unwrap();
        assert_eq!(h.position_sec, 0);
        assert!(!h.completed);

        db.history_update_progress("abc123def45", "テスト動画", 120.7, Some(300), false)
            .unwrap();
        let h = db.history_get("abc123def45").unwrap().unwrap();
        assert_eq!(h.title, "テスト動画");
        assert_eq!(h.position_sec, 120);
        assert_eq!(h.duration_sec, Some(300));
        assert!(!h.completed);
    }

    #[test]
    fn history_progress_keeps_title_on_empty() {
        let db = Db::connect_in_memory().unwrap();
        db.history_upsert("abc123def45").unwrap();
        db.history_update_progress("abc123def45", "タイトル", 10.0, Some(60), false)
            .unwrap();
        // media-title 未取得の早い保存でタイトルを消さない
        db.history_update_progress("abc123def45", "", 20.0, Some(60), false)
            .unwrap();
        assert_eq!(
            db.history_get("abc123def45").unwrap().unwrap().title,
            "タイトル"
        );
    }

    #[test]
    fn history_completed_resets_position() {
        let db = Db::connect_in_memory().unwrap();
        db.history_upsert("abc123def45").unwrap();
        db.history_update_progress("abc123def45", "t", 290.0, Some(300), false)
            .unwrap();
        db.history_update_progress("abc123def45", "t", 300.0, Some(300), true)
            .unwrap();
        let h = db.history_get("abc123def45").unwrap().unwrap();
        assert!(h.completed);
        assert_eq!(h.position_sec, 0);
    }

    #[test]
    fn history_update_without_upsert_creates_row() {
        let db = Db::connect_in_memory().unwrap();
        // upsert を経由しない直接保存でも行が作られる（upsert 文の両経路を確認）
        db.history_update_progress("new12345678", "v", 5.0, None, false)
            .unwrap();
        assert!(db.history_get("new12345678").unwrap().is_some());
    }

    /// v4 マイグレーション: UC プレフィックスなしで保存された channel_id の修復。
    /// v3 スキーマまで適用した DB に UC 無しのデータを埋めてから v4 を適用する。
    #[test]
    fn migrate_v4_normalizes_uc_less_channel_ids() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE schema_migrations (
               version INTEGER PRIMARY KEY,
               applied_at TEXT NOT NULL DEFAULT (datetime('now'))
             );",
        )
        .unwrap();
        for m in migrations::MIGRATIONS.iter().filter(|m| m.version <= 3) {
            conn.execute_batch(m.sql).unwrap();
            conn.execute(
                "INSERT INTO schema_migrations (version) VALUES (?1)",
                [m.version],
            )
            .unwrap();
        }
        // UC 無しの購読・ブロック・動画・履歴と、UC 付きの重複チャンネルを仕込む。
        // 旧行はカテゴリ・古い購読日時を持つ（衝突統合で引き継がれるべき値）
        conn.execute("INSERT INTO categories (id, name) VALUES (7, 'tech')", [])
            .unwrap();
        conn.execute(
            "INSERT INTO channels (channel_id, title, category_id, subscribed_at)
             VALUES ('XuqSBlHAE6Xw-yeJA0Tunw', 'LTT', 7, '2025-01-01 00:00:00')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO channels (channel_id, title, subscribed_at)
             VALUES ('UCXuqSBlHAE6Xw-yeJA0Tunw', 'LTT-uc', '2026-10-01 00:00:00')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO channels (channel_id, title) VALUES ('shortOnlyChannel000001', 'OnlyOld')",
            [],
        )
        .unwrap();
        // 先頭が UC で始まる 22 文字の旧形式 id（NOT LIKE 判定では取りこぼされる）
        conn.execute(
            "INSERT INTO videos (video_id, channel_id, title)
             VALUES ('v1', 'XuqSBlHAE6Xw-yeJA0Tunw', 'V1'),
                    ('v2', 'UCXuqSBlHAE6Xw-yeJA0Tunw', 'V2'),
                    ('v3', 'UCzzzzzzzzzzzzzzzzzzzz', 'V3')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO blocked_channels (channel_id, title) VALUES ('badChannelX000000000001', 'B')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO blocked_channels (channel_id, title) VALUES ('ucBlockedChannel000000', 'B2')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO watch_history (video_id, title, channel_id)
             VALUES ('v1', 'V1', 'XuqSBlHAE6Xw-yeJA0Tunw')",
            [],
        )
        .unwrap();
        let db = Db {
            conn: Arc::new(Mutex::new(conn)),
        };
        db.migrate().unwrap();

        // UC 付きが既にあるチャンネルは UC 無し行が消え、
        // カテゴリと最古の購読日時は UC 付き行へ引き継がれる
        assert!(db.channel_get("XuqSBlHAE6Xw-yeJA0Tunw").unwrap().is_none());
        let merged = db.channel_get("UCXuqSBlHAE6Xw-yeJA0Tunw").unwrap().unwrap();
        assert_eq!(merged.category_id, Some(7));
        assert_eq!(merged.subscribed_at, "2025-01-01 00:00:00");
        // UC 付きが無いチャンネルはリネームされる
        assert!(db
            .channel_get("UCshortOnlyChannel000001")
            .unwrap()
            .is_some());
        // UC 始まりの 22 文字旧 id も正規化される
        assert!(db
            .channel_get("UCUCzzzzzzzzzzzzzzzzzzzz")
            .unwrap()
            .is_none()); // v3 は channels 行を持たないので videos 側だけ
        let conn = db.lock().unwrap();
        let v3_channel: String = conn
            .query_row("SELECT channel_id FROM videos WHERE video_id = 'v3'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(v3_channel, "UCUCzzzzzzzzzzzzzzzzzzzz");
        // videos / watch_history / blocked_channels も UC 付きに揃う
        let stale: i64 = conn
            .query_row(
                "SELECT (SELECT COUNT(*) FROM channels WHERE length(channel_id) = 22)
                      + (SELECT COUNT(*) FROM videos WHERE length(channel_id) = 22)
                      + (SELECT COUNT(*) FROM blocked_channels WHERE length(channel_id) = 22)
                      + (SELECT COUNT(*) FROM watch_history WHERE length(channel_id) = 22)",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(stale, 0);
        drop(conn);
        // v4 自体も冪等（再適用は schema_migrations で抑止される）
        db.migrate().unwrap();
    }

    /// 購読解除後も残る videos 行はフィード一覧に出さない（JOIN で除外）。
    /// 未読・すべて両モードで確認する。
    #[test]
    fn feed_list_hides_unsubscribed_channel_videos() {
        let db = Db::connect_in_memory().unwrap();
        let entries = [NewVideo {
            video_id: "v1",
            channel_id: "UCchan000000000000001",
            channel_title: "C",
            title: "V1",
            thumbnail_url: None,
            published_at: Some("2026-10-01 00:00:00"),
            kind: "video",
        }];
        db.feed_subscribe(&SubscribeArgs {
            channel_id: "UCchan000000000000001",
            title: "C",
            thumbnail_url: None,
            category_id: None,
            entries: &entries,
            etag: None,
            last_modified: None,
        })
        .unwrap();
        let all = FeedFilter {
            unread_only: false,
            category_id: None,
            days: None,
        };
        assert_eq!(db.feed_list(&all).unwrap().len(), 1);
        db.channel_delete("UCchan000000000000001").unwrap();
        assert!(db.feed_list(&all).unwrap().is_empty());
        let unread = FeedFilter {
            unread_only: true,
            category_id: None,
            days: None,
        };
        assert!(db.feed_list(&unread).unwrap().is_empty());
    }
}
