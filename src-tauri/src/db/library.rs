//! videos 台帳への登録と、favorites / playlists / playlist_items /
//! watch_later テーブル、ライブラリ横断検索。

use super::*;
use crate::model::{
    FavoriteEntry, LibrarySearchItem, LibrarySource, Playlist, PlaylistEntry, WatchLaterEntry,
};

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

    /// お気に入り解除。戻り値は削除した行のスナップショット
    /// （FR-54、仕様決定 CE の「元に戻す」復元用）。対象行が無ければ None。
    pub fn favorite_remove(&self, video_id: &str) -> Result<Option<FavoriteEntry>, DbError> {
        let conn = self.lock()?;
        let removed = Self::favorite_get_conn(&conn, video_id)?;
        conn.execute("DELETE FROM favorites WHERE video_id = ?1", [video_id])?;
        Ok(removed)
    }

    /// `favorite_remove` で取得したスナップショットの復元
    /// （FR-54、仕様決定 CE）。`added_at` も削除前の値へ戻す。
    /// `videos` 台帳のメタは `favorite_add` と同じ規則で補強する。
    /// 既存行がある場合はスナップショットで置き換える。
    pub fn favorite_restore(&self, entry: &FavoriteEntry) -> Result<(), DbError> {
        let mut conn = self.lock()?;
        let tx = conn.transaction()?;
        Self::video_upsert(
            &tx,
            &crate::model::VideoRef {
                video_id: entry.video_id.clone(),
                title: entry.title.clone(),
                channel_id: entry.channel_id.clone(),
                channel_title: entry.channel_title.clone(),
                thumbnail_url: entry.thumbnail_url.clone(),
            },
        )?;
        tx.execute(
            "INSERT OR REPLACE INTO favorites (video_id, added_at) VALUES (?1, ?2)",
            rusqlite::params![entry.video_id, entry.added_at],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// お気に入り 1 件分（`favorite_remove` のスナップショット取得用）。
    /// 表示メタは `favorite_list` と同じ JOIN で取る。
    fn favorite_get_conn(
        conn: &Connection,
        video_id: &str,
    ) -> Result<Option<FavoriteEntry>, DbError> {
        let mut stmt = conn.prepare(
            "SELECT f.video_id, v.title, NULLIF(v.channel_id, ''), v.channel_title,
                    v.thumbnail_url, f.added_at
             FROM favorites f JOIN videos v ON v.video_id = f.video_id
             WHERE f.video_id = ?1",
        )?;
        let mut rows = stmt.query([video_id])?;
        match rows.next()? {
            Some(row) => Ok(Some(FavoriteEntry {
                video_id: row.get(0)?,
                title: row.get(1)?,
                channel_id: row.get(2)?,
                channel_title: row.get(3)?,
                thumbnail_url: row.get(4)?,
                added_at: row.get(5)?,
            })),
            None => Ok(None),
        }
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

    /// プレイリスト作成（FR-7）。一覧の末尾に置くため `sort_order` は
    /// 既存の最大値 + 1 とする（FR-35、仕様決定 BF）。並べ替え前の全行 0 の
    /// データでも `ORDER BY sort_order, id` の作成順を崩さない。
    /// 取り込み（`playlist_import`）もこの経路で作る。
    pub fn playlist_create(&self, name: &str) -> Result<Playlist, DbError> {
        let conn = self.lock()?;
        let sort_order: i64 = conn.query_row(
            "INSERT INTO playlists (name, sort_order)
             VALUES (?1, (SELECT COALESCE(MAX(sort_order), -1) + 1 FROM playlists))
             RETURNING sort_order",
            [name],
            |r| r.get(0),
        )?;
        Ok(Playlist {
            id: conn.last_insert_rowid(),
            name: name.to_string(),
            sort_order,
            item_count: 0,
        })
    }

    /// プレイリスト一覧の並べ替え（FR-35、仕様決定 BF）。`playlist_ids` が
    /// 現在のプレイリスト全件と同一集合（順不同・重複なし）であることを
    /// このトランザクション内で検証し、一致しない場合は
    /// `DbError::MismatchedItems` で変更しない。一致すれば `sort_order` を
    /// 渡した順に 0..n-1 へ振り直す（`playlist_reorder` と同じ方針）。
    pub fn playlist_reorder_lists(&self, playlist_ids: &[i64]) -> Result<(), DbError> {
        let mut conn = self.lock()?;
        let tx = conn.transaction()?;
        let mut current: Vec<i64> = Vec::new();
        {
            let mut stmt = tx.prepare("SELECT id FROM playlists")?;
            let mut rows = stmt.query([])?;
            while let Some(row) = rows.next()? {
                current.push(row.get(0)?);
            }
        }
        let incoming: std::collections::HashSet<i64> = playlist_ids.iter().copied().collect();
        let same = playlist_ids.len() == current.len()
            && incoming.len() == playlist_ids.len()
            && current.iter().all(|id| incoming.contains(id));
        if !same {
            return Err(DbError::MismatchedItems);
        }
        for (pos, id) in playlist_ids.iter().enumerate() {
            tx.execute(
                "UPDATE playlists SET sort_order = ?2 WHERE id = ?1",
                rusqlite::params![id, pos as i64],
            )?;
        }
        tx.commit()?;
        Ok(())
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

    /// プレイリスト項目の個別削除。戻り値は削除した行のスナップショット
    /// （FR-54、仕様決定 CE の「元に戻す」復元用）。対象行が無ければ None。
    /// 削除しても他項目の position は振り直さない（空き番は残る）。
    pub fn playlist_remove(
        &self,
        playlist_id: i64,
        video_id: &str,
    ) -> Result<Option<PlaylistEntry>, DbError> {
        let conn = self.lock()?;
        let removed = Self::playlist_item_get_conn(&conn, playlist_id, video_id)?;
        conn.execute(
            "DELETE FROM playlist_items WHERE playlist_id = ?1 AND video_id = ?2",
            rusqlite::params![playlist_id, video_id],
        )?;
        Ok(removed)
    }

    /// `playlist_remove` で取得したスナップショットの復元
    /// （FR-54、仕様決定 CE）。スナップショットの `position` が他項目に
    /// 使われていなければ元位置へ、埋まっていれば末尾（MAX(position)+1）へ
    /// 挿す（暫定: 削除と追加の間に並べ替えや末尾への追加が走った場合の
    /// 収束先）。同一動画が既に登録済みなら既存行をそのまま返す（冪等）。
    /// 戻り値は実際に登録された行（実効 position 付き）。
    /// 存在しないプレイリストは `DbError::NotFound`。
    pub fn playlist_item_restore(
        &self,
        playlist_id: i64,
        v: &crate::model::VideoRef,
        position: i64,
    ) -> Result<PlaylistEntry, DbError> {
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
        let occupied: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM playlist_items
              WHERE playlist_id = ?1 AND position = ?2)",
            rusqlite::params![playlist_id, position],
            |r| r.get(0),
        )?;
        let target = if occupied {
            tx.query_row(
                "SELECT COALESCE(MAX(position) + 1, 0) FROM playlist_items
                  WHERE playlist_id = ?1",
                [playlist_id],
                |r| r.get::<_, i64>(0),
            )?
        } else {
            position
        };
        tx.execute(
            "INSERT OR IGNORE INTO playlist_items (playlist_id, video_id, position)
             VALUES (?1, ?2, ?3)",
            rusqlite::params![playlist_id, v.video_id, target],
        )?;
        let entry = Self::playlist_item_get_conn(&tx, playlist_id, &v.video_id)?
            .ok_or(DbError::NotFound)?;
        tx.commit()?;
        Ok(entry)
    }

    /// プレイリスト項目 1 件（削除スナップショットと復元結果の取得用）。
    /// 表示メタは `playlist_items_page` と同じ JOIN で取る。
    fn playlist_item_get_conn(
        conn: &Connection,
        playlist_id: i64,
        video_id: &str,
    ) -> Result<Option<PlaylistEntry>, DbError> {
        let mut stmt = conn.prepare(
            "SELECT i.position, i.video_id, v.title, NULLIF(v.channel_id, ''),
                    v.channel_title, v.thumbnail_url, v.published_at
             FROM playlist_items i JOIN videos v ON v.video_id = i.video_id
             WHERE i.playlist_id = ?1 AND i.video_id = ?2",
        )?;
        let mut rows = stmt.query(rusqlite::params![playlist_id, video_id])?;
        match rows.next()? {
            Some(row) => Ok(Some(PlaylistEntry {
                position: row.get(0)?,
                video_id: row.get(1)?,
                title: row.get(2)?,
                channel_id: row.get(3)?,
                channel_title: row.get(4)?,
                thumbnail_url: row.get(5)?,
                published_at: row.get(6)?,
            })),
            None => Ok(None),
        }
    }

    /// 「あとで見る」登録（FR-59、仕様決定 CJ）。`videos` 台帳への登録と
    /// 同じトランザクションで行う。既登録の動画は追加日時を変えない
    /// （INSERT OR IGNORE で冪等）。
    pub fn watch_later_add(&self, v: &crate::model::VideoRef) -> Result<(), DbError> {
        let mut conn = self.lock()?;
        let tx = conn.transaction()?;
        Self::video_upsert(&tx, v)?;
        tx.execute(
            "INSERT OR IGNORE INTO watch_later (video_id) VALUES (?1)",
            [&v.video_id],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// 「あとで見る」解除。未登録でも正常終了（冪等）。
    pub fn watch_later_remove(&self, video_id: &str) -> Result<(), DbError> {
        let conn = self.lock()?;
        conn.execute("DELETE FROM watch_later WHERE video_id = ?1", [video_id])?;
        Ok(())
    }

    /// 「あとで見る」一覧（FR-59）。追加が新しい順。
    /// 動画メタは `videos` 台帳から JOIN で取る（favorites と同じ規則）。
    pub fn watch_later_list(&self) -> Result<Vec<WatchLaterEntry>, DbError> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT w.video_id, v.title, NULLIF(v.channel_id, ''), v.channel_title,
                    v.thumbnail_url, w.added_at
             FROM watch_later w JOIN videos v ON v.video_id = w.video_id
             ORDER BY w.added_at DESC",
        )?;
        let mut rows = stmt.query([])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(WatchLaterEntry {
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

    /// ライブラリ横断検索（FR-60、仕様決定 CK）。
    /// 履歴・お気に入り・プレイリスト項目・あとで見るをタイトルと
    /// チャンネル名の部分一致 LIKE で検索し、同一動画は 1 件にまとめて
    /// ヒットした区分の列（`sources`）を返す。`query` の空・空白のみは
    /// 0 件を返す（全件列挙の用途ではない。暫定解釈）。
    /// 表示メタは区分をまたいだ先出の行を使い、空欄は後出の行で補完する。
    /// 並びは「履歴の新しい順 → お気に入りの新しい順 → プレイリストの
    /// 登録順 → あとで見るの新しい順」の先出順（暫定解釈）。
    /// `limit` は統合後の件数上限。
    pub fn library_search(
        &self,
        query: &str,
        limit: u32,
    ) -> Result<Vec<LibrarySearchItem>, DbError> {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return Ok(Vec::new());
        }
        let pattern = like_pattern(trimmed);
        let conn = self.lock()?;
        let mut items: Vec<LibrarySearchItem> = Vec::new();
        let mut index: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        {
            // 履歴は自身の列に加えて videos 台帳のメタでも照合する
            // （履歴行の channel 列が未入力でも台帳にあれば拾う。
            // 照合は title・channel_title の部分一致で FR-61 と同じ規則）
            let mut stmt = conn.prepare(
                "SELECT w.video_id,
                        COALESCE(NULLIF(w.title, ''), v.title),
                        COALESCE(NULLIF(w.channel_id, ''), NULLIF(v.channel_id, '')),
                        COALESCE(w.channel_title, v.channel_title),
                        v.thumbnail_url
                 FROM watch_history w
                 LEFT JOIN videos v ON v.video_id = w.video_id
                 WHERE w.title LIKE ?1 ESCAPE '\\'
                    OR w.channel_title LIKE ?1 ESCAPE '\\'
                    OR v.title LIKE ?1 ESCAPE '\\'
                    OR v.channel_title LIKE ?1 ESCAPE '\\'
                 ORDER BY w.last_watched_at DESC, w.rowid DESC",
            )?;
            let mut rows = stmt.query([&pattern])?;
            while let Some(row) = rows.next()? {
                merge_search_hit(&mut items, &mut index, LibrarySource::History, row)?;
            }
        }
        for (source, table, order) in [
            (LibrarySource::Favorites, "favorites", "added_at DESC"),
            (
                LibrarySource::Playlists,
                "playlist_items",
                "playlist_id, position",
            ),
            (LibrarySource::WatchLater, "watch_later", "added_at DESC"),
        ] {
            // 動画メタは videos 台帳から JOIN で取る（一覧取得と同じ規則）
            let mut stmt = conn.prepare(&format!(
                "SELECT i.video_id, v.title, NULLIF(v.channel_id, ''),
                        v.channel_title, v.thumbnail_url
                 FROM {table} i JOIN videos v ON v.video_id = i.video_id
                 WHERE v.title LIKE ?1 ESCAPE '\\'
                    OR v.channel_title LIKE ?1 ESCAPE '\\'
                 ORDER BY i.{order}"
            ))?;
            let mut rows = stmt.query([&pattern])?;
            while let Some(row) = rows.next()? {
                merge_search_hit(&mut items, &mut index, source, row)?;
            }
        }
        items.truncate(limit as usize);
        Ok(items)
    }
}

/// `library_search` の 1 行分の統合。同一動画の 2 件目以降は
/// `sources` への区分追加と空欄メタの補完だけを行う。
fn merge_search_hit(
    items: &mut Vec<LibrarySearchItem>,
    index: &mut std::collections::HashMap<String, usize>,
    source: LibrarySource,
    row: &rusqlite::Row<'_>,
) -> Result<(), rusqlite::Error> {
    let video_id: String = row.get(0)?;
    if let Some(&i) = index.get(&video_id) {
        let it = &mut items[i];
        if it.title.is_empty() {
            it.title = row.get(1)?;
        }
        if it.channel_id.is_none() {
            it.channel_id = row.get(2)?;
        }
        if it.channel_title.is_none() {
            it.channel_title = row.get(3)?;
        }
        if it.thumbnail_url.is_none() {
            it.thumbnail_url = row.get(4)?;
        }
        if !it.sources.contains(&source) {
            it.sources.push(source);
        }
    } else {
        index.insert(video_id.clone(), items.len());
        items.push(LibrarySearchItem {
            video_id,
            title: row.get(1)?,
            channel_id: row.get(2)?,
            channel_title: row.get(3)?,
            thumbnail_url: row.get(4)?,
            sources: vec![source],
        });
    }
    Ok(())
}
