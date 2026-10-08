//! settings テーブルの読み書き。

use super::*;

impl Db {
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
}
