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
    /// 'video' | 'short' | 'live' | 'upcoming' | 'premiere'
    /// （videos.kind の CHECK 制約、仕様決定 CG）。
    pub kind: &'a str,
}

/// メタの遅延解決の対象行（FR-53・FR-56・FR-57・FR-85、仕様決定 CD・CG・CH・DJ）。
/// `meta_resolve_targets` が返す。needs_* は「その時点で DB が欠いている情報」で、
/// 解決側は必要な部分だけを埋めればよい（複数立つ行では 1 回の
/// InnerTube 応答で同時に埋まる）。
#[derive(Debug, Clone)]
pub struct MetaResolveTarget {
    pub video_id: String,
    /// 現在の kind（暫定値・推定値を含む）。'short' は確定済みとして
    /// 選出対象外なので実質 'video' / 'live' / 'upcoming' / 'premiere'。
    pub kind: String,
    /// published_at が NULL（InnerTube メタ応答の publishDate を書き込む余地あり）。
    pub needs_published: bool,
    /// kind_state が 0（未確定）または 2（失敗）（kind 判定を試す対象）。
    pub needs_kind: bool,
    /// description が NULL（InnerTube メタ応答の shortDescription を
    /// 書き込む余地あり。空文字は解決済み・説明なしで対象外）。
    pub needs_desc: bool,
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
                    v.thumbnail_url, v.published_at, v.kind, v.is_read,
                    v.description
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
               AND (?4 IS NULL
                    OR v.kind = ?4
                    OR (?4 = 'live' AND v.kind IN ('live','upcoming','premiere')))
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
                description: row.get(8)?,
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
                   kind = CASE WHEN videos.kind_state = 1 THEN videos.kind
                               ELSE excluded.kind END,
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

