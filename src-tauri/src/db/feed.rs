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

/// `videos` テーブルへの遅延解決一括書き込み引数（FR-53・FR-56・FR-85・FR-96）。
#[derive(Debug)]
pub struct ResolveMetaParams<'a> {
    pub video_id: &'a str,
    pub needs_published: bool,
    pub published_at: Option<&'a str>,
    pub needs_desc: bool,
    pub description: &'a str,
    pub needs_kind: bool,
    pub resolved_kind: Option<&'a str>,
    pub broadcast_state: Option<&'a str>,
    pub broadcast_start_ms: Option<i64>,
    pub now_ms: i64,
}

/// 定期再確認対象のアクティブ放送行（FR-96、仕様決定 EB）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BroadcastRefreshTarget {
    pub video_id: String,
    pub broadcast_state: String,
    pub broadcast_start_ms: Option<i64>,
    pub broadcast_checked_at_ms: Option<i64>,
}

/// 放送状態別シーク SQL の対象状態。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BroadcastQueryState {
    Live,
    Upcoming,
}

const MS_2_MIN: i64 = 120_000;
const MS_3_MIN: i64 = 180_000;
const MS_10_MIN: i64 = 600_000;
const MS_15_MIN: i64 = 900_000;
const MS_2_HOURS: i64 = 7_200_000;
const MS_12_HOURS: i64 = 12 * 3_600_000;
const MS_24_HOURS: i64 = 86_400_000;
const MS_6_HOURS: i64 = 6 * 3_600_000;
const MS_7_DAYS: i64 = 7 * 86_400_000;

