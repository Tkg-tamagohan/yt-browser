//! channels / blocked_channels / categories テーブルとポーラーの `PollTarget` 型。

use super::*;
use crate::model::{BlockedChannel, Category, Channel};

/// ポーラーが逐次処理するチャンネルの条件付き取得メタ（設計書 §8 channels 表のサブセット）。
#[derive(Debug, Clone)]
pub struct PollTarget {
    pub channel_id: String,
    pub title: String,
    pub rss_etag: Option<String>,
    pub rss_last_modified: Option<String>,
}

impl Db {
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

    /// ブロック登録（FR-5）。既に登録済みならタイトルを更新する。
    pub fn blocked_add(&self, channel_id: &str, title: &str) -> Result<(), DbError> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO blocked_channels (channel_id, title) VALUES (?1, ?2)
             ON CONFLICT(channel_id) DO UPDATE SET title = excluded.title",
            rusqlite::params![channel_id, title],
        )?;
        Ok(())
    }

    /// ブロック解除。
    pub fn blocked_remove(&self, channel_id: &str) -> Result<(), DbError> {
        let conn = self.lock()?;
        conn.execute(
            "DELETE FROM blocked_channels WHERE channel_id = ?1",
            [channel_id],
        )?;
        Ok(())
    }

    /// ブロック中チャンネルの一覧（設定画面の解除 UI 用）。
    pub fn blocked_list(&self) -> Result<Vec<BlockedChannel>, DbError> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT channel_id, title, created_at FROM blocked_channels
             ORDER BY created_at DESC",
        )?;
        let mut rows = stmt.query([])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(BlockedChannel {
                channel_id: row.get(0)?,
                title: row.get(1)?,
                created_at: row.get(2)?,
            });
        }
        Ok(out)
    }

    /// ブロック中チャンネル ID の集合。検索・関連動画の結果絞り込みに使う。
    pub fn blocked_ids(&self) -> Result<std::collections::HashSet<String>, DbError> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare("SELECT channel_id FROM blocked_channels")?;
        let mut rows = stmt.query([])?;
        let mut out = std::collections::HashSet::new();
        while let Some(row) = rows.next()? {
            out.insert(row.get::<_, String>(0)?);
        }
        Ok(out)
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
}
