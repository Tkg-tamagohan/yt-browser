//! 動画内ブックマークの永続化（FR-49、仕様決定 BZ）。
//!
//! `video_bookmarks` への write-through を担う。行の表示用タイトルは
//! テーブルに持たず、一覧・単件の取得時に `videos` 台帳 → `watch_history`
//! の順で解決する（`video_meta_lookup` と同じ規則）。

use super::*;
use crate::model::VideoBookmark;
use rusqlite::OptionalExtension;

/// 一覧・単件取得で使う列と JOIN 解決のタイトル。
/// `videos` 台帳優先、無ければ `watch_history`、タイトルが空の行は
/// 未解決（None）として返す。
const BOOKMARK_SELECT: &str = "SELECT b.id, b.video_id, b.position_sec, b.label,
        b.created_at,
        COALESCE(NULLIF(v.title, ''), NULLIF(h.title, '')) AS title
     FROM video_bookmarks b
     LEFT JOIN videos v ON v.video_id = b.video_id
     LEFT JOIN watch_history h ON h.video_id = b.video_id";

impl Db {
    /// ブックマーク登録。同一動画の同一位置の重複は
    /// UNIQUE (video_id, position_sec) で抑止し、実際に挿入したときだけ
    /// 挿入行を返す（既存行があるときは None。既存行のラベルは維持する）。
    pub fn bookmark_insert(
        &self,
        video_id: &str,
        position_sec: i64,
        label: &str,
    ) -> Result<Option<VideoBookmark>, DbError> {
        let conn = self.lock()?;
        let n = conn.execute(
            "INSERT OR IGNORE INTO video_bookmarks (video_id, position_sec, label)
             VALUES (?1, ?2, ?3)",
            rusqlite::params![video_id, position_sec, label],
        )?;
        if n == 0 {
            return Ok(None);
        }
        let id = conn.last_insert_rowid();
        conn.query_row(
            &format!("{BOOKMARK_SELECT} WHERE b.id = ?1"),
            [id],
            read_bookmark,
        )
        .optional()
        .map_err(DbError::from)
    }

    /// 1 件の取得。未登録は None。
    pub fn bookmark_get(&self, id: i64) -> Result<Option<VideoBookmark>, DbError> {
        let conn = self.lock()?;
        conn.query_row(
            &format!("{BOOKMARK_SELECT} WHERE b.id = ?1"),
            [id],
            read_bookmark,
        )
        .optional()
        .map_err(DbError::from)
    }

    /// ブックマーク一覧。保存の新しい順（`created_at` 降順、同時刻は
    /// `id` 降順。暫定解釈: 直近に保存した地点を先に見せる並びとした）。
    pub fn bookmark_list(&self) -> Result<Vec<VideoBookmark>, DbError> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(&format!(
            "{BOOKMARK_SELECT} ORDER BY b.created_at DESC, b.id DESC"
        ))?;
        let mut rows = stmt.query([])?;
        let mut items = Vec::new();
        while let Some(row) = rows.next()? {
            items.push(read_bookmark(row)?);
        }
        Ok(items)
    }

    /// 個別削除。戻り値は削除したか（未登録なら false）。
    pub fn bookmark_remove(&self, id: i64) -> Result<bool, DbError> {
        let conn = self.lock()?;
        let n = conn.execute("DELETE FROM video_bookmarks WHERE id = ?1", [id])?;
        Ok(n > 0)
    }

    /// ラベルの更新。対象が存在しなければ `DbError::NotFound`。
    /// 空・空白のみのラベルへの更新は呼び出し側が位置表記へ潰してから渡す。
    pub fn bookmark_update_label(&self, id: i64, label: &str) -> Result<(), DbError> {
        let conn = self.lock()?;
        let n = conn.execute(
            "UPDATE video_bookmarks SET label = ?2 WHERE id = ?1",
            rusqlite::params![id, label],
        )?;
        if n == 0 {
            return Err(DbError::NotFound);
        }
        Ok(())
    }
}

fn read_bookmark(row: &rusqlite::Row<'_>) -> Result<VideoBookmark, rusqlite::Error> {
    Ok(VideoBookmark {
        id: row.get(0)?,
        video_id: row.get(1)?,
        position_sec: row.get(2)?,
        label: row.get(3)?,
        created_at: row.get(4)?,
        title: row.get(5)?,
    })
}
