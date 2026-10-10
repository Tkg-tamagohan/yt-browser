//! videos 台帳への登録と、favorites / playlists / playlist_items テーブル。

use super::*;
use crate::model::{FavoriteEntry, Playlist, PlaylistEntry};

impl Db {
    /// `videos` 台帳への登録（お気に入り・プレイリスト追加の前段）。
    /// フィード由来でない動画は channel_id を `VideoRef` の値（または空文字）で
    /// 登録する。空のチャンネル ID は `channels` に JOIN しないためフィードに
    /// 現れない。既存行は非空の値だけで上書きし、is_read は触らない。
    fn video_upsert(conn: &Connection, v: &crate::model::VideoRef) -> Result<(), DbError> {
        conn.execute(
            "INSERT INTO videos (video_id, channel_id, channel_title, title, thumbnail_url, kind)
             VALUES (?1, ?2, ?3, ?4, ?5, 'video')
             ON CONFLICT (video_id) DO UPDATE SET
               channel_id = CASE WHEN excluded.channel_id <> ''
                                 THEN excluded.channel_id ELSE videos.channel_id END,
               channel_title = COALESCE(excluded.channel_title, videos.channel_title),
               title = CASE WHEN excluded.title <> ''
                            THEN excluded.title ELSE videos.title END,
               thumbnail_url = COALESCE(excluded.thumbnail_url, videos.thumbnail_url)",
            rusqlite::params![
                v.video_id,
                v.channel_id.as_deref().unwrap_or(""),
                v.channel_title,
                v.title,
                v.thumbnail_url,
            ],
        )?;
        Ok(())
    }

