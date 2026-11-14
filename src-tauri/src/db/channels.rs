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

    /// カテゴリ作成。一覧の末尾に置くため `sort_order` は既存の最大値 + 1
    /// （FR-63、仕様決定 CN。`playlist_create` と同じ方針）。
    /// 並べ替え前の全行 0 のデータでも `ORDER BY sort_order, id` の
    /// 作成順を崩さない。
    pub fn category_create(&self, name: &str) -> Result<Category, DbError> {
        let conn = self.lock()?;
        let sort_order: i64 = conn.query_row(
            "INSERT INTO categories (name, sort_order)
             VALUES (?1, (SELECT COALESCE(MAX(sort_order), -1) + 1 FROM categories))
             RETURNING sort_order",
            [name],
            |r| r.get(0),
        )?;
        Ok(Category {
            id: conn.last_insert_rowid(),
            name: name.to_string(),
            sort_order,
        })
    }

    /// カテゴリ一覧の並べ替え（FR-63、仕様決定 CN）。`category_ids` が
    /// 現在のカテゴリ全件と同一集合（順不同・重複なし）であることを
    /// このトランザクション内で検証し、一致しない場合は
    /// `DbError::MismatchedItems` で変更しない。一致すれば `sort_order` を
    /// 渡した順に 0..n-1 へ振り直す（`playlist_reorder_lists` と同じ方針）。
    pub fn category_reorder(&self, category_ids: &[i64]) -> Result<(), DbError> {
        let mut conn = self.lock()?;
        let tx = conn.transaction()?;
        let mut current: Vec<i64> = Vec::new();
        {
            let mut stmt = tx.prepare("SELECT id FROM categories")?;
            let mut rows = stmt.query([])?;
            while let Some(row) = rows.next()? {
                current.push(row.get(0)?);
            }
        }
        let incoming: std::collections::HashSet<i64> = category_ids.iter().copied().collect();
        let same = category_ids.len() == current.len()
            && incoming.len() == category_ids.len()
            && current.iter().all(|id| incoming.contains(id));
        if !same {
            return Err(DbError::MismatchedItems);
        }
        for (pos, id) in category_ids.iter().enumerate() {
            tx.execute(
                "UPDATE categories SET sort_order = ?2 WHERE id = ?1",
                rusqlite::params![id, pos as i64],
            )?;
        }
        tx.commit()?;
        Ok(())
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