/// 放送状態専用の部分索引 `idx_videos_feed_broadcast` を使う 4 分岐 SQL（FR-96）。
/// SQLite の部分索引一致をコンパイル時に確定させるため `v.broadcast_state` は
/// `'live'` または `'upcoming'` をリテラルで埋め込む。
///
/// バインド順は `feed_list_filtered_sql` と同一（?1..=?6、チャンネル指定時 ?7）。
pub(crate) fn feed_list_broadcast_sql(
    state: BroadcastQueryState,
    unread_only: bool,
    has_channel_id: bool,
) -> &'static str {
    match (state, unread_only, has_channel_id) {
        (BroadcastQueryState::Live, false, false) => {
            "SELECT v.video_id, v.channel_id, v.channel_title, v.title,
                    v.thumbnail_url, v.published_at, v.kind, v.is_read,
                    v.broadcast_state, v.broadcast_start_ms, v.description
             FROM videos v
             CROSS JOIN channels c ON c.channel_id = v.channel_id
             WHERE v.ingested = 1
               AND v.broadcast_state IN ('live', 'upcoming')
               AND v.broadcast_state = 'live'
               AND v.channel_id NOT IN (SELECT channel_id FROM blocked_channels)
               AND (?1 IS NULL
                    OR (?1 = 0 AND c.category_id IS NULL)
                    OR c.category_id = ?1)
               AND (?2 IS NULL
                    OR datetime(v.published_at) >= datetime('now', '-' || ?2 || ' days'))
               AND (?3 IS NULL
                    OR ?3 IN ('live_now', 'upcoming')
                    OR (?3 = 'live' AND v.kind IN ('live','upcoming','premiere'))
                    OR (?3 NOT IN ('live','live_now','upcoming') AND v.kind = ?3))
               AND (?4 = 1 OR ?3 = 'short' OR v.kind <> 'short')
               AND (
                    ?6 IS NULL
                    OR (?5 IS NULL AND v.published_at IS NULL AND v.video_id > ?6)
                    OR (?5 IS NOT NULL AND (
                         v.published_at IS NULL
                         OR v.published_at < ?5
                         OR (v.published_at = ?5 AND v.video_id > ?6))))
             ORDER BY v.published_at DESC, v.video_id"
        }
        (BroadcastQueryState::Live, true, false) => {
            "SELECT v.video_id, v.channel_id, v.channel_title, v.title,
                    v.thumbnail_url, v.published_at, v.kind, v.is_read,
                    v.broadcast_state, v.broadcast_start_ms, v.description
             FROM videos v
             CROSS JOIN channels c ON c.channel_id = v.channel_id
             WHERE v.ingested = 1
               AND v.broadcast_state IN ('live', 'upcoming')
               AND v.broadcast_state = 'live'
               AND +v.is_read = 0
               AND v.channel_id NOT IN (SELECT channel_id FROM blocked_channels)
               AND (?1 IS NULL
                    OR (?1 = 0 AND c.category_id IS NULL)
                    OR c.category_id = ?1)
               AND (?2 IS NULL
                    OR datetime(v.published_at) >= datetime('now', '-' || ?2 || ' days'))
               AND (?3 IS NULL
                    OR ?3 IN ('live_now', 'upcoming')
                    OR (?3 = 'live' AND v.kind IN ('live','upcoming','premiere'))
                    OR (?3 NOT IN ('live','live_now','upcoming') AND v.kind = ?3))
               AND (?4 = 1 OR ?3 = 'short' OR v.kind <> 'short')
               AND (
                    ?6 IS NULL
                    OR (?5 IS NULL AND v.published_at IS NULL AND v.video_id > ?6)
                    OR (?5 IS NOT NULL AND (
                         v.published_at IS NULL
                         OR v.published_at < ?5
                         OR (v.published_at = ?5 AND v.video_id > ?6))))
             ORDER BY v.published_at DESC, v.video_id"
        }
        (BroadcastQueryState::Live, false, true) => {
            "SELECT v.video_id, v.channel_id, v.channel_title, v.title,
                    v.thumbnail_url, v.published_at, v.kind, v.is_read,
                    v.broadcast_state, v.broadcast_start_ms, v.description
             FROM videos v
             CROSS JOIN channels c ON c.channel_id = v.channel_id
             WHERE v.channel_id = ?7
               AND c.channel_id = ?7
               AND v.ingested = 1
               AND v.broadcast_state IN ('live', 'upcoming')
               AND v.broadcast_state = 'live'
               AND v.channel_id NOT IN (SELECT channel_id FROM blocked_channels)
               AND (?1 IS NULL
                    OR (?1 = 0 AND c.category_id IS NULL)
                    OR c.category_id = ?1)
               AND (?2 IS NULL
                    OR datetime(v.published_at) >= datetime('now', '-' || ?2 || ' days'))
               AND (?3 IS NULL
                    OR ?3 IN ('live_now', 'upcoming')
                    OR (?3 = 'live' AND v.kind IN ('live','upcoming','premiere'))
                    OR (?3 NOT IN ('live','live_now','upcoming') AND v.kind = ?3))
               AND (?4 = 1 OR ?3 = 'short' OR v.kind <> 'short')
               AND (
                    ?6 IS NULL
                    OR (?5 IS NULL AND v.published_at IS NULL AND v.video_id > ?6)
                    OR (?5 IS NOT NULL AND (
                         v.published_at IS NULL
                         OR v.published_at < ?5
                         OR (v.published_at = ?5 AND v.video_id > ?6))))
             ORDER BY v.published_at DESC, v.video_id"
        }
        (BroadcastQueryState::Live, true, true) => {
            "SELECT v.video_id, v.channel_id, v.channel_title, v.title,
                    v.thumbnail_url, v.published_at, v.kind, v.is_read,
                    v.broadcast_state, v.broadcast_start_ms, v.description
             FROM videos v
             CROSS JOIN channels c ON c.channel_id = v.channel_id
             WHERE v.channel_id = ?7
               AND c.channel_id = ?7
               AND v.ingested = 1
               AND v.broadcast_state IN ('live', 'upcoming')
               AND v.broadcast_state = 'live'
               AND +v.is_read = 0
               AND v.channel_id NOT IN (SELECT channel_id FROM blocked_channels)
               AND (?1 IS NULL
                    OR (?1 = 0 AND c.category_id IS NULL)
                    OR c.category_id = ?1)
               AND (?2 IS NULL
                    OR datetime(v.published_at) >= datetime('now', '-' || ?2 || ' days'))
               AND (?3 IS NULL
                    OR ?3 IN ('live_now', 'upcoming')
                    OR (?3 = 'live' AND v.kind IN ('live','upcoming','premiere'))
                    OR (?3 NOT IN ('live','live_now','upcoming') AND v.kind = ?3))
               AND (?4 = 1 OR ?3 = 'short' OR v.kind <> 'short')
               AND (
                    ?6 IS NULL
                    OR (?5 IS NULL AND v.published_at IS NULL AND v.video_id > ?6)
                    OR (?5 IS NOT NULL AND (
                         v.published_at IS NULL
                         OR v.published_at < ?5
                         OR (v.published_at = ?5 AND v.video_id > ?6))))
             ORDER BY v.published_at DESC, v.video_id"
        }
        (BroadcastQueryState::Upcoming, false, false) => {
            "SELECT v.video_id, v.channel_id, v.channel_title, v.title,
                    v.thumbnail_url, v.published_at, v.kind, v.is_read,
                    v.broadcast_state, v.broadcast_start_ms, v.description
             FROM videos v
             CROSS JOIN channels c ON c.channel_id = v.channel_id
             WHERE v.ingested = 1
               AND v.broadcast_state IN ('live', 'upcoming')
               AND v.broadcast_state = 'upcoming'
               AND v.channel_id NOT IN (SELECT channel_id FROM blocked_channels)
               AND (?1 IS NULL
                    OR (?1 = 0 AND c.category_id IS NULL)
                    OR c.category_id = ?1)
               AND (?2 IS NULL
                    OR datetime(v.published_at) >= datetime('now', '-' || ?2 || ' days'))
               AND (?3 IS NULL
                    OR ?3 IN ('live_now', 'upcoming')
                    OR (?3 = 'live' AND v.kind IN ('live','upcoming','premiere'))
                    OR (?3 NOT IN ('live','live_now','upcoming') AND v.kind = ?3))
               AND (?4 = 1 OR ?3 = 'short' OR v.kind <> 'short')
               AND (
                    ?6 IS NULL
                    OR (?5 IS NULL AND v.published_at IS NULL AND v.video_id > ?6)
                    OR (?5 IS NOT NULL AND (
                         v.published_at IS NULL
                         OR v.published_at < ?5
                         OR (v.published_at = ?5 AND v.video_id > ?6))))
             ORDER BY v.published_at DESC, v.video_id"
        }
        (BroadcastQueryState::Upcoming, true, false) => {
            "SELECT v.video_id, v.channel_id, v.channel_title, v.title,
                    v.thumbnail_url, v.published_at, v.kind, v.is_read,
                    v.broadcast_state, v.broadcast_start_ms, v.description
             FROM videos v
             CROSS JOIN channels c ON c.channel_id = v.channel_id
             WHERE v.ingested = 1
               AND v.broadcast_state IN ('live', 'upcoming')
               AND v.broadcast_state = 'upcoming'
               AND +v.is_read = 0
               AND v.channel_id NOT IN (SELECT channel_id FROM blocked_channels)
               AND (?1 IS NULL
                    OR (?1 = 0 AND c.category_id IS NULL)
                    OR c.category_id = ?1)
               AND (?2 IS NULL
                    OR datetime(v.published_at) >= datetime('now', '-' || ?2 || ' days'))
               AND (?3 IS NULL
                    OR ?3 IN ('live_now', 'upcoming')
                    OR (?3 = 'live' AND v.kind IN ('live','upcoming','premiere'))
                    OR (?3 NOT IN ('live','live_now','upcoming') AND v.kind = ?3))
               AND (?4 = 1 OR ?3 = 'short' OR v.kind <> 'short')
               AND (
                    ?6 IS NULL
                    OR (?5 IS NULL AND v.published_at IS NULL AND v.video_id > ?6)
                    OR (?5 IS NOT NULL AND (
                         v.published_at IS NULL
                         OR v.published_at < ?5
                         OR (v.published_at = ?5 AND v.video_id > ?6))))
             ORDER BY v.published_at DESC, v.video_id"
        }
        (BroadcastQueryState::Upcoming, false, true) => {
            "SELECT v.video_id, v.channel_id, v.channel_title, v.title,
                    v.thumbnail_url, v.published_at, v.kind, v.is_read,
                    v.broadcast_state, v.broadcast_start_ms, v.description
             FROM videos v
             CROSS JOIN channels c ON c.channel_id = v.channel_id
             WHERE v.channel_id = ?7
               AND c.channel_id = ?7
               AND v.ingested = 1
               AND v.broadcast_state IN ('live', 'upcoming')
               AND v.broadcast_state = 'upcoming'
               AND v.channel_id NOT IN (SELECT channel_id FROM blocked_channels)
               AND (?1 IS NULL
                    OR (?1 = 0 AND c.category_id IS NULL)
                    OR c.category_id = ?1)
               AND (?2 IS NULL
                    OR datetime(v.published_at) >= datetime('now', '-' || ?2 || ' days'))
               AND (?3 IS NULL
                    OR ?3 IN ('live_now', 'upcoming')
                    OR (?3 = 'live' AND v.kind IN ('live','upcoming','premiere'))
                    OR (?3 NOT IN ('live','live_now','upcoming') AND v.kind = ?3))
               AND (?4 = 1 OR ?3 = 'short' OR v.kind <> 'short')
               AND (
                    ?6 IS NULL
                    OR (?5 IS NULL AND v.published_at IS NULL AND v.video_id > ?6)
                    OR (?5 IS NOT NULL AND (
                         v.published_at IS NULL
                         OR v.published_at < ?5
                         OR (v.published_at = ?5 AND v.video_id > ?6))))
             ORDER BY v.published_at DESC, v.video_id"
        }
        (BroadcastQueryState::Upcoming, true, true) => {
            "SELECT v.video_id, v.channel_id, v.channel_title, v.title,
                    v.thumbnail_url, v.published_at, v.kind, v.is_read,
                    v.broadcast_state, v.broadcast_start_ms, v.description
             FROM videos v
             CROSS JOIN channels c ON c.channel_id = v.channel_id
             WHERE v.channel_id = ?7
               AND c.channel_id = ?7
               AND v.ingested = 1
               AND v.broadcast_state IN ('live', 'upcoming')
               AND v.broadcast_state = 'upcoming'
               AND +v.is_read = 0
               AND v.channel_id NOT IN (SELECT channel_id FROM blocked_channels)
               AND (?1 IS NULL
                    OR (?1 = 0 AND c.category_id IS NULL)
                    OR c.category_id = ?1)
               AND (?2 IS NULL
                    OR datetime(v.published_at) >= datetime('now', '-' || ?2 || ' days'))
               AND (?3 IS NULL
                    OR ?3 IN ('live_now', 'upcoming')
                    OR (?3 = 'live' AND v.kind IN ('live','upcoming','premiere'))
                    OR (?3 NOT IN ('live','live_now','upcoming') AND v.kind = ?3))
               AND (?4 = 1 OR ?3 = 'short' OR v.kind <> 'short')
               AND (
                    ?6 IS NULL
                    OR (?5 IS NULL AND v.published_at IS NULL AND v.video_id > ?6)
                    OR (?5 IS NOT NULL AND (
                         v.published_at IS NULL
                         OR v.published_at < ?5
                         OR (v.published_at = ?5 AND v.video_id > ?6))))
             ORDER BY v.published_at DESC, v.video_id"
        }
    }
}

