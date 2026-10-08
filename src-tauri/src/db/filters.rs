//! filters テーブル（NG フィルタ）。

use super::*;

impl Db {
    /// NG フィルタ登録（FR-7）。対象・種別・パターンの妥当性は呼び出し側で検証済み。
    pub fn filter_add(
        &self,
        target: &str,
        kind: &str,
        pattern: &str,
    ) -> Result<crate::model::Filter, DbError> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO filters (target, kind, pattern) VALUES (?1, ?2, ?3)",
            rusqlite::params![target, kind, pattern],
        )?;
        let id = conn.last_insert_rowid();
        let created_at: String =
            conn.query_row("SELECT created_at FROM filters WHERE id = ?1", [id], |r| {
                r.get(0)
            })?;
        Ok(crate::model::Filter {
            id,
            target: target.to_string(),
            kind: kind.to_string(),
            pattern: pattern.to_string(),
            enabled: true,
            created_at,
        })
    }

    /// NG フィルタ削除。行が存在しなければ false。
    pub fn filter_remove(&self, id: i64) -> Result<bool, DbError> {
        let conn = self.lock()?;
        let n = conn.execute("DELETE FROM filters WHERE id = ?1", [id])?;
        Ok(n > 0)
    }

    /// NG フィルタ一覧（設定画面と Matcher の rebuild 用）。
    pub fn filter_list(&self) -> Result<Vec<crate::model::Filter>, DbError> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT id, target, kind, pattern, enabled, created_at
             FROM filters ORDER BY id",
        )?;
        let mut rows = stmt.query([])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(crate::model::Filter {
                id: row.get(0)?,
                target: row.get(1)?,
                kind: row.get(2)?,
                pattern: row.get(3)?,
                enabled: row.get::<_, i64>(4)? != 0,
                created_at: row.get(5)?,
            });
        }
        Ok(out)
    }
}