    /// バックフィルのタブ別取得済み位置 `(videos, streams, shorts)`
    /// （`/shorts` は FR-55・仕様決定 CF で追加）。チャンネル行が無ければ `(0, 0, 0)`。
    pub fn channel_backfill_positions(&self, channel_id: &str) -> Result<(i64, i64, i64), DbError> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT backfill_videos_pos, backfill_streams_pos, backfill_shorts_pos
               FROM channels WHERE channel_id = ?1",
        )?;
        let mut rows = stmt.query([channel_id])?;
        match rows.next()? {
            Some(row) => Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            None => Ok((0, 0, 0)),
        }
    }

    /// バックフィルのタブ別取得済み位置の保存。
    pub fn channel_set_backfill_positions(
        &self,
        channel_id: &str,
        videos_pos: i64,
        streams_pos: i64,
        shorts_pos: i64,
    ) -> Result<(), DbError> {
        let conn = self.lock()?;
        conn.execute(
            "UPDATE channels SET backfill_videos_pos = ?2,
               backfill_streams_pos = ?3, backfill_shorts_pos = ?4
             WHERE channel_id = ?1",
            rusqlite::params![channel_id, videos_pos, streams_pos, shorts_pos],
        )?;
        Ok(())
    }

    /// `videos.kind` の事後更新（shorts 判定結果の書き戻し、仕様決定 V）。
    /// 'video' の行だけを更新し、既に別種別へ判定済みの行は上書きしない。
    /// 書き込んだ行は kind 判定済み（kind_state=1）とする（仕様決定 CG・CH。
    /// 'short' への判定は確定扱いで遅延解決の再試行対象から外れる）。
    pub fn videos_set_kind(&self, video_ids: &[String], kind: &str) -> Result<(), DbError> {
        if video_ids.is_empty() {
            return Ok(());
        }
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "UPDATE videos SET kind = ?2, kind_state = 1
               WHERE video_id = ?1 AND kind = 'video'",
        )?;
        for id in video_ids {
            stmt.execute(rusqlite::params![id, kind])?;
        }
        Ok(())
    }

    /// メタの遅延解決の対象選出（FR-53・FR-56・FR-57・FR-85、仕様決定 CD・CG・CH・DJ）。
    /// 対象は次の和集合で、新しい行から `limit` 件まで取る:
    ///   - kind 未確定（kind_state=0）または前回失敗（kind_state=2）の行
    ///   - published_at が NULL で InnerTube メタ応答を未試行（meta_checked=0）の行
    ///   - description が NULL の行（説明文を未解決。FR-85、仕様決定 DJ。
    ///     空文字は解決済み・説明なしなので対象外）
    ///
    /// 'short' は shorts 判定で確定済み（kind_state=1）なので kind では
    /// ここに出ないが、投稿日時・説明文が欠けていればその旗だけで選ばれる。
    pub fn meta_resolve_targets(&self, limit: usize) -> Result<Vec<MetaResolveTarget>, DbError> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT video_id, kind,
                    (published_at IS NULL) AS needs_published,
                    (kind_state <> 1)      AS needs_kind,
                    (description IS NULL)  AS needs_desc
               FROM videos
              WHERE kind_state <> 1
                 OR (published_at IS NULL AND meta_checked = 0)
                 OR description IS NULL
              ORDER BY first_seen_at DESC, video_id
              LIMIT ?1",
        )?;
        let mut rows = stmt.query([limit as i64])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(MetaResolveTarget {
                video_id: row.get(0)?,
                kind: row.get(1)?,
                needs_published: row.get::<_, i64>(2)? != 0,
                needs_kind: row.get::<_, i64>(3)? != 0,
                needs_desc: row.get::<_, i64>(4)? != 0,
            });
        }
        Ok(out)
    }

    /// 投稿日時の遅延書き戻し（FR-53、仕様決定 CD）。published_at が NULL の
    /// 行だけを埋め、既存値は上書きしない（RSS・ローカル由来の値を守る）。
    /// 実際に書き込めたら true（UI 更新イベントの要否判定用）。
    pub fn videos_set_published_at(&self, video_id: &str, at: &str) -> Result<bool, DbError> {
        let conn = self.lock()?;
        Ok(conn.execute(
            "UPDATE videos SET published_at = ?2
               WHERE video_id = ?1 AND published_at IS NULL",
            rusqlite::params![video_id, at],
        )? > 0)
    }

    /// InnerTube メタ応答の試行済み印（FR-53、仕様決定 CD）。
    /// publishDate を持たない応答でも立て、投稿日時だけが欠けた行を
    /// 「もう一度問う」対象から外す（kind の再試行選出には影響しない）。
    pub fn videos_mark_meta_checked(&self, video_id: &str) -> Result<(), DbError> {
        let conn = self.lock()?;
        conn.execute(
            "UPDATE videos SET meta_checked = 1 WHERE video_id = ?1",
            [video_id],
        )?;
        Ok(())
    }

    /// 説明文の遅延書き戻し（FR-85、仕様決定 DJ）。
    /// InnerTube メタ応答の `videoDetails.shortDescription` を入れる。
    /// description が NULL の行だけを埋め、既存値は上書きしない
    /// （投稿日時と同じく解決済みの値を守る暫定解釈: 投稿後に説明文が
    /// 編集されても追従しない）。応答が説明文を持たない（パース結果が
    /// 空文字）場合も空文字を書き込み、説明なしを確定させて
    /// 「もう一度問う」対象から外す。実際に書き込めたら true。
    pub fn videos_set_description(&self, video_id: &str, desc: &str) -> Result<bool, DbError> {
        let conn = self.lock()?;
        Ok(conn.execute(
            "UPDATE videos SET description = ?2
               WHERE video_id = ?1 AND description IS NULL",
            rusqlite::params![video_id, desc],
        )? > 0)
    }

    /// kind の遅延確定の書き戻し（FR-56、仕様決定 CG）。
    /// `resolved_kind` には 'video' 以外の確定値（'live' / 'upcoming' /
    /// 'premiere' / 'short'）だけを渡す。'short' の行は shorts 判定が立てた
    /// 事実として kind を維持し、判定済み印だけ立てる。
    /// kind の値が変わったとき true を返す（UI 更新イベントの要否判定用）。
    pub fn videos_resolve_kind(
        &self,
        video_id: &str,
        resolved_kind: &str,
    ) -> Result<bool, DbError> {
        debug_assert!(resolved_kind != "video");
        let conn = self.lock()?;
        // 値が違う行は確定値へ置き換えて判定済みにする
        let changed = conn.execute(
            "UPDATE videos SET kind = ?2, kind_state = 1
               WHERE video_id = ?1 AND kind <> 'short' AND kind <> ?2",
            rusqlite::params![video_id, resolved_kind],
        )? > 0;
        // 'short' または同値だった行も「この video_id の判定は済んだ」ものとする
        conn.execute(
            "UPDATE videos SET kind_state = 1
               WHERE video_id = ?1 AND (kind = 'short' OR kind = ?2)",
            rusqlite::params![video_id, resolved_kind],
        )?;
        Ok(changed)
    }

    /// kind 判定状態の明示的な遷移（FR-56・FR-57、仕様決定 CG・CH）。
    /// 「放送系でないと確定（1）」「今回の試行が失敗（2）」の書き込みに使う。
    /// 既に確定済み（kind_state=1）の行は書き換えない（別経路の確定結果を守る）。
    /// 実際に状態が変わったら true。
    pub fn videos_set_kind_state(&self, video_id: &str, state: i64) -> Result<bool, DbError> {
        let conn = self.lock()?;
        Ok(conn.execute(
            "UPDATE videos SET kind_state = ?2
               WHERE video_id = ?1 AND kind_state <> 1",
            rusqlite::params![video_id, state],
        )? > 0)
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

    /// 自動既読（FR-64、仕様決定 CO）。未読（`is_read=0`）の投入済み項目を
    /// 設定に従って既読にし、既読化した件数を返す。
    /// - `played` が真: `watch_history` に行がある項目を既読にする
    ///   （「再生済み」= 再生履歴が残っている項目、とする暫定解釈。
    ///   `completed` の完視聴までは要求しない）
    /// - `days` が 1 以上: 投稿日時（`published_at`。NULL は `first_seen_at`
    ///   にフォールバックする暫定解釈）が N 日経過した項目を既読にする。
    ///   0 以下は無効
    ///
    /// 両ルールとも `ingested=0` のプレースホルダは対象外（一覧に出ないため）。
    pub fn feed_auto_read(&self, played: bool, days: i64) -> Result<u64, DbError> {
        let conn = self.lock()?;
        let mut total = 0u64;
        if played {
            total += conn.execute(
                "UPDATE videos SET is_read = 1
                 WHERE is_read = 0 AND ingested = 1
                   AND video_id IN (SELECT video_id FROM watch_history)",
                [],
            )? as u64;
        }
        if days > 0 {
            total += conn.execute(
                "UPDATE videos SET is_read = 1
                 WHERE is_read = 0 AND ingested = 1
                   AND datetime(COALESCE(published_at, first_seen_at))
                       <= datetime('now', '-' || ?1 || ' days')",
                [days],
            )? as u64;
        }
        Ok(total)
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
               kind = CASE WHEN videos.kind_state = 1 THEN videos.kind
                           ELSE excluded.kind END,
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
