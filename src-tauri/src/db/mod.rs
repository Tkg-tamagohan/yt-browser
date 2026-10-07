//! SQLite 接続とバージョン管理されたマイグレーション（技術方針 K）。
//!
//! rusqlite は同期 API のため、接続は `Mutex<Connection>` 1 本に集約する（設計書 §1.2）。
//! 書き込みは短いトランザクションに収め、接続プールは持たない。

mod migrations;

use std::path::Path;
use std::sync::Mutex;

use rusqlite::Connection;
use thiserror::Error;

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

/// アプリ全体で共有する DB 接続。
pub struct Db {
    conn: Mutex<Connection>,
}

impl Db {
    /// `path` のデータベースを開き、未適用マイグレーションをすべて適用する。
    pub fn connect(path: &Path) -> Result<Self, DbError> {
        let conn = Connection::open(path)?;
        let db = Self {
            conn: Mutex::new(conn),
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
            conn: Mutex::new(conn),
        };
        db.init_pragmas()?;
        db.migrate()?;
        Ok(db)
    }

    fn init_pragmas(&self) -> Result<(), DbError> {
        let conn = self.conn.lock().map_err(|_| DbError::Poisoned)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        Ok(())
    }

    fn migrate(&self) -> Result<(), DbError> {
        let mut conn = self.conn.lock().map_err(|_| DbError::Poisoned)?;
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
        let conn = self.conn.lock().map_err(|_| DbError::Poisoned)?;
        let v = conn.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
            [],
            |row| row.get(0),
        )?;
        Ok(v)
    }

    pub fn setting_get(&self, key: &str) -> Result<Option<String>, DbError> {
        let conn = self.conn.lock().map_err(|_| DbError::Poisoned)?;
        let mut stmt = conn.prepare("SELECT value FROM settings WHERE key = ?1")?;
        let mut rows = stmt.query([key])?;
        match rows.next()? {
            Some(row) => Ok(Some(row.get(0)?)),
            None => Ok(None),
        }
    }

    pub fn setting_set(&self, key: &str, value: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().map_err(|_| DbError::Poisoned)?;
        conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            rusqlite::params![key, value],
        )?;
        Ok(())
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
}
