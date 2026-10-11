//! 一時キューの永続化（FR-30、仕様決定 AY）。
//!
//! `queue_items`（項目列＋表示メタ）と `queue_state`（現在位置）への
//! write-through を担う。永続化の対象は一時キューのみで、
//! プレイリストキューはセッション限りのままとする。
//! `video_meta_lookup` は拡張からのキュー追加（FR-29、仕様決定 AX）で
//! 動画 ID だけ判明する項目の表示メタを `videos` 台帳 → `watch_history`
//! の順に補完する（yt-dlp による遅延解決は持たない）。

use super::*;
use crate::model::{QueueItem, QueueSnapshot, VideoRef};

impl Db {
    /// 一時キューの全置換（write-through）。項目列と現在位置を
    /// 1 トランザクションで置き換える。`current_index` は項目数へ収める
    /// （空なら 0、末尾超過は末尾へ）。
    pub fn queue_save(&self, items: &[QueueItem], current_index: i64) -> Result<(), DbError> {
        let index = if items.is_empty() {
            0
        } else {
            current_index.clamp(0, items.len() as i64 - 1)
        };
        let mut conn = self.lock()?;
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM queue_items", [])?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO queue_items
                   (position, video_id, title, channel_title, thumbnail_url)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )?;
            for (i, item) in items.iter().enumerate() {
                stmt.execute(rusqlite::params![
                    i as i64,
                    item.video_id,
                    item.title,
                    item.channel_title,
                    item.thumbnail_url,
                ])?;
            }
        }
        tx.execute(
            "INSERT INTO queue_state (id, current_index) VALUES (1, ?1)
             ON CONFLICT (id) DO UPDATE SET current_index = excluded.current_index",
            [index],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// 保存済みの一時キュー。未保存（初回起動など）は空のスナップショット。
    pub fn queue_load(&self) -> Result<QueueSnapshot, DbError> {
        let conn = self.lock()?;
        let current_index: i64 = conn
            .query_row(
                "SELECT current_index FROM queue_state WHERE id = 1",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);
        let mut stmt = conn.prepare(
            "SELECT video_id, title, channel_title, thumbnail_url
             FROM queue_items ORDER BY position",
        )?;
        let mut rows = stmt.query([])?;
        let mut items = Vec::new();
        while let Some(row) = rows.next()? {
            items.push(QueueItem {
                video_id: row.get(0)?,
                title: row.get(1)?,
                channel_title: row.get(2)?,
                thumbnail_url: row.get(3)?,
            });
        }
        // 保存位置が項目数を超えていてはいけない（手編集等への防御）
        let current_index = if items.is_empty() {
            0
        } else {
            current_index.clamp(0, items.len() as i64 - 1)
        };
        Ok(QueueSnapshot {
            items,
            current_index,
        })
    }

    /// `videos` 台帳 → `watch_history` の順で表示メタを補完する
    /// （FR-29、仕様決定 AX）。どちらにも無ければ `None` を返し、
    /// 呼び出し側が動画 ID 表示へ倒す。
    pub fn video_meta_lookup(&self, video_id: &str) -> Result<Option<VideoRef>, DbError> {
        let conn = self.lock()?;
        let from_videos = conn
            .query_row(
                "SELECT title, NULLIF(channel_id, ''), channel_title, thumbnail_url
                 FROM videos WHERE video_id = ?1",
                [video_id],
                |r| {
                    Ok(VideoRef {
                        video_id: video_id.to_string(),
                        title: r.get(0)?,
                        channel_id: r.get(1)?,
                        channel_title: r.get(2)?,
                        thumbnail_url: r.get(3)?,
                    })
                },
            )
            .ok();
        if let Some(v) = from_videos {
            return Ok(Some(v));
        }
        let from_history = conn
            .query_row(
                "SELECT title, channel_id, channel_title
                 FROM watch_history WHERE video_id = ?1",
                [video_id],
                |r| {
                    Ok(VideoRef {
                        video_id: video_id.to_string(),
                        title: r.get(0)?,
                        channel_id: r.get(1)?,
                        channel_title: r.get(2)?,
                        thumbnail_url: None,
                    })
                },
            )
            .ok();
        Ok(from_history)
    }
}