/// `feed_list_filtered` の `(unread_only, has_channel_id)` 4 分岐 SQL を返す（F-08）。
/// 全 4 分岐で `ORDER BY v.published_at DESC, v.video_id` を維持しつつ、
/// v24 索引（`idx_videos_feed_pub` / `idx_videos_unread` / `idx_videos_channel_pub`）を
/// 直接走査させて `SCAN v` 素走査と `USE TEMP B-TREE` を排除する。
/// フェーズ 2（非ライブ中走査）として `broadcast_state <> 'live'`（NULL 含む）のみ返す。
///
/// バインド順:
/// - `?1`: `category_id`
/// - `?2`: `days`
/// - `?3`: `kind`
/// - `?4`: `show_shorts` (`0` / `1`)
/// - `?5`: `cursor.published_at`
/// - `?6`: `cursor.video_id`
/// - `?7`: `channel_id`（`has_channel_id == true` のときのみ）
pub(crate) fn feed_list_filtered_sql(unread_only: bool, has_channel_id: bool) -> &'static str {
    match (unread_only, has_channel_id) {
        (false, false) => {
            "SELECT v.video_id, v.channel_id, v.channel_title, v.title,
                    v.thumbnail_url, v.published_at, v.kind, v.is_read,
                    v.broadcast_state, v.broadcast_start_ms, v.description
             FROM videos v
             CROSS JOIN channels c ON c.channel_id = v.channel_id
             WHERE v.ingested = 1
               AND (v.broadcast_state IS NULL OR v.broadcast_state <> 'live')
               AND v.channel_id NOT IN (SELECT channel_id FROM blocked_channels)
               AND (?1 IS NULL
                    OR (?1 = 0 AND c.category_id IS NULL)
                    OR c.category_id = ?1)
               AND (?2 IS NULL
                    OR datetime(v.published_at) >= datetime('now', '-' || ?2 || ' days'))
               AND (?3 IS NULL
                    OR v.kind = ?3
                    OR (?3 = 'live' AND v.kind IN ('live','upcoming','premiere')))
               AND (?4 = 1 OR ?3 = 'short' OR v.kind <> 'short')
               AND (
                    ?6 IS NULL
                    OR (?5 IS NULL AND v.published_at IS NULL AND v.video_id > ?6)
                    OR (?5 IS NOT NULL AND (
                         v.published_at IS NULL
                         OR v.published_at < ?5
                         OR (v.published_at = ?5 AND v.video_id > ?6))))
             ORDER BY v.published_at DESC, v.video_id"
        }
        (true, false) => {
            "SELECT v.video_id, v.channel_id, v.channel_title, v.title,
                    v.thumbnail_url, v.published_at, v.kind, v.is_read,
                    v.broadcast_state, v.broadcast_start_ms, v.description
             FROM videos v
             CROSS JOIN channels c ON c.channel_id = v.channel_id
             WHERE v.ingested = 1
               AND (v.broadcast_state IS NULL OR v.broadcast_state <> 'live')
               AND v.is_read = 0
               AND v.channel_id NOT IN (SELECT channel_id FROM blocked_channels)
               AND (?1 IS NULL
                    OR (?1 = 0 AND c.category_id IS NULL)
                    OR c.category_id = ?1)
               AND (?2 IS NULL
                    OR datetime(v.published_at) >= datetime('now', '-' || ?2 || ' days'))
               AND (?3 IS NULL
                    OR v.kind = ?3
                    OR (?3 = 'live' AND v.kind IN ('live','upcoming','premiere')))
               AND (?4 = 1 OR ?3 = 'short' OR v.kind <> 'short')
               AND (
                    ?6 IS NULL
                    OR (?5 IS NULL AND v.published_at IS NULL AND v.video_id > ?6)
                    OR (?5 IS NOT NULL AND (
                         v.published_at IS NULL
                         OR v.published_at < ?5
                         OR (v.published_at = ?5 AND v.video_id > ?6))))
             ORDER BY v.published_at DESC, v.video_id"
        }
        (false, true) => {
            "SELECT v.video_id, v.channel_id, v.channel_title, v.title,
                    v.thumbnail_url, v.published_at, v.kind, v.is_read,
                    v.broadcast_state, v.broadcast_start_ms, v.description
             FROM videos v
             CROSS JOIN channels c ON c.channel_id = v.channel_id
             WHERE v.channel_id = ?7
               AND c.channel_id = ?7
               AND v.ingested = 1
               AND (v.broadcast_state IS NULL OR v.broadcast_state <> 'live')
               AND v.channel_id NOT IN (SELECT channel_id FROM blocked_channels)
               AND (?1 IS NULL
                    OR (?1 = 0 AND c.category_id IS NULL)
                    OR c.category_id = ?1)
               AND (?2 IS NULL
                    OR datetime(v.published_at) >= datetime('now', '-' || ?2 || ' days'))
               AND (?3 IS NULL
                    OR v.kind = ?3
                    OR (?3 = 'live' AND v.kind IN ('live','upcoming','premiere')))
               AND (?4 = 1 OR ?3 = 'short' OR v.kind <> 'short')
               AND (
                    ?6 IS NULL
                    OR (?5 IS NULL AND v.published_at IS NULL AND v.video_id > ?6)
                    OR (?5 IS NOT NULL AND (
                         v.published_at IS NULL
                         OR v.published_at < ?5
                         OR (v.published_at = ?5 AND v.video_id > ?6))))
             ORDER BY v.published_at DESC, v.video_id"
        }
        (true, true) => {
            // `+v.is_read = 0` の単項 `+` で部分索引 `idx_videos_unread` より
            // `idx_videos_channel_pub (channel_id=?)` を確定選択させ、
            // 他チャンネルの未読全件走査と `ANALYZE` 後の `SCAN c` 起因ソートを同時に防ぐ。
            "SELECT v.video_id, v.channel_id, v.channel_title, v.title,
                    v.thumbnail_url, v.published_at, v.kind, v.is_read,
                    v.broadcast_state, v.broadcast_start_ms, v.description
             FROM videos v
             CROSS JOIN channels c ON c.channel_id = v.channel_id
             WHERE v.channel_id = ?7
               AND c.channel_id = ?7
               AND v.ingested = 1
               AND (v.broadcast_state IS NULL OR v.broadcast_state <> 'live')
               AND +v.is_read = 0
               AND v.channel_id NOT IN (SELECT channel_id FROM blocked_channels)
               AND (?1 IS NULL
                    OR (?1 = 0 AND c.category_id IS NULL)
                    OR c.category_id = ?1)
               AND (?2 IS NULL
                    OR datetime(v.published_at) >= datetime('now', '-' || ?2 || ' days'))
               AND (?3 IS NULL
                    OR v.kind = ?3
                    OR (?3 = 'live' AND v.kind IN ('live','upcoming','premiere')))
               AND (?4 = 1 OR ?3 = 'short' OR v.kind <> 'short')
               AND (
                    ?6 IS NULL
                    OR (?5 IS NULL AND v.published_at IS NULL AND v.video_id > ?6)
                    OR (?5 IS NOT NULL AND (
                         v.published_at IS NULL
                         OR v.published_at < ?5
                         OR (v.published_at = ?5 AND v.video_id > ?6))))
             ORDER BY v.published_at DESC, v.video_id"
        }
    }
}

