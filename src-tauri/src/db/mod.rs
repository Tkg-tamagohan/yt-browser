//! SQLite 接続とバージョン管理されたマイグレーション（技術方針 K）。
//!
//! rusqlite は同期 API のため、接続は `Mutex<Connection>` 1 本に集約する（設計書 §1.2）。
//! 書き込みは短いトランザクションに収め、接続プールは持たない。

mod migrations;

use std::collections::HashSet;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

use rusqlite::Connection;
use thiserror::Error;

use crate::model::{
    BlockedChannel, Category, Channel, FavoriteEntry, FeedFilter, FeedItem, Playlist,
    PlaylistEntry, WatchHistory,
};

/// フィード一覧の表示件数上限（設計書 §3.1 の LIMIT 500）。
/// NG フィルタで抜けた分は後続行で埋めるため、走査は述語適合がこの件数に
/// 達するまで続く（`feed_list_filtered`）。
pub const FEED_LIST_LIMIT: usize = 500;

/// `videos` への新規挿入 1 件分（`video_insert_new` の引数）。
#[derive(Debug, Clone)]
pub struct NewVideo<'a> {
    pub video_id: &'a str,
    pub channel_id: &'a str,
    pub channel_title: &'a str,
    pub title: &'a str,
    pub thumbnail_url: Option<&'a str>,
    pub published_at: Option<&'a str>,
    /// 'video' | 'short' | 'live' | 'upcoming'（videos.kind の CHECK 制約）。
    pub kind: &'a str,
}

/// `feed_ingest` / `feed_subscribe` の戻り値。
#[derive(Debug, Clone, Copy, Default)]
pub struct IngestOutcome {
    /// 新たにフィードへ現れた動画数。新規 INSERT と、ライブラリ登録で
    /// 先に作られたプレースホルダ行（published_at NULL）への初回投入を含む。
    pub inserted: usize,
    /// 既存行の既読フラグを未読に戻した数（初回購読投入でのみ発生）。
    pub unread_changed: usize,
}

impl IngestOutcome {
    /// UI 更新が必要な変化（新規挿入または未読への復帰）があったか。
    pub fn touched(&self) -> bool {
        self.inserted + self.unread_changed > 0
    }
}

/// `feed_subscribe` の引数一式。
#[derive(Debug)]
pub struct SubscribeArgs<'a> {
    pub channel_id: &'a str,
    pub title: &'a str,
    pub thumbnail_url: Option<&'a str>,
    pub category_id: Option<i64>,
    pub entries: &'a [NewVideo<'a>],
    pub etag: Option<&'a str>,
    pub last_modified: Option<&'a str>,
}

/// ポーラーが逐次処理するチャンネルの条件付き取得メタ（設計書 §8 channels 表のサブセット）。
#[derive(Debug, Clone)]
pub struct PollTarget {
    pub channel_id: String,
    pub title: String,
    pub rss_etag: Option<String>,
    pub rss_last_modified: Option<String>,
}

#[derive(Debug, Error)]
pub enum DbError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("接続ミューテックスがポイズンされた")]
    Poisoned,
    #[error("マイグレーション v{version} ({name}) の適用に失敗: {source}")]
    Migration {
        version: u32,
        name: &'static str,
        source: rusqlite::Error,
    },
    #[error("対象が存在しない")]
    NotFound,
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// アプリ全体で共有する DB 接続。`Clone` は同一接続を共有する
/// （`Mutex<Connection>` は設計書 §1.2 の通り 1 本のみ）。
#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
    /// 履歴を手動削除された動画の保存抑止（セッション内のみ有効）。
    /// `history_remove` で登録し、明示的な再生開始（`history_upsert`）で解除する。
    /// 再生中プレイヤーの定期・終了保存が削除済み履歴を復活させないためのもの。
    history_suppressed: Arc<Mutex<HashSet<String>>>,
}

