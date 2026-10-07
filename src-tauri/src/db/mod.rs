//! SQLite 接続とバージョン管理されたマイグレーション（技術方針 K）。
//!
//! rusqlite は同期 API のため、接続は `Mutex<Connection>` 1 本に集約する（設計書 §1.2）。
//! 書き込みは短いトランザクションに収め、接続プールは持たない。

mod migrations;

use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

use rusqlite::Connection;
use thiserror::Error;

use crate::model::WatchHistory;

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
}