struct FeedQuery<'a> {
    sql: &'a str,
    filter: &'a FeedFilter,
    cursor: Option<&'a crate::model::FeedCursor>,
    show_shorts: bool,
    limit: usize,
}

fn collect_feed_rows(
    conn: &rusqlite::Connection,
    q: &FeedQuery<'_>,
    keep: &impl Fn(&FeedItem) -> bool,
    out: &mut Vec<FeedItem>,
) -> Result<(), DbError> {
    if out.len() >= q.limit {
        return Ok(());
    }
    let mut stmt = conn.prepare(q.sql)?;
    let mut rows = match q.filter.channel_id.as_deref() {
        Some(channel_id) => stmt.query(rusqlite::params![
            q.filter.category_id,
            q.filter.days.map(|d| d as i64),
            q.filter.kind,
            q.show_shorts as i64,
            q.cursor.and_then(|c| c.published_at.as_deref()),
            q.cursor.map(|c| c.video_id.as_str()),
            channel_id,
        ])?,
        None => stmt.query(rusqlite::params![
            q.filter.category_id,
            q.filter.days.map(|d| d as i64),
            q.filter.kind,
            q.show_shorts as i64,
            q.cursor.and_then(|c| c.published_at.as_deref()),
            q.cursor.map(|c| c.video_id.as_str()),
        ])?,
    };
    while out.len() < q.limit {
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
            broadcast_state: row.get(8)?,
            broadcast_start_ms: row.get(9)?,
            description: row.get(10)?,
        };
        if keep(&item) {
            out.push(item);
        }
    }
    Ok(())
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
        let mut out = Vec::new();
        let kind = filter.kind.as_deref();
        let cursor = filter.cursor.as_ref();

        // フェーズ 1（ライブ中走査）：全表示 / ライブ傘 / ライブ中 / Shorts 選択時、
        // かつ先頭ページまたは前ページ末尾がライブ中だった場合のみ実行する。
        let run_phase1 = matches!(kind, None | Some("live") | Some("live_now") | Some("short"))
            && cursor.is_none_or(|c| c.is_live);
        if run_phase1 {
            let sql = feed_list_broadcast_sql(
                BroadcastQueryState::Live,
                filter.unread_only,
                filter.channel_id.is_some(),
            );
            collect_feed_rows(
                &conn,
                &FeedQuery {
                    sql,
                    filter,
                    cursor,
                    show_shorts,
                    limit,
                },
                &keep,
                &mut out,
            )?;
        }

        // フェーズ 2（非ライブ中走査）：まだ上限に達しておらず、かつ live_now 限定でない場合に実行する。
        // 前ページ末尾がライブ中（c.is_live == true）だった場合は非ライブ側の先頭から走査する。
        if out.len() < limit && kind != Some("live_now") {
            let phase2_cursor = cursor.filter(|c| !c.is_live);
            let sql = if kind == Some("upcoming") {
                feed_list_broadcast_sql(
                    BroadcastQueryState::Upcoming,
                    filter.unread_only,
                    filter.channel_id.is_some(),
                )
            } else {
                feed_list_filtered_sql(filter.unread_only, filter.channel_id.is_some())
            };
            collect_feed_rows(
                &conn,
                &FeedQuery {
                    sql,
                    filter,
                    cursor: phase2_cursor,
                    show_shorts,
                    limit,
                },
                &keep,
                &mut out,
            )?;
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
        let mut conn = self.lock()?;
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare(
                "UPDATE videos SET kind = ?2, kind_state = 1
                   WHERE video_id = ?1 AND kind = 'video'",
            )?;
            for id in video_ids {
                stmt.execute(rusqlite::params![id, kind])?;
            }
        }
        tx.commit()?;
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
    #[cfg(test)]
    pub fn videos_set_published_at(&self, video_id: &str, at: &str) -> Result<bool, DbError> {
        let conn = self.lock()?;
        Ok(conn.execute(
            "UPDATE videos SET published_at = ?2
               WHERE video_id = ?1 AND published_at IS NULL",
            rusqlite::params![video_id, at],
        )? > 0)
    }

    #[cfg(test)]
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
        let mut conn = self.lock()?;
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare("UPDATE videos SET is_read = 1 WHERE video_id = ?1")?;
            for id in video_ids {
                stmt.execute([id])?;
            }
        }
        tx.commit()?;
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
        if !played && days <= 0 {
            return Ok(0);
        }
        let mut conn = self.lock()?;
        let tx = conn.transaction()?;
        let mut total = 0u64;
        if played {
            total += tx.execute(
                "UPDATE videos SET is_read = 1
                 WHERE is_read = 0 AND ingested = 1
                   AND video_id IN (SELECT video_id FROM watch_history)",
                [],
            )? as u64;
        }
        if days > 0 {
            total += tx.execute(
                "UPDATE videos SET is_read = 1
                 WHERE is_read = 0 AND ingested = 1
                   AND datetime(COALESCE(published_at, first_seen_at))
                       <= datetime('now', '-' || ?1 || ' days')",
                [days],
            )? as u64;
        }
        tx.commit()?;
        Ok(total)
    }

    /// 遅延解決（FR-53・FR-56・FR-85・FR-96）の結果を単一トランザクションで書き戻す。
    /// `published_at` / `description` / `kind`（既存 `'short'` 保護）/ `broadcast_*`（単調時刻ガード）を更新し、
    /// 一覧表示に影響する変化があった場合に `true` を返す。
    pub fn videos_resolve_meta_tx(&self, p: &ResolveMetaParams<'_>) -> Result<bool, DbError> {
        let mut conn = self.lock()?;
        let tx = conn.transaction()?;
        let mut changed = false;

        if p.needs_published {
            if let Some(at) = p.published_at {
                if tx.execute(
                    "UPDATE videos SET published_at = ?2
                       WHERE video_id = ?1 AND published_at IS NULL",
                    rusqlite::params![p.video_id, at],
                )? > 0
                {
                    changed = true;
                }
            }
        }
        tx.execute(
            "UPDATE videos SET meta_checked = 1 WHERE video_id = ?1",
            [p.video_id],
        )?;
        if p.needs_desc
            && tx.execute(
                "UPDATE videos SET description = ?2
                   WHERE video_id = ?1 AND description IS NULL",
                rusqlite::params![p.video_id, p.description],
            )? > 0
        {
            changed = true;
        }
        if p.needs_kind {
            if let Some(kind) = p.resolved_kind {
                debug_assert!(kind != "video");
                if tx.execute(
                    "UPDATE videos SET kind = ?2, kind_state = 1
                       WHERE video_id = ?1 AND kind <> 'short' AND kind <> ?2",
                    rusqlite::params![p.video_id, kind],
                )? > 0
                {
                    changed = true;
                }
                tx.execute(
                    "UPDATE videos SET kind_state = 1
                       WHERE video_id = ?1 AND (kind = 'short' OR kind = ?2)",
                    rusqlite::params![p.video_id, kind],
                )?;
            } else {
                // 通常動画候補（kind='video'）はトランザクション外の shorts_probe で確定するため、
                // それ以外の由来 kind だけをここで確定印にする。
                tx.execute(
                    "UPDATE videos SET kind_state = 1
                       WHERE video_id = ?1 AND kind <> 'video' AND kind_state <> 1",
                    [p.video_id],
                )?;
            }
        }

        if apply_broadcast_columns_tx(
            &tx,
            p.video_id,
            None,
            p.broadcast_state,
            p.broadcast_start_ms,
            p.now_ms,
            false,
        )? {
            changed = true;
        }

        tx.commit()?;
        Ok(changed)
    }

    /// 7 日超過した未開始の配信予定（`ingested = 1`, `broadcast_state = 'upcoming'`,
    /// かつ最低 1 回確認済み `broadcast_checked_at_ms > 0`）を一括で解除する（FR-96、仕様決定 EB）。
    pub fn videos_expire_stale_upcoming(&self, now_ms: i64) -> Result<usize, DbError> {
        let conn = self.lock()?;
        let n = conn.execute(
            "UPDATE videos
                SET broadcast_state = NULL,
                    broadcast_start_ms = NULL,
                    broadcast_checked_at_ms = ?1
              WHERE ingested = 1
                AND broadcast_state = 'upcoming'
                AND COALESCE(broadcast_checked_at_ms, 0) > 0
                AND ?1 > (
                    CASE WHEN broadcast_start_ms > 0 THEN broadcast_start_ms
                         ELSE COALESCE(CAST(strftime('%s', first_seen_at) AS INTEGER) * 1000, broadcast_checked_at_ms)
                    END
                ) + ?2",
            rusqlite::params![now_ms, MS_7_DAYS],
        )?;
        Ok(n)
    }

    /// 定期再確認対象のアクティブ放送行を選出する（FR-96、仕様決定 EB）。
    /// `ingested = 1` かつ `broadcast_state IN ('live', 'upcoming')` の行から、
    /// 緊急度別の再確認期限を過ぎたものを階層 1（最大 8 枠）・階層 2（最低 2 枠、相互融通）で最大 `limit` 件返す。
    pub fn broadcast_refresh_targets(
        &self,
        now_ms: i64,
        force_manual: bool,
        limit: usize,
    ) -> Result<Vec<BroadcastRefreshTarget>, DbError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT video_id, broadcast_state, broadcast_start_ms, broadcast_checked_at_ms,
                    CAST(strftime('%s', first_seen_at) AS INTEGER) * 1000 AS first_seen_ms
               FROM videos
              WHERE ingested = 1
                AND broadcast_state IN ('live', 'upcoming')",
        )?;
        let mut rows = stmt.query([])?;
        let mut tier1: Vec<BroadcastRefreshTarget> = Vec::new();
        let mut tier2: Vec<BroadcastRefreshTarget> = Vec::new();

        while let Some(row) = rows.next()? {
            let video_id: String = row.get(0)?;
            let broadcast_state: String = row.get(1)?;
            let raw_start_ms: Option<i64> = row.get(2)?;
            let broadcast_start_ms = raw_start_ms.filter(|&m| m > 0);
            let broadcast_checked_at_ms: Option<i64> = row.get(3)?;
            let first_seen_ms: Option<i64> = row.get(4)?;
            let checked_ms = broadcast_checked_at_ms.unwrap_or(0);
            let ref_ms = broadcast_start_ms.or(first_seen_ms).unwrap_or(checked_ms);

            let (is_tier1, interval_ms) = if broadcast_state == "live" {
                let long_live = broadcast_start_ms.is_some_and(|s| now_ms - s > MS_12_HOURS);
                (true, if long_live { MS_10_MIN } else { MS_3_MIN })
            } else if broadcast_start_ms
                .is_some_and(|s| now_ms >= s - MS_15_MIN && now_ms <= s + MS_2_HOURS)
            {
                (true, MS_2_MIN)
            } else {
                let over_24h = now_ms > ref_ms + MS_24_HOURS;
                (false, if over_24h { MS_6_HOURS } else { MS_15_MIN })
            };

            if !force_manual && checked_ms > now_ms - interval_ms {
                continue;
            }
            let item = BroadcastRefreshTarget {
                video_id,
                broadcast_state,
                broadcast_start_ms,
                broadcast_checked_at_ms,
            };
            if is_tier1 {
                tier1.push(item);
            } else {
                tier2.push(item);
            }
        }

        let sort_key = |a: &BroadcastRefreshTarget, b: &BroadcastRefreshTarget| {
            a.broadcast_checked_at_ms
                .unwrap_or(0)
                .cmp(&b.broadcast_checked_at_ms.unwrap_or(0))
                .then_with(|| a.video_id.cmp(&b.video_id))
        };
        tier1.sort_by(sort_key);
        tier2.sort_by(sort_key);

        let tier2_min = if limit >= 2 { (limit / 5).max(2) } else { 0 };
        let tier1_cap = limit.saturating_sub(tier2.len().min(tier2_min));
        let mut out: Vec<BroadcastRefreshTarget> = tier1.into_iter().take(tier1_cap).collect();
        let rem = limit.saturating_sub(out.len());
        out.extend(tier2.into_iter().take(rem));
        Ok(out)
    }

    /// 定期再確認およびライブ待機相乗りから `videos` の放送状態・開始時刻・`kind` を更新する。
    /// 単調時刻ガード（`COALESCE(broadcast_checked_at_ms, 0) <= now_ms`）と
    /// `ingested_only`（未投入行の抑止）を単一トランザクションで保証する。
    pub fn videos_apply_broadcast_refresh(
        &self,
        video_id: &str,
        new_kind: Option<&str>,
        broadcast_state: Option<&str>,
        broadcast_start_ms: Option<i64>,
        now_ms: i64,
        ingested_only: bool,
    ) -> Result<bool, DbError> {
        let mut conn = self.lock()?;
        let tx = conn.transaction()?;
        let changed = apply_broadcast_columns_tx(
            &tx,
            video_id,
            new_kind,
            broadcast_state,
            broadcast_start_ms,
            now_ms,
            ingested_only,
        )?;
        tx.commit()?;
        Ok(changed)
    }

    /// 通信失敗・ボット応答保護時に直前状態を維持したまま確認時刻のみ進める（FR-96）。
    pub fn videos_touch_broadcast_checked(
        &self,
        video_id: &str,
        now_ms: i64,
    ) -> Result<(), DbError> {
        let conn = self.lock()?;
        conn.execute(
            "UPDATE videos SET broadcast_checked_at_ms = ?2
               WHERE video_id = ?1 AND COALESCE(broadcast_checked_at_ms, 0) <= ?2",
            rusqlite::params![video_id, now_ms],
        )?;
        Ok(())
    }
}