impl Db {
    /// `path` のデータベースを開き、未適用マイグレーションをすべて適用する。
    pub fn connect(path: &Path) -> Result<Self, DbError> {
        let conn = Connection::open(path)?;
        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
            history_suppressed: Arc::new(Mutex::new(HashSet::new())),
        };
        db.init_pragmas()?;
        db.migrate()?;
        Ok(db)
    }

    /// テスト用のインメモリ接続。
    #[cfg(test)]
    fn connect_in_memory() -> Result<Self, DbError> {
        let conn = Connection::open_in_memory()?;
        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
            history_suppressed: Arc::new(Mutex::new(HashSet::new())),
        };
        db.init_pragmas()?;
        db.migrate()?;
        Ok(db)
    }

    fn lock(&self) -> Result<MutexGuard<'_, Connection>, DbError> {
        self.conn.lock().map_err(|_| DbError::Poisoned)
    }

    fn init_pragmas(&self) -> Result<(), DbError> {
        let conn = self.lock()?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        Ok(())
    }

    fn migrate(&self) -> Result<(), DbError> {
        let mut conn = self.lock()?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_migrations (
               version INTEGER PRIMARY KEY,
               applied_at TEXT NOT NULL DEFAULT (datetime('now'))
             );",
        )?;
        let current: u32 = conn.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
            [],
            |row| row.get(0),
        )?;
        for m in migrations::MIGRATIONS
            .iter()
            .filter(|m| m.version > current)
        {
            let tx = conn.transaction()?;
            tx.execute_batch(m.sql)
                .map_err(|source| DbError::Migration {
                    version: m.version,
                    name: m.name,
                    source,
                })?;
            tx.execute(
                "INSERT INTO schema_migrations (version) VALUES (?1)",
                [m.version],
            )
            .map_err(|source| DbError::Migration {
                version: m.version,
                name: m.name,
                source,
            })?;
            tx.commit()?;
            tracing::info!(version = m.version, name = m.name, "マイグレーション適用");
        }
        Ok(())
    }

    /// 現在のスキーマバージョン（適用済みの最大 version）。
    pub fn schema_version(&self) -> Result<u32, DbError> {
        let conn = self.lock()?;
        let v = conn.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
            [],
            |row| row.get(0),
        )?;
        Ok(v)
    }

    pub fn setting_get(&self, key: &str) -> Result<Option<String>, DbError> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare("SELECT value FROM settings WHERE key = ?1")?;
        let mut rows = stmt.query([key])?;
        match rows.next()? {
            Some(row) => Ok(Some(row.get(0)?)),
            None => Ok(None),
        }
    }

    pub fn setting_set(&self, key: &str, value: &str) -> Result<(), DbError> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            rusqlite::params![key, value],
        )?;
        Ok(())
    }

    /// 再生開始時に履歴行を確保する。既存行は位置や完了状態を壊さない。
    /// 明示的な再生開始なので、手動削除による保存抑止をここで解除する。
    pub fn history_upsert(&self, video_id: &str) -> Result<(), DbError> {
        self.history_suppressed
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(video_id);
        let conn = self.lock()?;
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
        // 手動削除済みの動画は再生中の定期・終了保存で復活させない
        // （FR-7 履歴削除の確定。再び明示的に再生されれば history_upsert で解除済み）
        if self
            .history_suppressed
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains(video_id)
        {
            return Ok(());
        }
        let position = if completed {
            0
        } else {
            position_sec.max(0.0) as i64
        };
        let conn = self.lock()?;
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

    /// フィード取得結果の投入。チャンネルの存在確認〜エントリ挿入〜取得メタ更新を
    /// 一トランザクションで行い、ポーリング中の購読解除（`channel_delete`）との競合を防ぐ。
    /// 戻り値は `Some(IngestOutcome)`。チャンネルが既に存在しなければ `None`（ロールバック）。
    /// なお ETag / Last-Modified は 200 応答に無ければ NULL で上書きする
    /// （欠落した validator を残すと次回以降の条件付き取得が腐る）。
    pub fn feed_ingest(
        &self,
        channel_id: &str,
        entries: &[NewVideo<'_>],
        etag: Option<&str>,
        last_modified: Option<&str>,
    ) -> Result<Option<IngestOutcome>, DbError> {
        let mut conn = self.lock()?;
        let tx = conn.transaction()?;
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM channels WHERE channel_id = ?1)",
            [channel_id],
            |r| r.get(0),
        )?;
        if !exists {
            return Ok(None);
        }
        let out = ingest_rows(&tx, channel_id, entries, false, etag, last_modified)?;
        tx.commit()?;
        Ok(Some(out))
    }

    /// 購読登録＋初回投入を一トランザクションで行う。
    /// 「channels に行が無い」= 新規購読（または解除済みの再購読）の場合だけ
    /// 既存動画の既読フラグを未読へ戻す（初回投入は未読の仕様）。
    /// 新規判定がトランザクション内なので、並行して走った二つの購読要求の
    /// 後着側は `is_new=false` になり、既読済み動画を未読に戻さない。
    pub fn feed_subscribe(&self, a: &SubscribeArgs<'_>) -> Result<IngestOutcome, DbError> {
        let mut conn = self.lock()?;
        let tx = conn.transaction()?;
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM channels WHERE channel_id = ?1)",
            [a.channel_id],
            |r| r.get(0),
        )?;
        tx.execute(
            "INSERT INTO channels (channel_id, title, thumbnail_url)
             VALUES (?1, ?2, ?3)
             ON CONFLICT (channel_id) DO UPDATE SET
               title = excluded.title,
               thumbnail_url = COALESCE(excluded.thumbnail_url, channels.thumbnail_url)",
            rusqlite::params![a.channel_id, a.title, a.thumbnail_url],
        )?;
        if a.category_id.is_some() {
            tx.execute(
                "UPDATE channels SET category_id = ?2 WHERE channel_id = ?1",
                rusqlite::params![a.channel_id, a.category_id],
            )?;
        }
        let out = ingest_rows(
            &tx,
            a.channel_id,
            a.entries,
            !exists,
            a.etag,
            a.last_modified,
        )?;
        tx.commit()?;
        Ok(out)
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

    /// `list_feed`（設計書 §3.1）。ブロックチャンネルの動画は常に除外する（FR-5）。
    /// SQL の LIMIT は掛けず、述語 `keep` に適合した行だけを `limit` 件までスキャンする
    /// （NG フィルタで先頭が抜けても後続の適合行を拾える。FR-9）。
    /// `feed_list` 相当の無条件取得は `keep: |_| true` として呼ぶ。
    pub fn feed_list_filtered(
        &self,
        filter: &FeedFilter,
        limit: usize,
        keep: impl Fn(&FeedItem) -> bool,
    ) -> Result<Vec<FeedItem>, DbError> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT v.video_id, v.channel_id, v.channel_title, v.title,
                    v.thumbnail_url, v.published_at, v.kind, v.is_read
             FROM videos v
             JOIN channels c ON c.channel_id = v.channel_id
             WHERE v.channel_id NOT IN (SELECT channel_id FROM blocked_channels)
               AND (?1 = 0 OR v.is_read = 0)
               AND (?2 IS NULL
                    OR (?2 = 0 AND c.category_id IS NULL)
                    OR c.category_id = ?2)
               AND (?3 IS NULL
                    OR datetime(v.published_at) >= datetime('now', '-' || ?3 || ' days'))
             ORDER BY v.published_at DESC",
        )?;
        let mut rows = stmt.query(rusqlite::params![
            filter.unread_only as i64,
            filter.category_id,
            filter.days.map(|d| d as i64),
        ])?;
        let mut out = Vec::new();
        while out.len() < limit {
            let Some(row) = rows.next()? else { break };
            let item = FeedItem {
                video_id: row.get(0)?,
                channel_id: row.get(1)?,
                channel_title: row.get(2)?,
                title: row.get(3)?,
                thumbnail_url: row.get(4)?,
                published_at: row.get(5)?,
                kind: row.get(6)?,
                is_read: row.get::<_, i64>(7)? != 0,
            };
            if keep(&item) {
                out.push(item);
            }
        }
        Ok(out)
    }

    /// 個別既読（設計書 §3.1 の `mark_read`）。
    pub fn videos_mark_read(&self, video_ids: &[String]) -> Result<(), DbError> {
        if video_ids.is_empty() {
            return Ok(());
        }
        let conn = self.lock()?;
        let mut stmt = conn.prepare("UPDATE videos SET is_read = 1 WHERE video_id = ?1")?;
        for id in video_ids {
            stmt.execute([id])?;
        }
        Ok(())
    }

    /// 一括既読。
    pub fn videos_mark_all_read(&self) -> Result<u64, DbError> {
        let conn = self.lock()?;
        let n = conn.execute("UPDATE videos SET is_read = 1 WHERE is_read = 0", [])?;
        Ok(n as u64)
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
    pub fn history_remove(&self, video_id: &str) -> Result<(), DbError> {
        let conn = self.lock()?;
        conn.execute("DELETE FROM watch_history WHERE video_id = ?1", [video_id])?;
        drop(conn);
        self.history_suppressed
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(video_id.to_string());
        Ok(())
    }

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
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT i.position, i.video_id, v.title, NULLIF(v.channel_id, ''),
                    v.channel_title, v.thumbnail_url
             FROM playlist_items i JOIN videos v ON v.video_id = i.video_id
             WHERE i.playlist_id = ?1
             ORDER BY i.position, i.rowid",
        )?;
        let mut rows = stmt.query([playlist_id])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(PlaylistEntry {
                position: row.get(0)?,
                video_id: row.get(1)?,
                title: row.get(2)?,
                channel_id: row.get(3)?,
                channel_title: row.get(4)?,
                thumbnail_url: row.get(5)?,
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

    pub fn playlist_remove(&self, playlist_id: i64, video_id: &str) -> Result<(), DbError> {
        let conn = self.lock()?;
        conn.execute(
            "DELETE FROM playlist_items WHERE playlist_id = ?1 AND video_id = ?2",
            rusqlite::params![playlist_id, video_id],
        )?;
        Ok(())
    }

    /// チャットの一括保存（設計書 §6.2）。ポーリング応答 1 回分を 1 トランザクションで。
    pub fn chat_insert_batch(&self, events: &[crate::model::ChatEvent]) -> Result<usize, DbError> {
        if events.is_empty() {
            return Ok(0);
        }
        let mut conn = self.lock()?;
        let tx = conn.transaction()?;
        let n = {
            let mut stmt = tx.prepare(
                "INSERT INTO chat_logs
                   (video_id, posted_at_usec, author_channel_id, author_name,
                    kind, message, amount_display, raw_json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
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
                raw_json: String::new(),
            });
        }
        Ok(out)
    }

    /// NG フィルタ登録（FR-7）。対象・種別・パターンの妥当性は呼び出し側で検証済み。
    pub fn filter_add(
        &self,
        target: &str,
        kind: &str,
        pattern: &str,
    ) -> Result<crate::model::Filter, DbError> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO filters (target, kind, pattern) VALUES (?1, ?2, ?3)",
            rusqlite::params![target, kind, pattern],
        )?;
        let id = conn.last_insert_rowid();
        let created_at: String =
            conn.query_row("SELECT created_at FROM filters WHERE id = ?1", [id], |r| {
                r.get(0)
            })?;
        Ok(crate::model::Filter {
            id,
            target: target.to_string(),
            kind: kind.to_string(),
            pattern: pattern.to_string(),
            enabled: true,
            created_at,
        })
    }

    /// NG フィルタ削除。行が存在しなければ false。
    pub fn filter_remove(&self, id: i64) -> Result<bool, DbError> {
        let conn = self.lock()?;
        let n = conn.execute("DELETE FROM filters WHERE id = ?1", [id])?;
        Ok(n > 0)
    }

    /// NG フィルタ一覧（設定画面と Matcher の rebuild 用）。
    pub fn filter_list(&self) -> Result<Vec<crate::model::Filter>, DbError> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT id, target, kind, pattern, enabled, created_at
             FROM filters ORDER BY id",
        )?;
        let mut rows = stmt.query([])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(crate::model::Filter {
                id: row.get(0)?,
                target: row.get(1)?,
                kind: row.get(2)?,
                pattern: row.get(3)?,
                enabled: row.get::<_, i64>(4)? != 0,
                created_at: row.get(5)?,
            });
        }
        Ok(out)
    }
}
/// 実行する前提で、チャンネルの存在確認はここでは行わない。
/// `reset_unread` が真のとき、既存行（解除済み購読の残骸など）の既読も未読に戻し、
/// 実際に戻した件数を `unread_changed` で返す。
fn ingest_rows(
    tx: &rusqlite::Transaction,
    channel_id: &str,
    entries: &[NewVideo<'_>],
    reset_unread: bool,
    etag: Option<&str>,
    last_modified: Option<&str>,
) -> Result<IngestOutcome, DbError> {
    let mut out = IngestOutcome::default();
    for v in entries {
        // 新規行は is_read=0 で未読投入。既存行は通常はそのままだが、
        // お気に入り・プレイリスト登録で先にできたプレースホルダ
        // （published_at が NULL = まだフィードへ現れていない行）には
        // 初回 RSS 到達の時点で投稿日・種別を埋めて未読へ戻す。
        // 既にフィード行として存在するもの（published_at 非 NULL）は
        // WHERE で除外して既読状態を保つ（既読→未読への戻しは初回購読時
        // の reset_unread 経路だけが担う）。
        out.inserted += tx.execute(
            "INSERT INTO videos
               (video_id, channel_id, channel_title, title, thumbnail_url,
                published_at, kind, is_read)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0)
             ON CONFLICT (video_id) DO UPDATE SET
               channel_id = excluded.channel_id,
               channel_title = COALESCE(excluded.channel_title, videos.channel_title),
               title = CASE WHEN excluded.title <> '' THEN excluded.title
                            ELSE videos.title END,
               thumbnail_url = COALESCE(excluded.thumbnail_url, videos.thumbnail_url),
               published_at = excluded.published_at,
               kind = excluded.kind,
               is_read = 0
             WHERE videos.published_at IS NULL",
            rusqlite::params![
                v.video_id,
                v.channel_id,
                v.channel_title,
                v.title,
                v.thumbnail_url,
                v.published_at,
                v.kind
            ],
        )?;
    }
    if reset_unread {
        for v in entries {
            out.unread_changed += tx.execute(
                "UPDATE videos SET is_read = 0 WHERE video_id = ?1 AND is_read != 0",
                [v.video_id],
            )?;
        }
    }
    tx.execute(
        "UPDATE channels SET
           last_polled_at = datetime('now'),
           rss_etag = ?2,
           rss_last_modified = ?3
         WHERE channel_id = ?1",
        rusqlite::params![channel_id, etag, last_modified],
    )?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrate_applies_all_and_is_idempotent() {
        let db = Db::connect_in_memory().unwrap();
        let v = db.schema_version().unwrap();
        assert_eq!(v, migrations::MIGRATIONS.last().unwrap().version);
        // 2 回目の migrate は何も適用しない
        db.migrate().unwrap();
        assert_eq!(db.schema_version().unwrap(), v);
    }

    #[test]
    fn settings_roundtrip() {
        let db = Db::connect_in_memory().unwrap();
        assert_eq!(db.setting_get("missing").unwrap(), None);
        db.setting_set("k", "v1").unwrap();
        assert_eq!(db.setting_get("k").unwrap().as_deref(), Some("v1"));
        db.setting_set("k", "v2").unwrap();
        assert_eq!(db.setting_get("k").unwrap().as_deref(), Some("v2"));
    }

    #[test]
    fn reopen_preserves_data() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");
        {
            let db = Db::connect(&path).unwrap();
            db.setting_set("persist", "yes").unwrap();
        }
        let db = Db::connect(&path).unwrap();
        assert_eq!(db.setting_get("persist").unwrap().as_deref(), Some("yes"));
    }

    #[test]
    fn history_upsert_then_progress_roundtrip() {
        let db = Db::connect_in_memory().unwrap();
        db.history_upsert("abc123def45").unwrap();
        let h = db.history_get("abc123def45").unwrap().unwrap();
        assert_eq!(h.position_sec, 0);
        assert!(!h.completed);

        db.history_update_progress("abc123def45", "テスト動画", 120.7, Some(300), false)
            .unwrap();
        let h = db.history_get("abc123def45").unwrap().unwrap();
        assert_eq!(h.title, "テスト動画");
        assert_eq!(h.position_sec, 120);
        assert_eq!(h.duration_sec, Some(300));
        assert!(!h.completed);
    }

    #[test]
    fn history_progress_keeps_title_on_empty() {
        let db = Db::connect_in_memory().unwrap();
        db.history_upsert("abc123def45").unwrap();
        db.history_update_progress("abc123def45", "タイトル", 10.0, Some(60), false)
            .unwrap();
        // media-title 未取得の早い保存でタイトルを消さない
        db.history_update_progress("abc123def45", "", 20.0, Some(60), false)
            .unwrap();
        assert_eq!(
            db.history_get("abc123def45").unwrap().unwrap().title,
            "タイトル"
        );
    }

    #[test]
    fn history_completed_resets_position() {
        let db = Db::connect_in_memory().unwrap();
        db.history_upsert("abc123def45").unwrap();
        db.history_update_progress("abc123def45", "t", 290.0, Some(300), false)
            .unwrap();
        db.history_update_progress("abc123def45", "t", 300.0, Some(300), true)
            .unwrap();
        let h = db.history_get("abc123def45").unwrap().unwrap();
        assert!(h.completed);
        assert_eq!(h.position_sec, 0);
    }

    #[test]
    fn history_update_without_upsert_creates_row() {
        let db = Db::connect_in_memory().unwrap();
        // upsert を経由しない直接保存でも行が作られる（upsert 文の両経路を確認）
        db.history_update_progress("new12345678", "v", 5.0, None, false)
            .unwrap();
        assert!(db.history_get("new12345678").unwrap().is_some());
    }

    /// v4 マイグレーション: UC プレフィックスなしで保存された channel_id の修復。
    /// v3 スキーマまで適用した DB に UC 無しのデータを埋めてから v4 を適用する。
    #[test]
    fn migrate_v4_normalizes_uc_less_channel_ids() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE schema_migrations (
               version INTEGER PRIMARY KEY,
               applied_at TEXT NOT NULL DEFAULT (datetime('now'))
             );",
        )
        .unwrap();
        for m in migrations::MIGRATIONS.iter().filter(|m| m.version <= 3) {
            conn.execute_batch(m.sql).unwrap();
            conn.execute(
                "INSERT INTO schema_migrations (version) VALUES (?1)",
                [m.version],
            )
            .unwrap();
        }
        // UC 無しの購読・ブロック・動画・履歴と、UC 付きの重複チャンネルを仕込む。
        // 旧行はカテゴリ・古い購読日時を持つ（衝突統合で引き継がれるべき値）
        conn.execute("INSERT INTO categories (id, name) VALUES (7, 'tech')", [])
            .unwrap();
        conn.execute(
            "INSERT INTO channels (channel_id, title, category_id, subscribed_at)
             VALUES ('XuqSBlHAE6Xw-yeJA0Tunw', 'LTT', 7, '2025-01-01 00:00:00')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO channels (channel_id, title, subscribed_at)
             VALUES ('UCXuqSBlHAE6Xw-yeJA0Tunw', 'LTT-uc', '2026-10-01 00:00:00')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO channels (channel_id, title) VALUES ('shortOnlyChannel000001', 'OnlyOld')",
            [],
        )
        .unwrap();
        // 先頭が UC で始まる 22 文字の旧形式 id（NOT LIKE 判定では取りこぼされる）
        conn.execute(
            "INSERT INTO videos (video_id, channel_id, title)
             VALUES ('v1', 'XuqSBlHAE6Xw-yeJA0Tunw', 'V1'),
                    ('v2', 'UCXuqSBlHAE6Xw-yeJA0Tunw', 'V2'),
                    ('v3', 'UCzzzzzzzzzzzzzzzzzzzz', 'V3')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO blocked_channels (channel_id, title) VALUES ('badChannelX00000000000', 'B')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO blocked_channels (channel_id, title) VALUES ('ucBlockedChannel000000', 'B2')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO watch_history (video_id, title, channel_id)
             VALUES ('v1', 'V1', 'XuqSBlHAE6Xw-yeJA0Tunw')",
            [],
        )
        .unwrap();
        let db = Db {
            conn: Arc::new(Mutex::new(conn)),
            history_suppressed: Arc::new(Mutex::new(HashSet::new())),
        };
        db.migrate().unwrap();

        // UC 付きが既にあるチャンネルは UC 無し行が消え、
        // カテゴリと最古の購読日時は UC 付き行へ引き継がれる
        assert!(db.channel_get("XuqSBlHAE6Xw-yeJA0Tunw").unwrap().is_none());
        let merged = db.channel_get("UCXuqSBlHAE6Xw-yeJA0Tunw").unwrap().unwrap();
        assert_eq!(merged.category_id, Some(7));
        assert_eq!(merged.subscribed_at, "2025-01-01 00:00:00");
        // UC 付きが無いチャンネルはリネームされる
        assert!(db
            .channel_get("UCshortOnlyChannel000001")
            .unwrap()
            .is_some());
        // UC 始まりの 22 文字旧 id も正規化される
        assert!(db
            .channel_get("UCUCzzzzzzzzzzzzzzzzzzzz")
            .unwrap()
            .is_none()); // v3 は channels 行を持たないので videos 側だけ
        let conn = db.lock().unwrap();
        let v3_channel: String = conn
            .query_row(
                "SELECT channel_id FROM videos WHERE video_id = 'v3'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(v3_channel, "UCUCzzzzzzzzzzzzzzzzzzzz");
        // blocked_channels は 22 文字の旧 id が UC 付きに正規化される
        for (raw, expected) in [
            ("badChannelX00000000000", "UCbadChannelX00000000000"),
            ("ucBlockedChannel000000", "UCucBlockedChannel000000"),
        ] {
            let n: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM blocked_channels WHERE channel_id = ?1",
                    [expected],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(n, 1, "{raw} の正規化先が無い");
        }
        // videos / watch_history / blocked_channels も UC 付きに揃う
        let stale: i64 = conn
            .query_row(
                "SELECT (SELECT COUNT(*) FROM channels WHERE length(channel_id) = 22)
                      + (SELECT COUNT(*) FROM videos WHERE length(channel_id) = 22)
                      + (SELECT COUNT(*) FROM blocked_channels WHERE length(channel_id) = 22)
                      + (SELECT COUNT(*) FROM watch_history WHERE length(channel_id) = 22)",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(stale, 0);
        drop(conn);
        // v4 自体も冪等（再適用は schema_migrations で抑止される）
        db.migrate().unwrap();
    }

    /// 購読解除後も残る videos 行はフィード一覧に出さない（JOIN で除外）。
    /// 未読・すべて両モードで確認する。
    #[test]
    fn feed_list_hides_unsubscribed_channel_videos() {
        let db = Db::connect_in_memory().unwrap();
        let entries = [NewVideo {
            video_id: "v1",
            channel_id: "UCchan000000000000001",
            channel_title: "C",
            title: "V1",
            thumbnail_url: None,
            published_at: Some("2026-10-01 00:00:00"),
            kind: "video",
        }];
        db.feed_subscribe(&SubscribeArgs {
            channel_id: "UCchan000000000000001",
            title: "C",
            thumbnail_url: None,
            category_id: None,
            entries: &entries,
            etag: None,
            last_modified: None,
        })
        .unwrap();
        let all = FeedFilter {
            unread_only: false,
            category_id: None,
            days: None,
        };
        assert_eq!(
            db.feed_list_filtered(&all, FEED_LIST_LIMIT, |_| true)
                .unwrap()
                .len(),
            1
        );
        db.channel_delete("UCchan000000000000001").unwrap();
        assert!(db
            .feed_list_filtered(&all, FEED_LIST_LIMIT, |_| true)
            .unwrap()
            .is_empty());
        let unread = FeedFilter {
            unread_only: true,
            category_id: None,
            days: None,
        };
        assert!(db
            .feed_list_filtered(&unread, FEED_LIST_LIMIT, |_| true)
            .unwrap()
            .is_empty());
    }

    /// DB-CH-01: チャットのバッチ保存と FTS5 検索（FR-8）。
    /// 本文・投稿者のどちらにもヒットし、video_id で絞り込める。
    #[test]
    fn chat_insert_and_search() {
        use crate::model::{ChatEvent, ChatKind};
        let db = Db::connect_in_memory().unwrap();
        let ev = |id: i64, vid: &str, author: &str, msg: &str| ChatEvent {
            item_id: String::new(),
            video_id: vid.to_string(),
            posted_at_usec: id,
            author_channel_id: None,
            author_name: Some(author.to_string()),
            kind: ChatKind::Text,
            message: msg.to_string(),
            amount_display: None,
            ng: false,
            raw_json: "{}".to_string(),
        };
        let n = db
            .chat_insert_batch(&[
                ev(1, "v1", "@alice", "こんにちは世界"),
                ev(2, "v1", "@bob", "another line"),
                ev(3, "v2", "@alice", "アルプスの発言"),
            ])
            .unwrap();
        assert_eq!(n, 3);

        // 本文検索（trigram: 部分文字列でもヒットする）
        let hits = db.chat_search(None, "こんにちは", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].message, "こんにちは世界");
        // 投稿者名でもヒットする
        let hits = db.chat_search(None, "alice", 10).unwrap();
        assert_eq!(hits.len(), 2);
        // AND 検索（投稿者＋本文の両方を含む行のみ）
        // 各検索語は trigram の最小語長である 3 文字以上にする
        let hits = db.chat_search(None, "alice アルプ", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].video_id, "v2");
        // video_id 絞り込み
        let hits = db.chat_search(Some("v1"), "alice", 10).unwrap();
        assert_eq!(hits.len(), 1);
        // FTS5 構文を含む入力でも落ちない
        let hits = db.chat_search(None, "NEAR 'こんにちは'", 10).unwrap();
        assert_eq!(hits.len(), 0);
    }

    /// DB-CH-02: NG フィルタの登録・一覧・削除（FR-7）。
    #[test]
    fn filter_roundtrip() {
        let db = Db::connect_in_memory().unwrap();
        let f = db.filter_add("chat_text", "literal", "売り込み").unwrap();
        assert_eq!(f.target, "chat_text");
        assert!(f.enabled);
        assert!(!f.created_at.is_empty());
        assert_eq!(db.filter_list().unwrap().len(), 1);
        assert!(db.filter_remove(f.id).unwrap());
        assert!(db.filter_list().unwrap().is_empty());
        // 存在しない ID の削除は false
        assert!(!db.filter_remove(f.id).unwrap());
    }

    fn vref(video_id: &str, title: &str) -> crate::model::VideoRef {
        crate::model::VideoRef {
            video_id: video_id.to_string(),
            title: title.to_string(),
            channel_id: Some("UCchan000000000000001".to_string()),
            channel_title: Some("テストCH".to_string()),
            thumbnail_url: Some("https://i.ytimg.com/vi/x.jpg".to_string()),
        }
    }

    /// DB-LD-01: お気に入りの追加・一覧・削除（FR-7）。
    /// 動画メタは videos 台帳から JOIN で取り、重複登録は新しい日時に更新しない。
    #[test]
    fn favorite_roundtrip() {
        let db = Db::connect_in_memory().unwrap();
        db.favorite_add(&vref("dQw4w9WgXcQ", "動画A")).unwrap();
        let list = db.favorite_list().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].title, "動画A");
        assert_eq!(list[0].channel_id.as_deref(), Some("UCchan000000000000001"));
        db.favorite_add(&vref("dQw4w9WgXcQ", "動画A")).unwrap();
        assert_eq!(db.favorite_list().unwrap().len(), 1);
        db.favorite_remove("dQw4w9WgXcQ").unwrap();
        assert!(db.favorite_list().unwrap().is_empty());
    }

    /// DB-LD-02: video_upsert は非空値で既存メタを補強し、空値は上書きしない。
    /// channel_id が取れない入力は台帳では空文字になり、一覧では NULL で返す。
    #[test]
    fn video_upsert_merges_metadata() {
        let db = Db::connect_in_memory().unwrap();
        db.favorite_add(&vref("dQw4w9WgXcQ", "元タイトル")).unwrap();
        // 空タイトル・channel_id なしで再登録しても既存値を保つ
        let mut sparse = vref("dQw4w9WgXcQ", "");
        sparse.channel_id = None;
        db.favorite_add(&sparse).unwrap();
        let list = db.favorite_list().unwrap();
        assert_eq!(list[0].title, "元タイトル");
        assert_eq!(list[0].channel_id.as_deref(), Some("UCchan000000000000001"));
        // channel_id を一切持たない動画は NULLIF で None に見える
        let mut no_ch = vref("nochan12345", "無名");
        no_ch.channel_id = None;
        no_ch.channel_title = None;
        db.favorite_add(&no_ch).unwrap();
        let e = db
            .favorite_list()
            .unwrap()
            .into_iter()
            .find(|x| x.video_id == "nochan12345")
            .unwrap();
        assert_eq!(e.channel_id, None);
    }

    /// DB-LD-03: プレイリストの CRUD とアイテム順序（FR-7）。
    /// position は末尾追加で連番、重複追加は位置を維持して無視される。
    #[test]
    fn playlist_crud_and_order() {
        let db = Db::connect_in_memory().unwrap();
        let pl = db.playlist_create("夜の選曲").unwrap();
        assert_eq!(pl.item_count, 0);
        db.playlist_add(pl.id, &vref("aaaaaaaaaa1", "A")).unwrap();
        db.playlist_add(pl.id, &vref("bbbbbbbbbb2", "B")).unwrap();
        // 重複追加は位置を維持して無視される
        db.playlist_add(pl.id, &vref("aaaaaaaaaa1", "A")).unwrap();
        let items = db.playlist_items(pl.id).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].video_id, "aaaaaaaaaa1");
        assert_eq!(items[1].video_id, "bbbbbbbbbb2");
        assert_eq!(items[0].position, 0);
        // 一覧の item_count も 2
        assert_eq!(db.playlist_list().unwrap()[0].item_count, 2);
        // 途中削除でも残りの順序は変わらない
        db.playlist_remove(pl.id, "aaaaaaaaaa1").unwrap();
        let items = db.playlist_items(pl.id).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].video_id, "bbbbbbbbbb2");
        // リネームと削除
        db.playlist_rename(pl.id, "朝の選曲").unwrap();
        assert_eq!(db.playlist_list().unwrap()[0].name, "朝の選曲");
        db.playlist_delete(pl.id).unwrap();
        assert!(db.playlist_items(pl.id).unwrap().is_empty());
        assert!(db.playlist_list().unwrap().is_empty());
        // 存在しない id への rename / add は NotFound
        assert!(matches!(
            db.playlist_rename(999, "x"),
            Err(DbError::NotFound)
        ));
        assert!(matches!(
            db.playlist_add(999, &vref("cccccccccc3", "C")),
            Err(DbError::NotFound)
        ));
    }

    /// DB-LD-04: 履歴一覧は新しい順、history_remove で個別削除（FR-7、仕様決定 I）。
    #[test]
    fn history_list_and_remove() {
        let db = Db::connect_in_memory().unwrap();
        for (i, v) in ["a1", "a2", "a3"].iter().enumerate() {
            db.history_update_progress(v, "t", i as f64, Some(60), false)
                .unwrap();
        }
        let list = db.history_list(500).unwrap();
        assert_eq!(list.len(), 3);
        // last_watched_at は datetime('now') 同時刻でも rowid 降順で
        // 決定的になる（同一時刻の順序不定を防ぐ）
        assert_eq!(list[0].video_id, "a3");
        assert_eq!(list[1].video_id, "a2");
        assert_eq!(list[2].video_id, "a1");
        assert!(list.iter().all(|h| !h.completed));
        db.history_remove("a2").unwrap();
        let list = db.history_list(500).unwrap();
        assert_eq!(list.len(), 2);
        assert!(!list.iter().any(|h| h.video_id == "a2"));
        // limit が効く
        assert_eq!(db.history_list(1).unwrap().len(), 1);
    }

    /// DB-LD-05: 手動削除した履歴は再生中プレイヤーの進捗保存で復活しない。
    /// 明示的な再生開始（history_upsert）で抑止は解除される。
    #[test]
    fn history_remove_suppresses_inflight_progress() {
        let db = Db::connect_in_memory().unwrap();
        db.history_upsert("abc123def45").unwrap();
        db.history_update_progress("abc123def45", "t", 10.0, Some(60), false)
            .unwrap();
        // 視聴中にライブラリから削除 → 以後の進捗保存は書き込まない
        db.history_remove("abc123def45").unwrap();
        db.history_update_progress("abc123def45", "t", 30.0, Some(60), false)
            .unwrap();
        assert!(db.history_get("abc123def45").unwrap().is_none());
        // 明示的な再生開始で抑止解除 → 履歴が再び残る
        db.history_upsert("abc123def45").unwrap();
        db.history_update_progress("abc123def45", "t", 5.0, Some(60), false)
            .unwrap();
        let h = db.history_get("abc123def45").unwrap().unwrap();
        assert_eq!(h.position_sec, 5);
    }

    /// DB-LD-06: ライブラリ登録で先に作ったプレースホルダ行に、
    /// 後着の RSS 投入で投稿日・種別・未読を埋める。既存のフィード行は
    /// 既読状態を含めて書き換えない。
    #[test]
    fn feed_ingest_backfills_library_placeholder() {
        let db = Db::connect_in_memory().unwrap();
        let ch = "UCchan000000000000001";
        // 購読チャンネルと、お気に入り登録で先にできた行（published_at NULL）
        db.feed_subscribe(&SubscribeArgs {
            channel_id: ch,
            title: "テストCH",
            thumbnail_url: None,
            category_id: None,
            entries: &[],
            etag: None,
            last_modified: None,
        })
        .unwrap();
        let mut v = vref("dQw4w9WgXcQ", "動画A");
        v.channel_id = Some(ch.to_string());
        db.favorite_add(&v).unwrap();

        // RSS で同じ動画が届く: published_at / kind が埋まり未読になる
        let entry = NewVideo {
            video_id: "dQw4w9WgXcQ",
            channel_id: ch,
            channel_title: "テストCH",
            title: "動画A",
            thumbnail_url: None,
            published_at: Some("2026-10-07T00:00:00+00:00"),
            kind: "video",
        };
        let out = db
            .feed_ingest(ch, std::slice::from_ref(&entry), None, None)
            .unwrap()
            .unwrap();
        assert_eq!(out.inserted, 1);
        let feed = db
            .feed_list_filtered(&FeedFilter::default(), FEED_LIST_LIMIT, |_| true)
            .unwrap();
        let item = feed.iter().find(|i| i.video_id == "dQw4w9WgXcQ").unwrap();
        assert_eq!(
            item.published_at.as_deref(),
            Some("2026-10-07T00:00:00+00:00")
        );
        assert!(!item.is_read);

        // 既読にしても再投入で既読状態は保つ（プレースホルダではないため）
        db.videos_mark_read(&["dQw4w9WgXcQ".to_string()]).unwrap();
        db.feed_ingest(ch, &[entry], None, None).unwrap();
        let feed = db
            .feed_list_filtered(&FeedFilter::default(), FEED_LIST_LIMIT, |_| true)
            .unwrap();
        let item = feed.iter().find(|i| i.video_id == "dQw4w9WgXcQ").unwrap();
        assert!(item.is_read);
    }
}
