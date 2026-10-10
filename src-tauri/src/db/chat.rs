//! chat_logs テーブルへの一括保存と FTS5 検索。

use super::*;

impl Db {
    /// チャットの一括保存（設計書 §6.2）。ポーリング応答 1 回分を 1 トランザクションで。
    pub fn chat_insert_batch(&self, events: &[crate::model::ChatEvent]) -> Result<usize, DbError> {
        if events.is_empty() {
            return Ok(0);
        }
        let mut conn = self.lock()?;
        let tx = conn.transaction()?;
        let n = {
            // idx_chat_item の一意制約に頼って冪等化する。
            // 既知の item_id を持つ重複イベントは IGNORE され、
            // 戻り値には実際に挿入された件数だけが入る。
            let mut stmt = tx.prepare(
                "INSERT OR IGNORE INTO chat_logs
                   (video_id, posted_at_usec, author_channel_id, author_name,
                    kind, message, amount_display, raw_json, item_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, NULLIF(?9, ''))",
            )?;
            let mut n = 0usize;
            for e in events {
                n += stmt.execute(rusqlite::params![
                    e.video_id,
                    e.posted_at_usec,
                    e.author_channel_id,
                    e.author_name,
                    e.kind.as_str(),
                    e.message,
                    e.amount_display,
                    e.raw_json,
                    e.item_id,
                ])?;
            }
            n
        };
        tx.commit()?;
        Ok(n)
    }

    /// チャット履歴の全文検索（FR-8、`chat_history_search`）。
    /// 本文・投稿者名を FTS5 で AND 検索し、時刻の新しい順に返す。
    /// `video_id` 指定でその動画に限定する。
    pub fn chat_search(
        &self,
        video_id: Option<&str>,
        query: &str,
        limit: u32,
    ) -> Result<Vec<crate::model::ChatEvent>, DbError> {
        // 入力を空白でトークン化し、各語をフレーズ指定にする（AND 検索）。
        // FTS5 構文との衝突を避けるため各語を " で囲み、内部の " は "" に逃がす。
        let fts = query
            .split_whitespace()
            .map(|t| format!("\"{}\"", t.replace('"', "\"\"")))
            .collect::<Vec<_>>()
            .join(" ");
        if fts.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT c.video_id, c.posted_at_usec, c.author_channel_id, c.author_name,
                    c.kind, c.message, c.amount_display
             FROM chat_logs_fts f JOIN chat_logs c ON c.id = f.rowid
             WHERE chat_logs_fts MATCH ?1
               AND (?2 IS NULL OR c.video_id = ?2)
             ORDER BY c.posted_at_usec DESC
             LIMIT ?3",
        )?;
        let mut rows = stmt.query(rusqlite::params![fts, video_id, limit])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            let kind: String = row.get(4)?;
            out.push(crate::model::ChatEvent {
                item_id: String::new(),
                video_id: row.get(0)?,
                posted_at_usec: row.get(1)?,
                author_channel_id: row.get(2)?,
                author_name: row.get(3)?,
                kind: crate::model::ChatKind::from_str(&kind),
                message: row.get(5)?,
                amount_display: row.get(6)?,
                ng: false,
                video_offset_ms: None,
                raw_json: String::new(),
            });
        }
        Ok(out)
    }
}
