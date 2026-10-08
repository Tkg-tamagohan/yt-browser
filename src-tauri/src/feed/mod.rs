//! チャンネル RSS ポーラー（設計書 §2 の `feed` モジュール、§3.2 の feed:// イベント）。
//!
//! YouTube のチャンネル RSS（`https://www.youtube.com/feeds/videos.xml?channel_id=`）
//! を購読チャンネルごとにポーリングし、新着動画を `videos` へ未読（is_read=0）で積む。
//! 更新効率化に ETag / Last-Modified の条件付き取得を使い、更新のないチャンネルは
//! 間隔を伸ばし、失敗が続くチャンネルはバックオフする（間隔の適応化）。
//! ポーリングやパースの失敗は再生を阻害しない（warn ログ＋`feed://status` に留める）。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter};
use thiserror::Error;
use tokio::sync::Notify;

use crate::db::{Db, PollTarget};
use crate::model::{FeedNewItems, FeedStatus};
use crate::util::lock;

/// チャンネル RSS のエンドポイント（`?channel_id=` を後置）。
const FEED_URL: &str = "https://www.youtube.com/feeds/videos.xml";
/// 1 回の取得の上限時間。
const HTTP_TIMEOUT: Duration = Duration::from_secs(15);
/// スケジューラの巡回間隔。各チャンネルの予定時刻を超えたものだけを取りに行く。
const TICK: Duration = Duration::from_secs(30);
/// 新着が見つかった（または初回取得した）チャンネルの基本間隔。
const BASE_INTERVAL: Duration = Duration::from_secs(15 * 60);
/// 新着なしで伸ばしていく間隔の上限。
const IDLE_MAX: Duration = Duration::from_secs(60 * 60);
/// 新着なしのたびの伸長率（×1.5）。
const IDLE_GROW_NUM: u32 = 3;
const IDLE_GROW_DEN: u32 = 2;
/// 連続失敗時の再試行間隔の底（5 分から倍々で FAIL_MAX まで）。
const FAIL_BASE: Duration = Duration::from_secs(5 * 60);
const FAIL_MAX: Duration = Duration::from_secs(60 * 60);
/// この回数以上の連続失敗で `feed://status` へ劣化を通知する。
const FAIL_NOTIFY_THRESHOLD: u32 = 2;

