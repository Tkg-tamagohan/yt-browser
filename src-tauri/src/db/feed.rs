//! videos テーブルのフィード投入・一覧・既読管理と、公開 API 型。

use super::*;
use crate::model::{FeedFilter, FeedItem};

/// フィード一覧の表示件数上限（設計書 §3.1 の LIMIT 500）。
/// NG フィルタで抜けた分は後続行で埋めるため、走査は述語適合がこの件数に
/// 達するまで続く（`feed_list_filtered`）。
pub const FEED_LIST_LIMIT: usize = 500;

/// `videos` への新規挿入 1 件分（`feed_ingest` / `feed_subscribe` の `entries` 要素）。
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
#[derive(Debug, Clone, Default)]
pub struct IngestOutcome {
    /// 新たにフィードへ現れた動画数。新規 INSERT と、ライブラリ登録で
    /// 先に作られたプレースホルダ行（published_at NULL）への初回投入を含む。
    pub inserted: usize,
    /// 既存行の既読フラグを未読に戻した数（初回購読投入でのみ発生）。
    pub unread_changed: usize,
    /// `inserted` に数えた動画の video_id（shorts 判定など投入後処理の対象）。
    pub new_video_ids: Vec<String>,
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

impl Db {
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

    /// `list_feed`（設計書 §3.1）。ブロックチャンネルの動画は常に除外する（FR-5）。
    /// SQL の LIMIT は掛けず、述語 `keep` に適合した行だけを `limit` 件までスキャンする
    /// （NG フィルタで先頭が抜けても後続の適合行を拾える。FR-9）。
    /// `feed_list` 相当の無条件取得は `keep: |_| true` として呼ぶ。
    /// `show_shorts=false` のとき `kind='short'` の行を除外する（FR-23、仕様決定 AP。
    /// `short` 明示選択時に表示へ回す判定は呼び出し側が行う）。
    pub fn feed_list_filtered(
        &self,
        filter: &FeedFilter,
        show_shorts: bool,
        limit: usize,
        keep: impl Fn(&FeedItem) -> bool,
    ) -> Result<Vec<FeedItem>, DbError> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT v.video_id, v.channel_id, v.channel_title, v.title,
                    v.thumbnail_url, v.published_at, v.kind, v.is_read
             FROM videos v
             JOIN channels c ON c.channel_id = v.channel_id
             WHERE v.ingested = 1
               AND v.channel_id NOT IN (SELECT channel_id FROM blocked_channels)
               AND (?1 = 0 OR v.is_read = 0)
               AND (?2 IS NULL
                    OR (?2 = 0 AND c.category_id IS NULL)
                    OR c.category_id = ?2)
               AND (?3 IS NULL
                    OR datetime(v.published_at) >= datetime('now', '-' || ?3 || ' days'))
               AND (?4 IS NULL OR v.kind = ?4)
               AND (?5 = 1 OR v.kind <> 'short')
               AND (?8 IS NULL OR v.channel_id = ?8)
               AND (
                    ?7 IS NULL
                    OR (?6 IS NULL AND v.published_at IS NULL AND v.video_id > ?7)
                    OR (?6 IS NOT NULL AND (
                         v.published_at IS NULL
                         OR v.published_at < ?6
                         OR (v.published_at = ?6 AND v.video_id > ?7))))
             ORDER BY v.published_at DESC, v.video_id",
        )?;
        let cursor = filter.cursor.as_ref();
        let mut rows = stmt.query(rusqlite::params![
            filter.unread_only as i64,
            filter.category_id,
            filter.days.map(|d| d as i64),
            filter.kind,
            show_shorts as i64,
            cursor.and_then(|c| c.published_at.as_deref()),
            cursor.map(|c| c.video_id.as_str()),
            filter.channel_id,
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

    /// バックフィル投入（FR-21、仕様決定 AN）。RSS の `feed_ingest` とは別経路:
    /// 新規行は「既読・投入済み」で挿入し、`ingested=0` のプレースホルダ行は
    /// 既読のままメタを埋めて `ingested=1` に確定する。投入済みの既存行は
    /// 一切書き換えない（投稿日を含む。既読→未読の戻しも行わない）。
    /// 戻り値は `Some(一覧に新たに現れた件数)`。チャンネル行が無ければ `None`。
    pub fn feed_backfill_ingest(
        &self,
        channel_id: &str,
        entries: &[NewVideo<'_>],
    ) -> Result<Option<usize>, DbError> {
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
        let mut inserted = 0usize;
        for v in entries {
            inserted += tx.execute(
                "INSERT INTO videos
                   (video_id, channel_id, channel_title, title, thumbnail_url,
                    published_at, kind, is_read, ingested)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1, 1)
                 ON CONFLICT (video_id) DO UPDATE SET
                   channel_id = excluded.channel_id,
                   channel_title = COALESCE(excluded.channel_title, videos.channel_title),
                   title = CASE WHEN excluded.title <> '' THEN excluded.title
                                ELSE videos.title END,
                   thumbnail_url = COALESCE(excluded.thumbnail_url, videos.thumbnail_url),
                   published_at = COALESCE(excluded.published_at, videos.published_at),
                   kind = excluded.kind,
                   ingested = 1
                 WHERE videos.ingested = 0",
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
        tx.commit()?;
        Ok(Some(inserted))
    }

    /// バックフィルのタブ別取得済み位置 `(videos, streams)`。
    /// チャンネル行が無ければ `(0, 0)`。
    pub fn channel_backfill_positions(&self, channel_id: &str) -> Result<(i64, i64), DbError> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT backfill_videos_pos, backfill_streams_pos
               FROM channels WHERE channel_id = ?1",
        )?;
        let mut rows = stmt.query([channel_id])?;
        match rows.next()? {
            Some(row) => Ok((row.get(0)?, row.get(1)?)),
            None => Ok((0, 0)),
        }
    }

    /// バックフィルのタブ別取得済み位置の保存。
    pub fn channel_set_backfill_positions(
        &self,
        channel_id: &str,
        videos_pos: i64,
        streams_pos: i64,
    ) -> Result<(), DbError> {
        let conn = self.lock()?;
        conn.execute(
            "UPDATE channels SET backfill_videos_pos = ?2,
               backfill_streams_pos = ?3 WHERE channel_id = ?1",
            rusqlite::params![channel_id, videos_pos, streams_pos],
        )?;
        Ok(())
    }

    /// `videos.kind` の事後更新（shorts 判定結果の書き戻し、仕様決定 V）。
    /// 'video' の行だけを更新し、既に別種別へ判定済みの行は上書きしない。
    pub fn videos_set_kind(&self, video_ids: &[String], kind: &str) -> Result<(), DbError> {
        if video_ids.is_empty() {
            return Ok(());
        }
        let conn = self.lock()?;
        let mut stmt =
            conn.prepare("UPDATE videos SET kind = ?2 WHERE video_id = ?1 AND kind = 'video'")?;
        for id in video_ids {
            stmt.execute(rusqlite::params![id, kind])?;
        }
        Ok(())
    }

    /// 再生中チャンネル解決の第 1 段（FR-12、仕様決定 U）。
    /// `(channel_id, channel_title)` を返す。行なし・channel_id 空文字は
    /// 「未知」として None に揃え、呼び出し側が次段（watch_history）へ進む。
    pub fn video_channel(
        &self,
        video_id: &str,
    ) -> Result<(Option<String>, Option<String>), DbError> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT NULLIF(channel_id, ''), channel_title FROM videos WHERE video_id = ?1",
        )?;
        let mut rows = stmt.query([video_id])?;
        match rows.next()? {
            Some(row) => Ok((row.get(0)?, row.get(1)?)),
            None => Ok((None, None)),
        }
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
        // 新規行は is_read=0・ingested=1 で未読投入。既存行は通常はそのままだが、
        // フィード未確定の行（ingested=0 のプレースホルダ）には RSS 到達の
        // 時点で投稿日・種別を埋めて未読へ戻し ingested=1 に確定する。
        // ingested=1 の行は WHERE で除外して既読状態を保つ（既読→未読への
        // 戻しは初回購読時の reset_unread 経路だけが担う）。
        let n = tx.execute(
            "INSERT INTO videos
               (video_id, channel_id, channel_title, title, thumbnail_url,
                published_at, kind, is_read, ingested)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, 1)
             ON CONFLICT (video_id) DO UPDATE SET
               channel_id = excluded.channel_id,
               channel_title = COALESCE(excluded.channel_title, videos.channel_title),
               title = CASE WHEN excluded.title <> '' THEN excluded.title
                            ELSE videos.title END,
               thumbnail_url = COALESCE(excluded.thumbnail_url, videos.thumbnail_url),
               published_at = excluded.published_at,
               kind = excluded.kind,
               is_read = 0,
               ingested = 1
             WHERE videos.ingested = 0",
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
        out.inserted += n;
        if n > 0 {
            out.new_video_ids.push(v.video_id.to_string());
        }
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
