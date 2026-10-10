//! watch_history テーブルと、手動削除による保存抑止。

use super::*;
use crate::model::WatchHistory;

/// `watch_history` の保持上限（仕様決定 AV）。
/// 超過分は起動時剪定で最古から消す。一覧表示の limit とは別枠。
const HISTORY_RETENTION_MAX: i64 = 10_000;

impl Db {
    /// 再生開始時に履歴行を確保する。既存行は位置や完了状態を壊さない。
    /// 明示的な再生開始なので、手動削除による保存抑止をここで解除する。
    /// 抑止の解除と挿入は conn ロック内で行い、`history_remove` や
    /// `history_update_progress` と同じ排他区間に載せて交錯を防ぐ。
    pub fn history_upsert(&self, video_id: &str) -> Result<(), DbError> {
        let conn = self.lock()?;
        crate::util::lock(&self.history_suppressed).remove(video_id);
        conn.execute(
            "INSERT INTO watch_history (video_id, title) VALUES (?1, '')
             ON CONFLICT (video_id) DO NOTHING",
            [video_id],
        )?;
        Ok(())
    }

    /// 再生中・終了時の進捗保存。タイトルは空文字なら既存値を維持する
    /// （media-title 未確定の早い保存で取れたタイトルを消さないため）。
    /// `completed` が true のときは位置を 0 に戻す（次回は先頭から再生）。
    pub fn history_update_progress(
        &self,
        video_id: &str,
        title: &str,
        position_sec: f64,
        duration_sec: Option<i64>,
        completed: bool,
    ) -> Result<(), DbError> {
        let position = if completed {
            0
        } else {
            position_sec.max(0.0) as i64
        };
        // 抑止確認と書き込みは conn ロックの同一排他区間に置く。
        // ここで先に確認してからロックを取ると、間に割り込んだ削除が
        // 抑止登録を済ませてもこの保存が削除済み行を再作成してしまう
        let conn = self.lock()?;
        if crate::util::lock(&self.history_suppressed).contains(video_id) {
            return Ok(());
        }
        conn.execute(
            "INSERT INTO watch_history
               (video_id, title, position_sec, duration_sec, last_watched_at, completed)
             VALUES (?1, ?2, ?3, ?4, datetime('now'), ?5)
             ON CONFLICT (video_id) DO UPDATE SET
               title = CASE WHEN excluded.title <> '' THEN excluded.title
                            ELSE watch_history.title END,
               position_sec = excluded.position_sec,
               duration_sec = COALESCE(excluded.duration_sec, watch_history.duration_sec),
               last_watched_at = excluded.last_watched_at,
               completed = excluded.completed",
            rusqlite::params![video_id, title, position, duration_sec, completed as i64,],
        )?;
        Ok(())
    }

    /// 動画 1 件分の履歴。レジューム可否の判定に使う。
    pub fn history_get(&self, video_id: &str) -> Result<Option<WatchHistory>, DbError> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT video_id, title, channel_id, channel_title,
                    position_sec, duration_sec, last_watched_at, completed
             FROM watch_history WHERE video_id = ?1",
        )?;
        let mut rows = stmt.query([video_id])?;
        match rows.next()? {
            Some(row) => Ok(Some(WatchHistory {
                video_id: row.get(0)?,
                title: row.get(1)?,
                channel_id: row.get(2)?,
                channel_title: row.get(3)?,
                position_sec: row.get(4)?,
                duration_sec: row.get(5)?,
                last_watched_at: row.get(6)?,
                completed: row.get::<_, i64>(7)? != 0,
            })),
            None => Ok(None),
        }
    }

    /// 視聴履歴の一覧（FR-7）。新しく見た順で `limit` 件まで返す。
    /// 同じ `last_watched_at`（秒精度）の行は rowid 降順で決定的にする。
    pub fn history_list(&self, limit: u32) -> Result<Vec<WatchHistory>, DbError> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT video_id, title, channel_id, channel_title,
                    position_sec, duration_sec, last_watched_at, completed
             FROM watch_history ORDER BY last_watched_at DESC, rowid DESC LIMIT ?1",
        )?;
        let mut rows = stmt.query([limit])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(WatchHistory {
                video_id: row.get(0)?,
                title: row.get(1)?,
                channel_id: row.get(2)?,
                channel_title: row.get(3)?,
                position_sec: row.get(4)?,
                duration_sec: row.get(5)?,
                last_watched_at: row.get(6)?,
                completed: row.get::<_, i64>(7)? != 0,
            });
        }
        Ok(out)
    }

    /// 視聴履歴の個別削除（手動削除のみ、仕様決定 I）。
    /// 再生中の同じ動画に対する以後の `history_update_progress` を抑止する
    /// （再生中の削除で直後に履歴が復活しないようにする）。
    /// 抑止はセッション内のみ有効で、明示的な再生開始で解除される。
    /// 削除と抑止登録は conn ロックの同一排他区間で行う
    /// （保存側の「確認→書き込み」と直列化される順序を一致させるため）。
    pub fn history_remove(&self, video_id: &str) -> Result<(), DbError> {
        let conn = self.lock()?;
        conn.execute("DELETE FROM watch_history WHERE video_id = ?1", [video_id])?;
        crate::util::lock(&self.history_suppressed).insert(video_id.to_string());
        Ok(())
    }

    /// 起動時の履歴剪定（仕様決定 AV）。直近 `HISTORY_RETENTION_MAX` 件を残し、
    /// 超過分を最古から削除する。保持対象は `history_list` と同じ順序
    /// （last_watched_at DESC、同時刻は rowid DESC）の上位とする。
    /// 手動削除の保存抑止（history_suppressed）とは独立に動く。
    pub fn history_prune(&self) -> Result<(), DbError> {
        let conn = self.lock()?;
        conn.execute(
            "DELETE FROM watch_history
             WHERE rowid NOT IN (
               SELECT rowid FROM watch_history
               ORDER BY last_watched_at DESC, rowid DESC LIMIT ?1)",
            [HISTORY_RETENTION_MAX],
        )?;
        Ok(())
    }
}