#[derive(Debug, Error)]
pub enum FeedError {
    #[error("フィード取得で HTTP エラー: {0}")]
    Http(#[from] reqwest::Error),
    #[error("フィード応答が異常: status {0}")]
    Status(u16),
    #[error("チャンネルが見つからないか RSS を提供していない")]
    NotFound,
    #[error("フィード XML の解析に失敗: {0}")]
    Parse(String),
    #[error("db: {0}")]
    Db(#[from] crate::db::DbError),
}

/// RSS の `<entry>` 1 件分。
#[derive(Debug, Clone)]
pub struct FeedEntry {
    pub video_id: String,
    pub title: String,
    pub published_at: Option<String>,
    pub thumbnail_url: Option<String>,
}

/// パース済みフィード（チャンネルメタ＋最新エントリ群）。
#[derive(Debug)]
pub struct ParsedFeed {
    pub channel_id: String,
    pub channel_title: String,
    pub entries: Vec<FeedEntry>,
}

/// YouTube チャンネル ID の正規化。チャンネル ID は常に `UC` + 22 文字だが、
/// フィード直下の `yt:channelId` が `UC` プレフィックスなしで返る応答を実測で確認しており
/// （そのまま `?channel_id=` に使うと 404 になる）、欠落していれば補う暫定仕様。
fn normalize_channel_id(id: &str) -> String {
    if id.starts_with("UC") {
        id.to_string()
    } else {
        format!("UC{id}")
    }
}

/// `media:thumbnail` の URL が想定ホストかを確認する。フィード由来の値を
/// そのまま `<img src>` に使うため、YouTube 系ホスト以外は採用しない暫定仕様
/// （WebView の CSP img-src とも一致させる）。
fn is_allowed_thumbnail(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    let host = rest.split('/').next().unwrap_or("");
    host == "ytimg.com"
        || host.ends_with(".ytimg.com")
        || host == "ggpht.com"
        || host.ends_with(".ggpht.com")
}

/// Atom XML をパースする。YouTube のチャンネルフィードは固定構造
/// （feed > yt:channelId, author > name, entry*）なので名前空間問わず
/// ローカル名で拾う。未知のエントリフィールドは無視する。
pub fn parse_atom(xml: &str) -> Result<ParsedFeed, FeedError> {
    let doc = roxmltree::Document::parse(xml).map_err(|e| FeedError::Parse(e.to_string()))?;
    let root = doc.root_element();
    if root.tag_name().name() != "feed" {
        return Err(FeedError::Parse("ルート要素が feed ではありません".into()));
    }
    let child_text = |node: &roxmltree::Node, name: &str| -> Option<String> {
        node.children()
            .find(|n| n.is_element() && n.tag_name().name() == name)
            .and_then(|n| n.text().map(|t| t.trim().to_string()))
            .filter(|t| !t.is_empty())
    };
    let root_channel_id = child_text(&root, "channelId");
    let mut entries = Vec::new();
    // entry 直下の yt:channelId は UC プレフィックス付きで安定しているため優先する
    let mut entry_channel_id: Option<String> = None;
    for e in root
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "entry")
    {
        let Some(video_id) = child_text(&e, "videoId") else {
            continue;
        };
        let Some(title) = child_text(&e, "title") else {
            continue;
        };
        if entry_channel_id.is_none() {
            entry_channel_id = child_text(&e, "channelId");
        }
        // media:group > media:thumbnail の url 属性（許可ホストのみ採用）
        let thumbnail_url = e
            .children()
            .find(|n| n.is_element() && n.tag_name().name() == "group")
            .and_then(|g| {
                g.children()
                    .find(|n| n.is_element() && n.tag_name().name() == "thumbnail")
                    .and_then(|t| t.attribute("url").map(|s| s.to_string()))
            })
            .filter(|u| is_allowed_thumbnail(u));
        entries.push(FeedEntry {
            video_id,
            title,
            published_at: child_text(&e, "published"),
            thumbnail_url,
        });
    }
    let channel_id = entry_channel_id
        .or(root_channel_id)
        .map(|id| normalize_channel_id(&id))
        .ok_or_else(|| FeedError::Parse("yt:channelId がありません".into()))?;
    // <author><name> はフィード直下のものを採る（entry 内の author は無視）
    let channel_title = root
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "author")
        .and_then(|a| child_text(&a, "name"))
        .unwrap_or_else(|| channel_id.clone());
    Ok(ParsedFeed {
        channel_id,
        channel_title,
        entries,
    })
}

/// 条件付き取得の結果。
pub enum FetchOutcome {
    /// 304。本文なし。
    NotModified,
    /// 200。パース済み本文と新しい条件付き取得メタ。
    Parsed {
        feed: ParsedFeed,
        etag: Option<String>,
        last_modified: Option<String>,
    },
}

/// 1 チャンネル分の RSS を条件付き取得してパースする。
pub async fn fetch_feed(
    client: &reqwest::Client,
    channel_id: &str,
    etag: Option<&str>,
    last_modified: Option<&str>,
) -> Result<FetchOutcome, FeedError> {
    let mut req = client
        .get(FEED_URL)
        .query(&[("channel_id", channel_id)])
        .timeout(HTTP_TIMEOUT);
    if let Some(v) = etag {
        req = req.header(reqwest::header::IF_NONE_MATCH, v);
    }
    if let Some(v) = last_modified {
        req = req.header(reqwest::header::IF_MODIFIED_SINCE, v);
    }
    let resp = req.send().await?;
    let status = resp.status();
    if status == reqwest::StatusCode::NOT_MODIFIED {
        return Ok(FetchOutcome::NotModified);
    }
    if status == reqwest::StatusCode::NOT_FOUND {
        return Err(FeedError::NotFound);
    }
    if !status.is_success() {
        return Err(FeedError::Status(status.as_u16()));
    }
    let headers = resp.headers();
    let etag = headers
        .get(reqwest::header::ETAG)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let last_modified = headers
        .get(reqwest::header::LAST_MODIFIED)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let body = resp.text().await?;
    Ok(FetchOutcome::Parsed {
        feed: parse_atom(&body)?,
        etag,
        last_modified,
    })
}

