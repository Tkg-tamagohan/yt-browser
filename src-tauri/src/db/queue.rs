//! キューの永続化（FR-30・FR-48、仕様決定 AY・BY）。
//!
//! 一時キューは `queue_items`（項目列＋表示メタ）と `queue_state`
//! （playlist_id NULL の行の現在位置）への write-through で保存する。
//! プレイリストキューは `queue_state` の playlist_id 行に現在位置だけを
//! 持ち、項目列は `playlist_items` を正として複製しない（FR-48、
//! 仕様決定 BY で「プレイリストキューは DB に保存しない」を改訂）。
//! `video_meta_lookup` は拡張からのキュー追加（FR-29、仕様決定 AX）で
//! 動画 ID だけ判明する項目の表示メタを `videos` 台帳 → `watch_history`
//! の順に補完する（yt-dlp による遅延解決は持たない）。

use super::*;
use crate::model::{PlaylistQueueState, QueueItem, QueueSnapshot, VideoRef};
use rusqlite::OptionalExtension;

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
                   (position, uid, video_id, title, channel_title, thumbnail_url)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?;
            for (i, item) in items.iter().enumerate() {
                stmt.execute(rusqlite::params![
                    i as i64,
                    item.uid,
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
        // 未保存（行無し）は 0。行以外のエラーは丸めずに返す
        let current_index: i64 = conn
            .query_row(
                "SELECT current_index FROM queue_state WHERE id = 1",
                [],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or(0);
        let mut stmt = conn.prepare(
            "SELECT uid, video_id, title, channel_title, thumbnail_url
             FROM queue_items ORDER BY position",
        )?;
        let mut rows = stmt.query([])?;
        let mut items = Vec::new();
        while let Some(row) = rows.next()? {
            items.push(QueueItem {
                uid: row.get(0)?,
                video_id: row.get(1)?,
                title: row.get(2)?,
                channel_title: row.get(3)?,
                thumbnail_url: row.get(4)?,
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

    /// プレイリストキューの保存位置を書き換える（FR-48、仕様決定 BY）。
    /// `playlist_id` ごとに高々 1 行（v16 の部分一意索引）で、
    /// 現在位置だけを持つ（項目列は `playlist_items` を正として保存しない）。
    /// キューが存在する間の各ミューテーションから write-through で呼ぶ。
    /// 削除済みプレイリストへの保存は FK 違反として失敗する
    /// （フロントはプレイリスト削除時にキューを畳むため通常到達しない）
    pub fn queue_playlist_save(&self, playlist_id: i64, current_index: i64) -> Result<(), DbError> {
        let mut conn = self.lock()?;
        let tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM queue_state WHERE playlist_id = ?1",
            [playlist_id],
        )?;
        tx.execute(
            "INSERT INTO queue_state (playlist_id, current_index) VALUES (?1, ?2)",
            rusqlite::params![playlist_id, current_index.max(0)],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// プレイリストキューの保存行を消す（キュー消滅時）。未保存なら何もしない。
    /// プレイリスト自体の削除では FK の ON DELETE CASCADE で行が消えるため、
    /// その後の呼び出しは空操作になる
    pub fn queue_playlist_clear(&self, playlist_id: i64) -> Result<(), DbError> {
        let conn = self.lock()?;
        conn.execute(
            "DELETE FROM queue_state WHERE playlist_id = ?1",
            [playlist_id],
        )?;
        Ok(())
    }

    /// 永続化されているプレイリストキュー一覧（FR-48）。プレイリスト名を
    /// JOIN して返す。削除済みプレイリストの行は CASCADE で消える前提だが、
    /// 外部キー無効で残った場合も INNER JOIN でこぼさない
    pub fn queue_playlist_load(&self) -> Result<Vec<PlaylistQueueState>, DbError> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT s.playlist_id, p.name, s.current_index
             FROM queue_state s JOIN playlists p ON p.id = s.playlist_id
             WHERE s.playlist_id IS NOT NULL
             ORDER BY s.playlist_id",
        )?;
        let mut rows = stmt.query([])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(PlaylistQueueState {
                playlist_id: row.get(0)?,
                playlist_name: row.get(1)?,
                current_index: row.get(2)?,
            });
        }
        Ok(out)
    }

    /// `videos` 台帳 → `watch_history` の順で表示メタを補完する
    /// （FR-29、仕様決定 AX）。どちらにも無ければ `None` を返し、
    /// 呼び出し側が動画 ID 表示へ倒す。
    pub fn video_meta_lookup(&self, video_id: &str) -> Result<Option<VideoRef>, DbError> {
        let conn = self.lock()?;
        // 行無しは None。行以外のエラーは丸めずに返す
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
            .optional()?;
        if let Some(v) = from_videos {
            return Ok(Some(v));
        }
        let from_history = conn
            .query_row(
                "SELECT title, NULLIF(channel_id, ''), channel_title
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
            .optional()?;
        Ok(from_history)
    }
}
