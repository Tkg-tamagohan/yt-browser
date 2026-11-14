//! ライブ配信の待機リスト永続化（FR-44、仕様決定 BT）。
//!
//! `live_waits` への write-through を担う。表示メタは項目行へ持たせて
//! `videos` 台帳を汚染しない（queue_items と同じ方針）。
//! `scheduled_start_ms` は InnerTube の放送予定時刻（epoch ms）で、
//! ポーラーが予定変更を検知したとき `live_wait_set_scheduled` で追随する。

use super::*;
use crate::model::LiveWait;
use rusqlite::OptionalExtension;

impl Db {
    /// 待機登録。同一動画の重複は主キーで抑止し、実際に挿入したかを返す。
    pub fn live_wait_insert(&self, entry: &LiveWait) -> Result<bool, DbError> {
        let conn = self.lock()?;
        let n = conn.execute(
            "INSERT OR IGNORE INTO live_waits
               (video_id, title, channel_id, channel_title, thumbnail_url,
                scheduled_start_ms, registered_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                entry.video_id,
                entry.title,
                entry.channel_id,
                entry.channel_title,
                entry.thumbnail_url,
                entry.scheduled_start_ms,
                entry.registered_ms,
            ],
        )?;
        Ok(n > 0)
    }

    /// 登録済みの待機一覧。予定時刻の昇順（未定は末尾）→ 登録順で返す。
    pub fn live_wait_list(&self) -> Result<Vec<LiveWait>, DbError> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT video_id, title, channel_id, channel_title, thumbnail_url,
                    scheduled_start_ms, registered_ms
             FROM live_waits
             ORDER BY scheduled_start_ms IS NULL, scheduled_start_ms,
                      registered_ms, video_id",
        )?;
        let mut rows = stmt.query([])?;
        let mut items = Vec::new();
        while let Some(row) = rows.next()? {
            items.push(read_wait(row)?);
        }
        Ok(items)
    }

    /// 1 件の取得。未登録は None。
    pub fn live_wait_get(&self, video_id: &str) -> Result<Option<LiveWait>, DbError> {
        let conn = self.lock()?;
        conn.query_row(
            "SELECT video_id, title, channel_id, channel_title, thumbnail_url,
                    scheduled_start_ms, registered_ms
             FROM live_waits WHERE video_id = ?1",
            [video_id],
            read_wait,
        )
        .optional()
        .map_err(DbError::from)
    }

    /// 個別解除。戻り値は削除したか（未登録なら false）。
    pub fn live_wait_remove(&self, video_id: &str) -> Result<bool, DbError> {
        let conn = self.lock()?;
        let n = conn.execute(
            "DELETE FROM live_waits WHERE video_id = ?1",
            [video_id],
        )?;
        Ok(n > 0)
    }

    /// 予定時刻の更新。ポーラーが InnerTube 側の予定変更を検知したときに
    /// 追随する（値が同じときは書き込まない）。
    pub fn live_wait_set_scheduled(
        &self,
        video_id: &str,
        scheduled_ms: Option<i64>,
    ) -> Result<(), DbError> {
        let conn = self.lock()?;
        conn.execute(
            "UPDATE live_waits SET scheduled_start_ms = ?2
             WHERE video_id = ?1
               AND scheduled_start_ms IS NOT ?2",
            rusqlite::params![video_id, scheduled_ms],
        )?;
        Ok(())
    }
}

fn read_wait(row: &rusqlite::Row<'_>) -> Result<LiveWait, rusqlite::Error> {
    Ok(LiveWait {
        video_id: row.get(0)?,
        title: row.get(1)?,
        channel_id: row.get(2)?,
        channel_title: row.get(3)?,
        thumbnail_url: row.get(4)?,
        scheduled_start_ms: row.get(5)?,
        registered_ms: row.get(6)?,
    })
}