    /// お気に入り追加。`videos` 登録と同じトランザクションで行う。
    pub fn favorite_add(&self, v: &crate::model::VideoRef) -> Result<(), DbError> {
        let mut conn = self.lock()?;
        let tx = conn.transaction()?;
        Self::video_upsert(&tx, v)?;
        tx.execute(
            "INSERT OR IGNORE INTO favorites (video_id) VALUES (?1)",
            [&v.video_id],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn favorite_remove(&self, video_id: &str) -> Result<(), DbError> {
        let conn = self.lock()?;
        conn.execute("DELETE FROM favorites WHERE video_id = ?1", [video_id])?;
        Ok(())
    }

    /// お気に入り一覧。追加が新しい順。
    pub fn favorite_list(&self) -> Result<Vec<FavoriteEntry>, DbError> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT f.video_id, v.title, NULLIF(v.channel_id, ''), v.channel_title,
                    v.thumbnail_url, f.added_at
             FROM favorites f JOIN videos v ON v.video_id = f.video_id
             ORDER BY f.added_at DESC",
        )?;
        let mut rows = stmt.query([])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(FavoriteEntry {
                video_id: row.get(0)?,
                title: row.get(1)?,
                channel_id: row.get(2)?,
                channel_title: row.get(3)?,
                thumbnail_url: row.get(4)?,
                added_at: row.get(5)?,
            });
        }
        Ok(out)
    }

    /// プレイリスト一覧。`item_count` は LEFT JOIN の件数集計。
    pub fn playlist_list(&self) -> Result<Vec<Playlist>, DbError> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT p.id, p.name, p.sort_order, COUNT(i.video_id) AS item_count
             FROM playlists p LEFT JOIN playlist_items i ON i.playlist_id = p.id
             GROUP BY p.id
             ORDER BY p.sort_order, p.id",
        )?;
        let mut rows = stmt.query([])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(Playlist {
                id: row.get(0)?,
                name: row.get(1)?,
                sort_order: row.get(2)?,
                item_count: row.get(3)?,
            });
        }
        Ok(out)
    }

    pub fn playlist_create(&self, name: &str) -> Result<Playlist, DbError> {
        let conn = self.lock()?;
        conn.execute("INSERT INTO playlists (name) VALUES (?1)", [name])?;
        Ok(Playlist {
            id: conn.last_insert_rowid(),
            name: name.to_string(),
            sort_order: 0,
            item_count: 0,
        })
    }

    /// 名称変更。対象が存在しなければ `DbError::NotFound`。
    pub fn playlist_rename(&self, playlist_id: i64, name: &str) -> Result<(), DbError> {
        let conn = self.lock()?;
        let n = conn.execute(
            "UPDATE playlists SET name = ?1 WHERE id = ?2",
            rusqlite::params![name, playlist_id],
        )?;
        if n == 0 {
            return Err(DbError::NotFound);
        }
        Ok(())
    }

    /// プレイリスト削除。`playlist_items` は ON DELETE CASCADE で連動して消える。
    pub fn playlist_delete(&self, playlist_id: i64) -> Result<(), DbError> {
        let conn = self.lock()?;
        let n = conn.execute("DELETE FROM playlists WHERE id = ?1", [playlist_id])?;
        if n == 0 {
            return Err(DbError::NotFound);
        }
        Ok(())
    }

    /// プレイリストの中身。position 昇順。
    pub fn playlist_items(&self, playlist_id: i64) -> Result<Vec<PlaylistEntry>, DbError> {
        self.playlist_items_page(playlist_id, None, None)
    }

    /// `playlist_items` の分割取得版（FR-25、仕様決定 AR）。
    /// `after_position` より後ろの項目を `limit` 件まで返す。
    /// どちらも None のとき全件（従来の `playlist_items` と同じ）。
    pub fn playlist_items_page(
        &self,
        playlist_id: i64,
        after_position: Option<i64>,
        limit: Option<u32>,
    ) -> Result<Vec<PlaylistEntry>, DbError> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT i.position, i.video_id, v.title, NULLIF(v.channel_id, ''),
                    v.channel_title, v.thumbnail_url, v.published_at
             FROM playlist_items i JOIN videos v ON v.video_id = i.video_id
             WHERE i.playlist_id = ?1 AND (?2 IS NULL OR i.position > ?2)
             ORDER BY i.position, i.rowid
             LIMIT ?3",
        )?;
        // SQLite は LIMIT -1 で制限なしになる
        let mut rows = stmt.query(rusqlite::params![
            playlist_id,
            after_position,
            limit.map(|l| l as i64).unwrap_or(-1)
        ])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(PlaylistEntry {
                position: row.get(0)?,
                video_id: row.get(1)?,
                title: row.get(2)?,
                channel_id: row.get(3)?,
                channel_title: row.get(4)?,
                thumbnail_url: row.get(5)?,
                published_at: row.get(6)?,
            });
        }
        Ok(out)
    }

    /// プレイリスト末尾への追加。`videos` 登録と同じトランザクションで行い、
    /// position は末尾 + 1。既登録の動画は重複登録しない（位置は維持）。
    /// 存在しないプレイリストには `DbError::NotFound`。
    pub fn playlist_add(
        &self,
        playlist_id: i64,
        v: &crate::model::VideoRef,
    ) -> Result<(), DbError> {
        let mut conn = self.lock()?;
        let tx = conn.transaction()?;
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM playlists WHERE id = ?1)",
            [playlist_id],
            |r| r.get(0),
        )?;
        if !exists {
            return Err(DbError::NotFound);
        }
        Self::video_upsert(&tx, v)?;
        tx.execute(
            "INSERT OR IGNORE INTO playlist_items (playlist_id, video_id, position)
             VALUES (?1, ?2,
                     COALESCE((SELECT MAX(position) + 1 FROM playlist_items
                               WHERE playlist_id = ?1), 0))",
            rusqlite::params![playlist_id, v.video_id],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// 取り込みなどの一括追加。`playlist_add` の末尾追加を 1 トランザクションで
    /// まとめて行う（重複は位置を維持して無視）。
    pub fn playlist_add_many(
        &self,
        playlist_id: i64,
        items: &[crate::model::VideoRef],
    ) -> Result<(), DbError> {
        let mut conn = self.lock()?;
        let tx = conn.transaction()?;
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM playlists WHERE id = ?1)",
            [playlist_id],
            |r| r.get(0),
        )?;
        if !exists {
            return Err(DbError::NotFound);
        }
        for v in items {
            Self::video_upsert(&tx, v)?;
            tx.execute(
                "INSERT OR IGNORE INTO playlist_items (playlist_id, video_id, position)
                 VALUES (?1, ?2,
                         COALESCE((SELECT MAX(position) + 1 FROM playlist_items
                                   WHERE playlist_id = ?1), 0))",
                rusqlite::params![playlist_id, v.video_id],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// 項目順の一括書き換え（仕様決定 T）。`video_ids` が現項目と
    /// 同一集合（順不同・重複なし）であることをこのトランザクション内で
    /// 検証し、一致しない場合は `DbError::MismatchedItems` で変更しない
    /// （検証と更新の分離による並行編集の誤適用を防ぐ）。
    /// 存在しないプレイリストは `DbError::NotFound`。
    pub fn playlist_reorder(&self, playlist_id: i64, video_ids: &[String]) -> Result<(), DbError> {
        let mut conn = self.lock()?;
        let tx = conn.transaction()?;
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM playlists WHERE id = ?1)",
            [playlist_id],
            |r| r.get(0),
        )?;
        if !exists {
            return Err(DbError::NotFound);
        }
        let mut current: Vec<String> = Vec::new();
        {
            let mut stmt = tx.prepare(
                "SELECT video_id FROM playlist_items WHERE playlist_id = ?1 ORDER BY position",
            )?;
            let mut rows = stmt.query([playlist_id])?;
            while let Some(row) = rows.next()? {
                current.push(row.get(0)?);
            }
        }
        // 大きなプレイリストでも線形で比較できるよう集合照合にする
        let incoming: std::collections::HashSet<&String> = video_ids.iter().collect();
        let same = video_ids.len() == current.len()
            && incoming.len() == video_ids.len()
            && current.iter().all(|id| incoming.contains(id));
        if !same {
            return Err(DbError::MismatchedItems);
        }
        for (pos, vid) in video_ids.iter().enumerate() {
            tx.execute(
                "UPDATE playlist_items SET position = ?3
                 WHERE playlist_id = ?1 AND video_id = ?2",
                rusqlite::params![playlist_id, vid, pos as i64],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// 項目順の一括反転（FR-11、仕様決定 Z）。
    /// 読み出しと書き込みを同一書き込みトランザクションで行い、
    /// 並行編集の途中状態を拾わない。存在しないプレイリストは `DbError::NotFound`。
    pub fn playlist_reverse(&self, playlist_id: i64) -> Result<(), DbError> {
        let mut conn = self.lock()?;
        let tx = conn.transaction()?;
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM playlists WHERE id = ?1)",
            [playlist_id],
            |r| r.get(0),
        )?;
        if !exists {
            return Err(DbError::NotFound);
        }
        let mut current: Vec<String> = Vec::new();
        {
            let mut stmt = tx.prepare(
                "SELECT video_id FROM playlist_items WHERE playlist_id = ?1 ORDER BY position",
            )?;
            let mut rows = stmt.query([playlist_id])?;
            while let Some(row) = rows.next()? {
                current.push(row.get(0)?);
            }
        }
        for (pos, vid) in current.iter().rev().enumerate() {
            tx.execute(
                "UPDATE playlist_items SET position = ?3
                 WHERE playlist_id = ?1 AND video_id = ?2",
                rusqlite::params![playlist_id, vid, pos as i64],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn playlist_remove(&self, playlist_id: i64, video_id: &str) -> Result<(), DbError> {
        let conn = self.lock()?;
        conn.execute(
            "DELETE FROM playlist_items WHERE playlist_id = ?1 AND video_id = ?2",
            rusqlite::params![playlist_id, video_id],
        )?;
        Ok(())
    }
}