/// 間隔の適応化（実装計画 Phase 4）。現在間隔・連続失敗数・新着有無から次回間隔を決める。
/// - 失敗が続く間は 5 分から倍々にバックオフ（上限 60 分）
/// - 新着があれば基本間隔（15 分）に戻す
/// - 新着なしは 1.5 倍ずつ伸ばす（上限 60 分）
pub fn next_interval(current: Duration, consecutive_failures: u32, found_new: bool) -> Duration {
    if consecutive_failures > 0 {
        let shift = consecutive_failures.saturating_sub(1).min(4);
        return (FAIL_BASE * 2u32.saturating_pow(shift)).min(FAIL_MAX);
    }
    if found_new {
        return BASE_INTERVAL;
    }
    (current * IDLE_GROW_NUM / IDLE_GROW_DEN).min(IDLE_MAX)
}

/// チャンネルごとのポーリング状態（永続化するのは etag / last_modified のみで、
/// 間隔と失敗回数はプロセス内状態とする）。
struct ChannelSchedule {
    next_at: Instant,
    interval: Duration,
    consecutive_failures: u32,
    /// 劣化通知を送った状態か（復帰通知の重複抑止）。
    degraded: bool,
}

impl Default for ChannelSchedule {
    fn default() -> Self {
        Self {
            next_at: Instant::now(),
            interval: BASE_INTERVAL,
            consecutive_failures: 0,
            degraded: false,
        }
    }
}

/// 購読フィードのポーラー。`run` を起動時に spawn する。
pub struct FeedPoller {
    db: Db,
    app: AppHandle,
    /// 購読コマンドからの初回取得にも使い回す共有クライアント。
    pub client: reqwest::Client,
    /// shorts 判定用クライアント（仕様決定 V）。非 short は /watch へ 303 で
    /// 転送されるため、リダイレクトを追跡しないクライアントで先頭応答を見る。
    detect_client: reqwest::Client,
    sched: Mutex<HashMap<String, ChannelSchedule>>,
    /// subscribe 時に即座に巡回を起こすための通知。
    wake: Notify,
}

/// shorts 判定 1 件あたりの上限時間。
const SHORTS_TIMEOUT: Duration = Duration::from_secs(8);

impl FeedPoller {
    pub fn new(db: Db, app: AppHandle) -> Arc<Self> {
        Arc::new(Self {
            db,
            app,
            client: reqwest::Client::new(),
            detect_client: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .expect("リダイレクト非追跡クライアントの生成に失敗"),
            sched: Mutex::new(HashMap::new()),
            wake: Notify::new(),
        })
    }

    /// 新規投入アイテムの shorts 判定をバックグラウンドで起こす（仕様決定 V）。
    /// 購読の初回投入・ポーリング投入の両方から呼び、非同期で `videos.kind` を更新する。
    pub fn spawn_kind_detection(&self, video_ids: Vec<String>) {
        if video_ids.is_empty() {
            return;
        }
        let db = self.db.clone();
        let client = self.detect_client.clone();
        tauri::async_runtime::spawn(async move {
            detect_shorts(&client, &db, video_ids).await;
        });
    }

    /// 新規購読などでスケジュールを即時回したいときに呼ぶ。
    /// 次回予定は変えない（初回取得直後のターゲットはスケジュール未登録＝即時対象）。
    pub fn wake_now(&self) {
        self.wake.notify_one();
    }

    /// 手動更新: 対象（全チャンネルまたは指定 1 件）の次回予定を現在に戻し巡回を起こす。
    /// 失敗カウンタは保持する（手動更新でも連続失敗として数える）。
    pub fn force_refresh(&self, channel_id: Option<&str>) {
        let now = Instant::now();
        let mut sched = lock(&self.sched);
        match channel_id {
            Some(id) => {
                sched.entry(id.to_string()).or_default().next_at = now;
            }
            None => {
                for s in sched.values_mut() {
                    s.next_at = now;
                }
            }
        }
        drop(sched);
        self.wake.notify_one();
    }

    /// メインループ。TICK ごとに期限の来たチャンネルを順次ポーリングする。
    /// 購読ゼロの間は wake 通知 or TICK の sleep で待つ。
    pub async fn run(self: Arc<Self>) {
        loop {
            tokio::select! {
                _ = self.wake.notified() => {},
                _ = tokio::time::sleep(TICK) => {},
            }
            self.poll_due().await;
        }
    }