/// トランザクション内で `broadcast_state` / `broadcast_start_ms` / `broadcast_checked_at_ms`
/// （および必要時は `kind`）を更新する共通ヘルパ。表示上の変化があれば `true` を返す。
fn apply_broadcast_columns_tx(
    tx: &rusqlite::Transaction<'_>,
    video_id: &str,
    new_kind: Option<&str>,
    broadcast_state: Option<&str>,
    broadcast_start_ms: Option<i64>,
    now_ms: i64,
    ingested_only: bool,
) -> Result<bool, DbError> {
    let mut stmt = tx.prepare(
        "SELECT kind, ingested, broadcast_state, broadcast_start_ms, broadcast_checked_at_ms
           FROM videos WHERE video_id = ?1",
    )?;
    let mut rows = stmt.query([video_id])?;
    let Some(row) = rows.next()? else {
        return Ok(false);
    };
    let cur_kind: String = row.get(0)?;
    let ingested: i64 = row.get(1)?;
    let cur_state: Option<String> = row.get(2)?;
    let cur_start: Option<i64> = row.get(3)?;
    let cur_checked: Option<i64> = row.get(4)?;
    drop(rows);
    drop(stmt);

    if ingested_only && ingested == 0 {
        return Ok(false);
    }
    if cur_checked.unwrap_or(0) > now_ms {
        return Ok(false);
    }

    let norm_start = broadcast_start_ms.filter(|&m| m > 0);
    let next_start = if broadcast_state.is_none() {
        None
    } else {
        norm_start.or(cur_start.filter(|&m| m > 0))
    };

    let mut changed = cur_state.as_deref() != broadcast_state || cur_start != next_start;

    tx.execute(
        "UPDATE videos
            SET broadcast_state = ?2,
                broadcast_start_ms = ?3,
                broadcast_checked_at_ms = ?4
          WHERE video_id = ?1 AND COALESCE(broadcast_checked_at_ms, 0) <= ?4",
        rusqlite::params![video_id, broadcast_state, next_start, now_ms],
    )?;

    if let Some(k) = new_kind {
        if cur_kind != "short" && cur_kind != k {
            tx.execute(
                "UPDATE videos SET kind = ?2, kind_state = 1 WHERE video_id = ?1 AND kind <> 'short'",
                rusqlite::params![video_id, k],
            )?;
            changed = true;
        }
    }

    Ok(changed)
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
