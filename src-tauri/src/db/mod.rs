//! SQLite 接続とバージョン管理されたマイグレーション（技術方針 K）。
//!
//! rusqlite は同期 API のため、接続は `Mutex<Connection>` 1 本に集約する（設計書 §1.2）。
//! 書き込みは短いトランザクションに収め、接続プールは持たない。

mod channels;
mod feed;
mod filters;
mod history;
mod library;
mod migrations;
mod settings;

#[cfg(test)]
mod tests;

use std::collections::HashSet;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

use rusqlite::Connection;
use thiserror::Error;

pub use channels::PollTarget;
// IngestOutcome は戻り値型としてのみ使われ外部から名前参照されないが、
// 公開面（crate::db::IngestOutcome）を維持するため re-export する。
#[allow(unused_imports)]
pub use feed::{IngestOutcome, NewVideo, SubscribeArgs, FEED_LIST_LIMIT};

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
    #[error("対象が存在しない")]
    NotFound,
    #[error("並べ替え対象の項目集合が現在の内容と一致しない")]
    MismatchedItems,
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// アプリ全体で共有する DB 接続。`Clone` は同一接続を共有する
/// （`Mutex<Connection>` は設計書 §1.2 の通り 1 本のみ）。
#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
    /// 履歴を手動削除された動画の保存抑止（セッション内のみ有効）。
    /// `history_remove` で登録し、明示的な再生開始（`history_upsert`）で解除する。
    /// 再生中プレイヤーの定期・終了保存が削除済み履歴を復活させないためのもの。
    history_suppressed: Arc<Mutex<HashSet<String>>>,
}

impl Db {
    /// `path` のデータベースを開き、未適用マイグレーションをすべて適用する。
    pub fn connect(path: &Path) -> Result<Self, DbError> {
        let conn = Connection::open(path)?;
        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
            history_suppressed: Arc::new(Mutex::new(HashSet::new())),
        };
        db.init_pragmas()?;
        db.migrate()?;
        // 起動時の履歴剪定（仕様決定 AV）。稼働中は行わない
        db.history_prune()?;
        Ok(db)
    }

    /// テスト用のインメモリ接続。
    #[cfg(test)]
    fn connect_in_memory() -> Result<Self, DbError> {
        let conn = Connection::open_in_memory()?;
        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
            history_suppressed: Arc::new(Mutex::new(HashSet::new())),
        };
        db.init_pragmas()?;
        db.migrate()?;
        db.history_prune()?;
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
}