    /// 期限の来たチャンネルをすべて処理する。戻り値は新着件数の合計。
    async fn poll_due(&self) {
        let targets = match self.db.channel_poll_targets() {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!(error = %e, "購読一覧の読み込みに失敗");
                return;
            }
        };
        let now = Instant::now();
        let due: Vec<PollTarget> = {
            let sched = lock(&self.sched);
            targets
                .into_iter()
                .filter(|t| sched.get(&t.channel_id).is_none_or(|s| s.next_at <= now))
                .collect()
        };
        let mut new_total = 0usize;
        for t in due {
            // ポーリング中に購読解除される場合に備えて、処理直前に存在を確認する
            if self.db.channel_get(&t.channel_id).ok().flatten().is_none() {
                lock(&self.sched).remove(&t.channel_id);
                continue;
            }
            new_total += self.poll_one(&t).await;
        }
        if new_total > 0 {
            let payload = FeedNewItems { count: new_total };
            if let Err(e) = self.app.emit("feed://new_items", payload) {
                tracing::warn!(error = %e, "feed://new_items の送出に失敗");
            }
        }
    }

    /// 1 チャンネルのポーリング。戻り値は新規挿入件数。
    async fn poll_one(&self, target: &PollTarget) -> usize {
        let outcome = fetch_feed(
            &self.client,
            &target.channel_id,
            target.rss_etag.as_deref(),
            target.rss_last_modified.as_deref(),
        )
        .await;
        match outcome {
            Ok(FetchOutcome::NotModified) => {
                if let Err(e) = self.db.channel_mark_polled(&target.channel_id) {
                    tracing::warn!(error = %e, "last_polled_at の更新に失敗");
                }
                self.after_success(&target.channel_id, false);
                0
            }
            Ok(FetchOutcome::Parsed {
                feed,
                etag,
                last_modified,
            }) => {
                let items: Vec<crate::db::NewVideo> = feed
                    .entries
                    .iter()
                    .map(|entry| crate::db::NewVideo {
                        video_id: &entry.video_id,
                        channel_id: &feed.channel_id,
                        channel_title: &feed.channel_title,
                        title: &entry.title,
                        thumbnail_url: entry.thumbnail_url.as_deref(),
                        published_at: entry.published_at.as_deref(),
                        kind: "video",
                    })
                    .collect();
                // 存在確認〜挿入〜取得メタ更新を一トランザクションで行い、
                // ポーリング中の購読解除との競合を防ぐ
                match self.db.feed_ingest(
                    &target.channel_id,
                    &items,
                    etag.as_deref(),
                    last_modified.as_deref(),
                ) {
                    Ok(Some(out)) => {
                        self.after_success(&target.channel_id, out.inserted > 0);
                        self.spawn_kind_detection(out.new_video_ids);
                        out.inserted
                    }
                    // 応答到着までに購読解除された: 挿入せずスケジュールも除去
                    Ok(None) => {
                        lock(&self.sched).remove(&target.channel_id);
                        0
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "フィード投入に失敗");
                        self.after_failure(&target.channel_id, e.to_string());
                        0
                    }
                }
            }
            Err(e) => {
                tracing::warn!(
                    channel_id = %target.channel_id,
                    channel = %target.title,
                    error = %e,
                    "チャンネル RSS の取得に失敗"
                );
                self.after_failure(&target.channel_id, e.to_string());
                0
            }
        }
    }

    /// 成功後のスケジュール更新と復帰通知。
    fn after_success(&self, channel_id: &str, found_new: bool) {
        let mut sched = lock(&self.sched);
        let s = sched.entry(channel_id.to_string()).or_default();
        let was_degraded = s.degraded;
        s.consecutive_failures = 0;
        s.interval = next_interval(s.interval, 0, found_new);
        s.next_at = Instant::now() + s.interval;
        s.degraded = false;
        drop(sched);
        if was_degraded {
            self.emit_status(
                Some(channel_id),
                "info",
                "フィード取得が復帰しました".to_string(),
            );
        }
    }

    /// 失敗後のスケジュール更新と劣化通知。
    fn after_failure(&self, channel_id: &str, message: String) {
        let emit = {
            let mut sched = lock(&self.sched);
            let s = sched.entry(channel_id.to_string()).or_default();
            s.consecutive_failures += 1;
            s.interval = next_interval(s.interval, s.consecutive_failures, false);
            s.next_at = Instant::now() + s.interval;
            // 閾値到達時のみ通知（連続失敗ごとに送ると通知が洪水になる）
            if s.consecutive_failures == FAIL_NOTIFY_THRESHOLD {
                s.degraded = true;
                true
            } else {
                false
            }
        };
        if emit {
            self.emit_status(
                Some(channel_id),
                "warn",
                format!("フィード取得が連続して失敗しています: {message}"),
            );
        }
    }

    fn emit_status(&self, channel_id: Option<&str>, level: &str, message: String) {
        let payload = FeedStatus {
            channel_id: channel_id.map(|s| s.to_string()),
            level: level.to_string(),
            message,
        };
        if let Err(e) = self.app.emit("feed://status", payload) {
            tracing::warn!(error = %e, "feed://status の送出に失敗");
        }
    }
}

/// `youtube.com/shorts/<id>` への先頭応答が 200 の項目を `kind='short'` に更新する
/// （仕様決定 V）。`client` はリダイレクト非追跡（`redirect::Policy::none()`）で
/// 作ること — 非 short は 303 で `/watch?v=` へ転送され、転送先の 200 は
/// 判定に使わない（実測、2026-10）。判定失敗・タイムアウトは 'video' のまま
/// 残し、初版ではリトライしない。
async fn detect_shorts(client: &reqwest::Client, db: &Db, video_ids: Vec<String>) {
    let mut shorts = Vec::new();
    for id in &video_ids {
        let url = format!("https://www.youtube.com/shorts/{id}");
        match client.head(&url).timeout(SHORTS_TIMEOUT).send().await {
            Ok(resp) if resp.status() == reqwest::StatusCode::OK => {
                shorts.push(id.clone());
            }
            Ok(_) => {}
            Err(e) => {
                tracing::debug!(video_id = %id, error = %e, "shorts 判定リクエストに失敗");
            }
        }
    }
    if !shorts.is_empty() {
        tracing::debug!(count = shorts.len(), "shorts と判定");
        if let Err(e) = db.videos_set_kind(&shorts, "short") {
            tracing::warn!(error = %e, "kind=short の保存に失敗");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<feed xmlns:yt="http://www.youtube.com/xml/schemas/2015"
      xmlns:media="http://search.yahoo.com/mrss/"
      xmlns="http://www.w3.org/2005/Atom">
  <link rel="self" href="http://www.youtube.com/feeds/videos.xml?channel_id=UCXuqSBlHAE6Xw-yeJA0Tunw"/>
  <id>yt:channel:UCXuqSBlHAE6Xw-yeJA0Tunw</id>
  <yt:channelId>UCXuqSBlHAE6Xw-yeJA0Tunw</yt:channelId>
  <title>Linus Tech Tips</title>
  <link rel="alternate" href="https://www.youtube.com/channel/UCXuqSBlHAE6Xw-yeJA0Tunw"/>
  <icon>https://yt3.example/icon.jpg</icon>
  <author>
    <name>Linus Tech Tips</name>
    <uri>https://www.youtube.com/channel/UCXuqSBlHAE6Xw-yeJA0Tunw</uri>
  </author>
  <published>2013-11-25T17:58:16+00:00</published>
  <entry>
    <id>yt:video:dQw4w9WgXcQ</id>
    <yt:videoId>dQw4w9WgXcQ</yt:videoId>
    <yt:channelId>UCXuqSBlHAE6Xw-yeJA0Tunw</yt:channelId>
    <title>テスト動画1</title>
    <link rel="alternate" href="https://www.youtube.com/watch?v=dQw4w9WgXcQ"/>
    <author><name>Linus Tech Tips</name></author>
    <published>2026-10-06T12:00:00+00:00</published>
    <updated>2026-10-06T13:00:00+00:00</updated>
    <media:group>
      <media:title>テスト動画1</media:title>
      <media:thumbnail url="https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg" width="480" height="360"/>
    </media:group>
  </entry>
  <entry>
    <id>yt:video:abc123def45</id>
    <yt:videoId>abc123def45</yt:videoId>
    <yt:channelId>UCXuqSBlHAE6Xw-yeJA0Tunw</yt:channelId>
    <title>テスト動画2</title>
    <published>2026-10-05T00:00:00+00:00</published>
    <updated>2026-10-05T00:00:00+00:00</updated>
  </entry>
</feed>"#;

    #[test]
    fn parse_atom_extracts_entries() {
        let feed = parse_atom(SAMPLE).unwrap();
        assert_eq!(feed.channel_id, "UCXuqSBlHAE6Xw-yeJA0Tunw");
        assert_eq!(feed.channel_title, "Linus Tech Tips");
        assert_eq!(feed.entries.len(), 2);
        let e = &feed.entries[0];
        assert_eq!(e.video_id, "dQw4w9WgXcQ");
        assert_eq!(e.title, "テスト動画1");
        assert_eq!(e.published_at.as_deref(), Some("2026-10-06T12:00:00+00:00"));
        assert_eq!(
            e.thumbnail_url.as_deref(),
            Some("https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg")
        );
        // media:group が無いエントリはサムネイルなし
        assert!(feed.entries[1].thumbnail_url.is_none());
    }

    #[test]
    fn parse_atom_rejects_non_feed() {
        assert!(parse_atom("<html></html>").is_err());
        assert!(parse_atom("not xml").is_err());
    }

    /// golden fixture: 実際に取得した YouTube チャンネル RSS（技術方針 O）。
    /// この実応答ではフィード直下の yt:channelId が `UC` プレフィックスなしで
    /// 返っているのに対し、entry 直下は `UC` 付き — その差異の回帰テスト。
    #[test]
    fn parse_atom_golden_fixture() {
        let xml = include_str!("../../tests/fixtures/youtube_channel_feed.xml");
        let feed = parse_atom(xml).unwrap();
        assert_eq!(feed.channel_id, "UCXuqSBlHAE6Xw-yeJA0Tunw");
        assert_eq!(feed.channel_title, "Linus Tech Tips");
        assert!(!feed.entries.is_empty());
        assert!(feed
            .entries
            .iter()
            .all(|e| !e.video_id.is_empty() && !e.title.is_empty()));
        // サムネイルは許可ホスト（ytimg.com / ggpht.com）のみ採用される
        assert!(feed
            .entries
            .iter()
            .filter_map(|e| e.thumbnail_url.as_deref())
            .all(is_allowed_thumbnail));
    }

    #[test]
    fn normalize_channel_id_restores_uc_prefix() {
        assert_eq!(
            normalize_channel_id("XuqSBlHAE6Xw-yeJA0Tunw"),
            "UCXuqSBlHAE6Xw-yeJA0Tunw"
        );
        assert_eq!(
            normalize_channel_id("UCXuqSBlHAE6Xw-yeJA0Tunw"),
            "UCXuqSBlHAE6Xw-yeJA0Tunw"
        );
    }

    #[test]
    fn thumbnail_rejects_foreign_origin() {
        assert!(is_allowed_thumbnail(
            "https://i.ytimg.com/vi/abc/hqdefault.jpg"
        ));
        assert!(is_allowed_thumbnail("https://yt3.ggpht.com/abc/photo.jpg"));
        assert!(!is_allowed_thumbnail("https://evil.example/x.png"));
        assert!(!is_allowed_thumbnail(
            "http://i.ytimg.com/vi/abc/hqdefault.jpg"
        ));
    }

    #[test]
    fn next_interval_grows_on_idle() {
        let d = next_interval(BASE_INTERVAL, 0, false);
        assert_eq!(d, Duration::from_secs(22 * 60 + 30)); // 15 * 1.5
        let d = next_interval(IDLE_MAX, 0, false);
        assert_eq!(d, IDLE_MAX);
    }

    #[test]
    fn next_interval_resets_on_new() {
        assert_eq!(next_interval(IDLE_MAX, 0, true), BASE_INTERVAL);
    }

    #[test]
    fn next_interval_backs_off_on_failures() {
        assert_eq!(next_interval(BASE_INTERVAL, 1, false), FAIL_BASE);
        assert_eq!(
            next_interval(BASE_INTERVAL, 2, false),
            Duration::from_secs(600)
        );
        assert_eq!(
            next_interval(BASE_INTERVAL, 3, false),
            Duration::from_secs(1200)
        );
        assert_eq!(next_interval(BASE_INTERVAL, 10, false), FAIL_MAX);
    }
}
