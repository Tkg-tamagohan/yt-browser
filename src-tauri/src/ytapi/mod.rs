//! YouTube Data API v3 クライアント基盤（FR-97、仕様決定 EC）。
//!
//! 3 層一方向依存（`model` / `util` のみに依存する純粋リーフドメインモジュール）を守り、
//! 任意設定 `youtube.api_key` が有効な間だけ公式 API を利用するための認証ヘッダ付与、
//! 独立 2 系統（共通枠 / `search.list` 枠）サーキットブレーカー、`key_invalid` ラッチ、
//! 指数バックオフ型クールダウン、および接続テストを提供する。

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::header::{HeaderName, HeaderValue};
use reqwest::{Client, StatusCode};
use serde_json::Value;
use thiserror::Error;
use tracing::warn;

use crate::model::{
    classify_kind, BroadcastStatus, MetaSnapshot, VideoRef, YtApiAvailability, YtApiStatus,
    YtApiTestOutcome,
};
use crate::util::{iso8601_ms, iso_utc, lock, parse_iso8601_duration};

/// YouTube Data API v3 キーの設定キー名（38 個目の設定キー、FR-97）。
pub const SETTING_YOUTUBE_API_KEY: &str = "youtube.api_key";

/// `videos.list` 1 リクエストあたりの最大 ID 件数（FR-98、仕様決定 ED）。
pub const VIDEOS_BATCH_MAX: usize = 50;

/// 本番 API のベース URL。ダンプインポート経由のキー流出を防ぐため定数固定とし、
/// 設定キーや環境変数での上書きは行わない（`#[cfg(test)]` のみモック注入可）。
const API_BASE: &str = "https://www.googleapis.com/youtube/v3";

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const HTTP_TIMEOUT: Duration = Duration::from_secs(8);
const TRANSIENT_COOLDOWN_STEPS_SECS: [i64; 3] = [30, 60, 300];
const HEADER_API_KEY: HeaderName = HeaderName::from_static("x-goog-api-key");

/// YouTube Data API v3 のクォータバケット区分。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuotaBucket {
    /// 共通日次枠（10,000 units / 日、`videos.list` 等）。
    Shared,
    /// 検索専用枠（100 回 / 日、`search.list`）。
    #[allow(dead_code)] // Phase 95 の search.list で使用予定
    Search,
}

#[derive(Debug, Error)]
pub enum YtApiError {
    #[error("YouTube Data API キーが設定されていません")]
    Unset,
    #[error("YouTube Data API キーが無効または利用不可としてラッチされています")]
    KeyInvalid,
    #[error("YouTube Data API の日次クォータ上限に達しています")]
    QuotaExhausted(QuotaBucket),
    #[error("YouTube Data API が一時クールダウン中です")]
    Cooldown,
    #[error("YouTube Data API キーの形式が不正です")]
    MalformedKey,
    #[error("YouTube Data API リクエスト対象が見つかりません (HTTP 404)")]
    NotFound,
    #[error("YouTube Data API が HTTP {0} を返しました")]
    Status(u16),
    #[error("YouTube Data API への通信に失敗しました: {0}")]
    Http(reqwest::Error),
    #[error("YouTube Data API の JSON 応答の解析に失敗しました: {0}")]
    Json(#[from] serde_json::Error),
}

impl YtApiError {
    /// ハイブリッド原則により、公式 API の未設定・無効キー・クォータ上限・
    /// 一時クールダウン・404・通信失敗はいずれも既存経路へのフォールバック対象とする。
    pub fn should_fallback(&self) -> bool {
        true
    }
}

/// `videos.list` 1 件分の統合パース結果（FR-98、仕様決定 ED）。
#[derive(Debug, Clone)]
pub struct YtVideoItem {
    pub video_id: String,
    pub video_ref: VideoRef,
    pub meta: MetaSnapshot,
    #[allow(dead_code)] // テストおよび後続フェーズの尺参照用
    pub duration_sec: Option<i64>,
}

impl YtVideoItem {
    /// 初回解決向けの `kind` 判定（ガード 4）。
    /// 終了後（`actualEndTime` あり かつ 非配信中）の項目は `is_live_content = None` となり、
    /// 既に `current_kind == "premiere"` なら上書きせず `None`、新規解決時は `Some("live")` を返す。
    pub fn classify_kind_for_initial(&self, current_kind: &str) -> Option<&'static str> {
        let b = &self.meta.broadcast;
        if b.end_ms.is_some() && b.is_live_now != Some(true) && self.meta.is_live_content.is_none()
        {
            return if current_kind == "premiere" {
                None
            } else {
                Some("live")
            };
        }
        classify_kind(&self.meta)
    }

    /// 定期再確認・ライブ待機相乗り向けの `kind` 判定（ガード 4）。
    /// 終了後（`actualEndTime` あり かつ 非配信中）で `is_live_content = None` のときは
    /// `None` を返し、DB 側の既存 `kind`（特に `premiere`）を保持させる。
    pub fn classify_kind_for_refresh(&self) -> Option<&'static str> {
        let b = &self.meta.broadcast;
        if b.end_ms.is_some() && b.is_live_now != Some(true) && self.meta.is_live_content.is_none()
        {
            return None;
        }
        classify_kind(&self.meta)
    }
}

struct YtApiState {
    api_key: Option<String>,
    key_invalid: bool,
    shared_quota_until_sec: Option<i64>,
    search_quota_until_sec: Option<i64>,
    transient_cooldown_until_sec: Option<i64>,
    transient_streak: u32,
    session_epoch: u64,
}

/// YouTube Data API v3 クライアントとサーキットブレーカー状態。
pub struct YtApi {
    client: Client,
    api_base: String,
    state: Mutex<YtApiState>,
}

impl fmt::Debug for YtApi {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let st = lock(&self.state);
        f.debug_struct("YtApi")
            .field("configured", &st.api_key.is_some())
            .field("api_key", &st.api_key.as_ref().map(|_| "<redacted>"))
            .field("key_invalid", &st.key_invalid)
            .field("shared_quota_until_sec", &st.shared_quota_until_sec)
            .field("search_quota_until_sec", &st.search_quota_until_sec)
            .field(
                "transient_cooldown_until_sec",
                &st.transient_cooldown_until_sec,
            )
            .field("transient_streak", &st.transient_streak)
            .field("session_epoch", &st.session_epoch)
            .finish()
    }
}

fn normalize_key(raw: Option<&str>) -> Option<String> {
    raw.map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn now_utc_sec() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn build_client(connect_timeout: Duration, timeout: Duration) -> Client {
    Client::builder()
        .connect_timeout(connect_timeout)
        .timeout(timeout)
        .build()
        .expect("ytapi reqwest client")
}

impl YtApi {
    /// 設定済みの初期キーを受け取って `YtApi` を構築する。
    pub fn new(initial_key: Option<String>) -> Self {
        Self {
            client: build_client(CONNECT_TIMEOUT, HTTP_TIMEOUT),
            api_base: API_BASE.to_string(),
            state: Mutex::new(YtApiState {
                api_key: normalize_key(initial_key.as_deref()),
                key_invalid: false,
                shared_quota_until_sec: None,
                search_quota_until_sec: None,
                transient_cooldown_until_sec: None,
                transient_streak: 0,
                session_epoch: 0,
            }),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_test_base_url(
        initial_key: Option<String>,
        api_base: impl Into<String>,
    ) -> Self {
        let mut api = Self::new(initial_key);
        api.api_base = api_base.into();
        api
    }

    #[cfg(test)]
    pub(crate) fn with_test_timeouts(
        mut self,
        connect_timeout: Duration,
        timeout: Duration,
    ) -> Self {
        self.client = build_client(connect_timeout, timeout);
        self
    }

    /// `settings_set` によるキー変更通知。トリム後の文字列が実際に変化した場合のみ
    /// すべての遮断器・ラッチ・クールダウン・`session_epoch` をリセットする。
    pub fn on_key_changed(&self, raw: Option<&str>) {
        let next = normalize_key(raw);
        let mut st = lock(&self.state);
        if st.api_key == next {
            return;
        }
        st.api_key = next;
        st.key_invalid = false;
        st.shared_quota_until_sec = None;
        st.search_quota_until_sec = None;
        st.transient_cooldown_until_sec = None;
        st.transient_streak = 0;
        st.session_epoch = st.session_epoch.wrapping_add(1);
    }

    /// 現在の利用可否状態と次回リセット時刻（UTC epoch ms）を返す。
    pub fn status(&self) -> YtApiStatus {
        self.status_at(now_utc_sec())
    }

    pub(crate) fn status_at(&self, now_sec: i64) -> YtApiStatus {
        let mut st = lock(&self.state);
        clear_expired_breakers(&mut st, now_sec);
        let configured = st.api_key.is_some();
        let key_invalid = configured && st.key_invalid;
        let shared_quota_exhausted = configured && st.shared_quota_until_sec.is_some();
        let search_quota_exhausted = configured && st.search_quota_until_sec.is_some();
        let shared_reset_ms = if shared_quota_exhausted {
            st.shared_quota_until_sec.map(|s| s * 1_000)
        } else {
            None
        };
        let search_reset_ms = if search_quota_exhausted {
            st.search_quota_until_sec.map(|s| s * 1_000)
        } else {
            None
        };
        let state = if !configured {
            YtApiAvailability::Unset
        } else if key_invalid {
            YtApiAvailability::KeyInvalid
        } else if shared_quota_exhausted {
            YtApiAvailability::SharedQuota
        } else if search_quota_exhausted {
            YtApiAvailability::SearchQuota
        } else {
            YtApiAvailability::Active
        };
        YtApiStatus {
            state,
            configured,
            key_invalid,
            shared_quota_exhausted,
            search_quota_exhausted,
            shared_reset_ms,
            search_reset_ms,
        }
    }

    /// 指定バケットが現在利用可能か判定する（期限切れ遮断器は遅延クリアする）。
    pub fn can_use(&self, bucket: QuotaBucket) -> bool {
        self.can_use_at(bucket, now_utc_sec())
    }

    pub(crate) fn can_use_at(&self, bucket: QuotaBucket, now_sec: i64) -> bool {
        let mut st = lock(&self.state);
        clear_expired_breakers(&mut st, now_sec);
        if st.api_key.is_none() || st.key_invalid {
            return false;
        }
        if st.transient_cooldown_until_sec.is_some() {
            return false;
        }
        match bucket {
            QuotaBucket::Shared => st.shared_quota_until_sec.is_none(),
            QuotaBucket::Search => st.search_quota_until_sec.is_none(),
        }
    }

    /// 1 unit の `videos.list` 呼び出しで接続テストを行う。
    /// `candidate_key` がトリム後非空で保存済みキーと異なる場合は、保存済みキーの
    /// 遮断器・ラッチを書き換えずに候補キー単体をテストする。
    /// `None`（または空白のみ）や保存済みキーと一致する場合は保存済みキーをテストし、
    /// 成功時はラッチ・クールダウンを解除、拒否時は遮断器・ラッチへ記録する。
    pub async fn test_connection(&self, candidate_key: Option<&str>) -> YtApiTestOutcome {
        let candidate = normalize_key(candidate_key);
        let (key_to_test, is_saved_key) = {
            let st = lock(&self.state);
            match candidate {
                Some(c) => {
                    let is_saved = st.api_key.as_deref() == Some(c.as_str());
                    (c, is_saved)
                }
                None => match &st.api_key {
                    Some(saved) => (saved.clone(), true),
                    None => {
                        return YtApiTestOutcome {
                            ok: false,
                            reason: Some("API キーが設定されていません".to_string()),
                        };
                    }
                },
            }
        };

        match self
            .send_get_with_key(
                QuotaBucket::Shared,
                "/videos",
                &[("part", "id"), ("id", "dQw4w9WgXcQ")],
                &key_to_test,
                is_saved_key,
            )
            .await
        {
            Ok(_) => YtApiTestOutcome {
                ok: true,
                reason: None,
            },
            Err(err) => YtApiTestOutcome {
                ok: false,
                reason: Some(describe_test_error(&err)),
            },
        }
    }

    /// `videos.list` で最大 50 件チャンクごとに動画メタ・尺・放送状態を一括取得する（1 unit / 回）。
    /// 空白と重複 ID は除去し、空入力時は通信せず空マップを返す。
    pub async fn videos_list<S: AsRef<str>>(
        &self,
        video_ids: &[S],
    ) -> Result<HashMap<String, YtVideoItem>, YtApiError> {
        let mut unique: Vec<&str> = Vec::new();
        let mut seen: HashSet<&str> = HashSet::new();
        for id in video_ids {
            let s = id.as_ref().trim();
            if !s.is_empty() && seen.insert(s) {
                unique.push(s);
            }
        }
        if unique.is_empty() {
            return Ok(HashMap::new());
        }

        let mut out = HashMap::with_capacity(unique.len());
        for chunk in unique.chunks(VIDEOS_BATCH_MAX) {
            let joined = chunk.join(",");
            let val = self
                .get_json(
                    QuotaBucket::Shared,
                    "/videos",
                    &[
                        (
                            "part",
                            "snippet,contentDetails,liveStreamingDetails,statistics",
                        ),
                        ("id", &joined),
                    ],
                )
                .await?;
            if let Some(items) = val.get("items").and_then(|v| v.as_array()) {
                for item in items {
                    if let Some(parsed) = parse_video_item(item) {
                        out.insert(parsed.video_id.clone(), parsed);
                    }
                }
            }
        }
        Ok(out)
    }

    /// 保存済みキーと指定バケットを使って GET リクエストを送る（遮断器チェック込み）。
    pub(crate) async fn get_json(
        &self,
        bucket: QuotaBucket,
        path: &str,
        query: &[(&str, &str)],
    ) -> Result<Value, YtApiError> {
        let now_sec = now_utc_sec();
        let key = {
            let mut st = lock(&self.state);
            clear_expired_breakers(&mut st, now_sec);
            let Some(k) = st.api_key.clone() else {
                return Err(YtApiError::Unset);
            };
            if st.key_invalid {
                return Err(YtApiError::KeyInvalid);
            }
            if st.transient_cooldown_until_sec.is_some() {
                return Err(YtApiError::Cooldown);
            }
            let quota_blocked = match bucket {
                QuotaBucket::Shared => st.shared_quota_until_sec.is_some(),
                QuotaBucket::Search => st.search_quota_until_sec.is_some(),
            };
            if quota_blocked {
                return Err(YtApiError::QuotaExhausted(bucket));
            }
            k
        };
        self.send_get_with_key(bucket, path, query, &key, true)
            .await
    }

    async fn send_get_with_key(
        &self,
        bucket: QuotaBucket,
        path: &str,
        query: &[(&str, &str)],
        api_key: &str,
        mutate_saved_state: bool,
    ) -> Result<Value, YtApiError> {
        let mut header_val = match HeaderValue::from_str(api_key) {
            Ok(v) => v,
            Err(_) => {
                if mutate_saved_state {
                    let mut st = lock(&self.state);
                    if st.api_key.as_deref() == Some(api_key) {
                        st.key_invalid = true;
                    }
                }
                return Err(YtApiError::MalformedKey);
            }
        };
        header_val.set_sensitive(true);

        let url = format!("{}{}", self.api_base.trim_end_matches('/'), path);
        let resp = self
            .client
            .get(&url)
            .query(query)
            .header(HEADER_API_KEY, header_val)
            .send()
            .await
            .map_err(|e| e.without_url());

        let resp = match resp {
            Ok(r) => r,
            Err(e) => {
                if mutate_saved_state {
                    self.record_transient_failure(api_key, now_utc_sec());
                }
                warn!(error = %e, "YouTube Data API への HTTP 送信に失敗");
                return Err(YtApiError::Http(e));
            }
        };

        let status = resp.status();
        let bytes = match resp.bytes().await {
            Ok(b) => b,
            Err(e) => {
                let e = e.without_url();
                if mutate_saved_state {
                    self.record_transient_failure(api_key, now_utc_sec());
                }
                warn!(
                    error = %e,
                    status = status.as_u16(),
                    "YouTube Data API 応答本文の受信に失敗"
                );
                return Err(YtApiError::Http(e));
            }
        };
        if status.is_success() {
            let value = serde_json::from_slice::<Value>(&bytes)?;
            if mutate_saved_state {
                self.record_success(api_key, bucket);
            }
            return Ok(value);
        }

        let body = String::from_utf8_lossy(&bytes);
        let err = self.handle_error_response(
            bucket,
            status,
            &body,
            api_key,
            mutate_saved_state,
            now_utc_sec(),
        );
        Err(err)
    }

    fn record_success(&self, api_key: &str, bucket: QuotaBucket) {
        let mut st = lock(&self.state);
        if st.api_key.as_deref() != Some(api_key) {
            return;
        }
        st.key_invalid = false;
        st.transient_cooldown_until_sec = None;
        st.transient_streak = 0;
        match bucket {
            QuotaBucket::Shared => st.shared_quota_until_sec = None,
            QuotaBucket::Search => st.search_quota_until_sec = None,
        }
    }

    fn record_transient_failure(&self, api_key: &str, now_sec: i64) {
        let mut st = lock(&self.state);
        if st.api_key.as_deref() != Some(api_key) {
            return;
        }
        let idx = (st.transient_streak as usize).min(TRANSIENT_COOLDOWN_STEPS_SECS.len() - 1);
        let delay = TRANSIENT_COOLDOWN_STEPS_SECS[idx];
        st.transient_cooldown_until_sec = Some(now_sec + delay);
        st.transient_streak = st.transient_streak.saturating_add(1);
    }

    fn handle_error_response(
        &self,
        bucket: QuotaBucket,
        status: StatusCode,
        body: &str,
        api_key: &str,
        mutate_saved_state: bool,
        now_sec: i64,
    ) -> YtApiError {
        let reasons = parse_error_reasons(body);
        let is_key_invalid = (status == StatusCode::BAD_REQUEST
            && reasons
                .iter()
                .any(|r| matches!(r.as_str(), "keyInvalid" | "API_KEY_INVALID")))
            || status == StatusCode::UNAUTHORIZED
            || (status == StatusCode::FORBIDDEN
                && reasons.iter().any(|r| {
                    matches!(
                        r.as_str(),
                        "keyExpired" | "accessNotConfigured" | "ipRefererBlocked"
                    )
                }));
        if is_key_invalid {
            if mutate_saved_state {
                let mut st = lock(&self.state);
                if st.api_key.as_deref() == Some(api_key) {
                    st.key_invalid = true;
                }
            }
            return YtApiError::KeyInvalid;
        }

        let is_quota = status == StatusCode::FORBIDDEN
            && (reasons.is_empty()
                || reasons.iter().any(|r| {
                    matches!(
                        r.as_str(),
                        "quotaExceeded"
                            | "dailyLimitExceeded"
                            | "rateLimitExceeded"
                            | "userRateLimitExceeded"
                            | "dailyLimitExceededUnreg"
                    )
                }));
        if is_quota {
            if mutate_saved_state {
                let reset_sec = next_la_midnight_plus_60s(now_sec);
                let mut st = lock(&self.state);
                if st.api_key.as_deref() == Some(api_key) {
                    match bucket {
                        QuotaBucket::Shared => st.shared_quota_until_sec = Some(reset_sec),
                        QuotaBucket::Search => st.search_quota_until_sec = Some(reset_sec),
                    }
                }
            }
            return YtApiError::QuotaExhausted(bucket);
        }

        if status == StatusCode::NOT_FOUND {
            return YtApiError::NotFound;
        }

        if (status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error())
            && mutate_saved_state
        {
            self.record_transient_failure(api_key, now_sec);
        }

        YtApiError::Status(status.as_u16())
    }
}

fn parse_video_item(item: &Value) -> Option<YtVideoItem> {
    let video_id = item
        .get("id")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())?
        .to_string();

    let snippet = item.get("snippet");
    let title = snippet
        .and_then(|s| s.get("title"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .unwrap_or_default()
        .to_string();
    let channel_id = snippet
        .and_then(|s| s.get("channelId"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let channel_title = snippet
        .and_then(|s| s.get("channelTitle"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let description = snippet
        .and_then(|s| s.get("description"))
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let published_at = snippet
        .and_then(|s| s.get("publishedAt"))
        .and_then(|v| v.as_str())
        .and_then(iso8601_ms)
        .map(iso_utc);
    let thumbnails = snippet.and_then(|s| s.get("thumbnails"));
    let thumbnail_url = ["maxres", "standard", "high", "medium", "default"]
        .into_iter()
        .find_map(|key| {
            thumbnails
                .and_then(|t| t.get(key))
                .and_then(|entry| entry.get("url"))
                .and_then(|u| u.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        })
        .or_else(|| Some(format!("https://i.ytimg.com/vi/{video_id}/hqdefault.jpg")));

    let duration_sec = item
        .get("contentDetails")
        .and_then(|c| c.get("duration"))
        .and_then(|v| v.as_str())
        .and_then(parse_iso8601_duration);

    let live_broadcast_content = snippet
        .and_then(|s| s.get("liveBroadcastContent"))
        .and_then(|v| v.as_str())
        .map(str::trim);
    let details = item.get("liveStreamingDetails").filter(|v| v.is_object());
    let has_details = details.is_some();
    let detail_ms = |key: &str| {
        details
            .and_then(|d| d.get(key))
            .and_then(|v| v.as_str())
            .and_then(iso8601_ms)
    };
    let start_ms = detail_ms("actualStartTime");
    let end_ms = detail_ms("actualEndTime");
    let scheduled_start_ms = detail_ms("scheduledStartTime");
    let is_upcoming = live_broadcast_content == Some("upcoming");
    let is_live_now = if live_broadcast_content == Some("live") {
        Some(true)
    } else if has_details || is_upcoming {
        Some(false)
    } else {
        None
    };

    let broadcast = BroadcastStatus {
        is_live_now,
        start_ms,
        end_ms,
        scheduled_start_ms,
        has_details,
        is_upcoming,
    };

    let is_broadcast = is_live_now == Some(true) || has_details || is_upcoming;
    let is_live_content = if !is_broadcast {
        Some(false)
    } else if end_ms.is_some() && is_live_now != Some(true) {
        // ガード 4: 終了後はライブアーカイブと公開済みプレミアの判別情報が公式 API に無い。
        None
    } else {
        // 配信前・配信中は尺 0 秒（P0D / PT0S）がライブ、>0 秒がプレミア。
        // 尺が欠落したライブ中応答は None とし、classify_kind 側で live と扱う。
        duration_sec.map(|d| d == 0)
    };

    Some(YtVideoItem {
        video_id: video_id.clone(),
        video_ref: VideoRef {
            video_id,
            title,
            channel_id,
            channel_title,
            thumbnail_url,
        },
        meta: MetaSnapshot {
            published_at,
            broadcast,
            is_live_content,
            playability: Some("OK".to_string()),
            description,
        },
        duration_sec,
    })
}

fn clear_expired_breakers(st: &mut YtApiState, now_sec: i64) {
    if st.shared_quota_until_sec.is_some_and(|u| u <= now_sec) {
        st.shared_quota_until_sec = None;
    }
    if st.search_quota_until_sec.is_some_and(|u| u <= now_sec) {
        st.search_quota_until_sec = None;
    }
    if st
        .transient_cooldown_until_sec
        .is_some_and(|u| u <= now_sec)
    {
        st.transient_cooldown_until_sec = None;
    }
}

fn describe_test_error(err: &YtApiError) -> String {
    match err {
        YtApiError::Unset => "API キーが設定されていません".to_string(),
        YtApiError::MalformedKey => "API キーの形式が不正です".to_string(),
        YtApiError::KeyInvalid => {
            "API キーが無効、または API の有効化・制限設定と一致しません".to_string()
        }
        YtApiError::QuotaExhausted(_) => {
            "YouTube Data API の日次クォータ上限に達しています".to_string()
        }
        YtApiError::NotFound => "テスト対象動画が見つかりませんでした (HTTP 404)".to_string(),
        YtApiError::Status(code) => {
            format!("YouTube Data API がエラー応答を返しました (HTTP {code})")
        }
        YtApiError::Http(e) => {
            if e.is_timeout() {
                "YouTube Data API への接続がタイムアウトしました".to_string()
            } else {
                "YouTube Data API への接続に失敗しました".to_string()
            }
        }
        YtApiError::Json(_) => "YouTube Data API の応答を解析できませんでした".to_string(),
        YtApiError::Cooldown => "一時的なエラー後にクールダウン中です".to_string(),
    }
}

fn parse_error_reasons(body: &str) -> Vec<String> {
    let Ok(val) = serde_json::from_str::<Value>(body) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    if let Some(arr) = val
        .get("error")
        .and_then(|e| e.get("errors"))
        .and_then(|a| a.as_array())
    {
        for item in arr {
            if let Some(r) = item.get("reason").and_then(|r| r.as_str()) {
                out.push(r.to_string());
            }
        }
    }
    if let Some(arr) = val
        .get("error")
        .and_then(|e| e.get("details"))
        .and_then(|a| a.as_array())
    {
        for item in arr {
            if let Some(r) = item.get("reason").and_then(|r| r.as_str()) {
                out.push(r.to_string());
            }
        }
    }
    out
}

/// Howard Hinnant `days_from_civil`: 1970-01-01 = 0。
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y2 = if m <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468
}

/// Howard Hinnant `civil_from_days`: 1970-01-01 = 0 -> `(y, m, d)`。
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mo <= 2 { y + 1 } else { y };
    (y, mo, d)
}

/// 月初の曜日（0 = 日, 1 = 月, ..., 6 = 土）から第 1 日曜の日付（1..=7）を返す。
fn first_sunday(y: i64, m: i64) -> i64 {
    let w1 = (days_from_civil(y, m, 1) + 4).rem_euclid(7);
    1 + (7 - w1) % 7
}

/// 指定年の米国夏時間開始時刻（3 月第 2 日曜 02:00 PST = 10:00 UTC）の epoch 秒。
fn us_dst_start_utc_sec(y: i64) -> i64 {
    let mar_2nd_sun = first_sunday(y, 3) + 7;
    days_from_civil(y, 3, mar_2nd_sun) * 86_400 + 10 * 3_600
}

/// 指定年の米国夏時間終了時刻（11 月第 1 日曜 02:00 PDT = 09:00 UTC）の epoch 秒。
fn us_dst_end_utc_sec(y: i64) -> i64 {
    let nov_1st_sun = first_sunday(y, 11);
    days_from_civil(y, 11, nov_1st_sun) * 86_400 + 9 * 3_600
}

/// 現在 UTC epoch 秒から見て次に来る `00:00 America/Los_Angeles (+60s)` の UTC epoch 秒。
/// 米国夏時間（3 月第 2 日曜 02:00 PST 開始、11 月第 1 日曜 02:00 PDT 終了）を厳密に考慮する。
pub(crate) fn next_la_midnight_plus_60s(now_utc_sec: i64) -> i64 {
    let (utc_y, _, _) = civil_from_days(now_utc_sec.div_euclid(86_400));
    let is_pdt =
        now_utc_sec >= us_dst_start_utc_sec(utc_y) && now_utc_sec < us_dst_end_utc_sec(utc_y);
    let offset_sec = if is_pdt { -7 * 3_600 } else { -8 * 3_600 };

    let (ly, lm, ld) = civil_from_days((now_utc_sec + offset_sec).div_euclid(86_400));
    let next_la_day = days_from_civil(ly, lm, ld) + 1;
    let (ny, _, _) = civil_from_days(next_la_day);

    // 米国 DST の遷移は 02:00 のため、00:00 の時点が DST 中かどうかは
    // 08:00 UTC（PST 00:00）と [dst_start, dst_end) の比較で曖昧さなく決まる。
    let pst_midnight_utc = next_la_day * 86_400 + 8 * 3_600;
    let next_is_pdt =
        pst_midnight_utc >= us_dst_start_utc_sec(ny) && pst_midnight_utc < us_dst_end_utc_sec(ny);
    let midnight_utc = if next_is_pdt {
        pst_midnight_utc - 3_600
    } else {
        pst_midnight_utc
    };
    midnight_utc + 60
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::{iso8601_ms, iso_utc, parse_iso8601_duration};
    use std::sync::{Arc, Mutex as StdMutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[derive(Clone)]
    struct MockReply {
        status: u16,
        body: String,
        delay: Duration,
    }

    #[derive(Clone)]
    struct RecordedReq {
        request_line: String,
        headers_lower: String,
    }

    async fn spawn_mock_server(
        replies: Vec<MockReply>,
    ) -> (String, Arc<StdMutex<Vec<RecordedReq>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let recorded = Arc::new(StdMutex::new(Vec::new()));
        let rec_clone = Arc::clone(&recorded);
        let queue = Arc::new(StdMutex::new(std::collections::VecDeque::from(replies)));

        tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };
                let rec = Arc::clone(&rec_clone);
                let q = Arc::clone(&queue);
                tokio::spawn(async move {
                    let mut buf = [0u8; 4096];
                    let mut raw = Vec::new();
                    loop {
                        let Ok(n) = stream.read(&mut buf).await else {
                            return;
                        };
                        if n == 0 {
                            break;
                        }
                        raw.extend_from_slice(&buf[..n]);
                        if raw.windows(4).any(|w| w == b"\r\n\r\n") {
                            break;
                        }
                    }
                    let text = String::from_utf8_lossy(&raw).to_string();
                    let request_line = text.lines().next().unwrap_or_default().to_string();
                    rec.lock().unwrap().push(RecordedReq {
                        request_line,
                        headers_lower: text.to_ascii_lowercase(),
                    });

                    let reply = q.lock().unwrap().pop_front().unwrap_or(MockReply {
                        status: 200,
                        body: r#"{"items":[]}"#.to_string(),
                        delay: Duration::ZERO,
                    });
                    if !reply.delay.is_zero() {
                        tokio::time::sleep(reply.delay).await;
                    }
                    let reason = match reply.status {
                        200 => "OK",
                        400 => "Bad Request",
                        401 => "Unauthorized",
                        403 => "Forbidden",
                        404 => "Not Found",
                        429 => "Too Many Requests",
                        500 => "Internal Server Error",
                        _ => "Error",
                    };
                    let resp = format!(
                        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        reply.status,
                        reason,
                        reply.body.len(),
                        reply.body
                    );
                    let _ = stream.write_all(resp.as_bytes()).await;
                    let _ = stream.shutdown().await;
                });
            }
        });

        (format!("http://{addr}"), recorded)
    }

    /// DB-YA-01: HTTP 衛生規約（`X-Goog-Api-Key` ヘッダのみを使用し `?key=` クエリを付けない、
    /// `Debug` 出力がキー値を伏せる、`reqwest::Error::without_url()` が URL を落とす）を検証する。
    #[tokio::test]
    async fn http_hygiene_header_only_debug_redacted_and_without_url() {
        let secret = "AIzaSyHYGIENE_01";
        let (base, recorded) = spawn_mock_server(vec![MockReply {
            status: 200,
            body: r#"{"items":[{"id":"dQw4w9WgXcQ"}]}"#.to_string(),
            delay: Duration::ZERO,
        }])
        .await;

        let api = YtApi::with_test_base_url(Some(secret.to_string()), base);
        let dbg = format!("{api:?}");
        assert!(!dbg.contains(secret), "Debug leaked API key: {dbg}");
        assert!(dbg.contains("<redacted>"), "{dbg}");

        let out = api.test_connection(None).await;
        assert!(out.ok, "{out:?}");

        {
            let reqs = recorded.lock().unwrap();
            assert_eq!(reqs.len(), 1);
            assert!(
                !reqs[0].request_line.contains("key="),
                "query leaked key: {}",
                reqs[0].request_line
            );
            assert!(
                reqs[0]
                    .headers_lower
                    .contains(&format!("x-goog-api-key: {}", secret.to_ascii_lowercase())),
                "missing X-Goog-Api-Key header: {}",
                reqs[0].headers_lower
            );
        }

        // 接続不能ポートへ送った通信エラーでも URL（クエリ含む）がエラー表示に含まれない
        let dead_api = YtApi::with_test_base_url(Some(secret.to_string()), "http://127.0.0.1:1");
        let err = dead_api
            .get_json(QuotaBucket::Shared, "/videos", &[("part", "id")])
            .await
            .unwrap_err();
        let msg = err.to_string();
        assert!(!msg.contains("127.0.0.1:1"), "without_url failed: {msg}");
        assert!(!msg.contains(secret), "error leaked key: {msg}");
    }

    /// DB-YA-02: `400 keyInvalid` / `401` / `403 keyExpired|accessNotConfigured|ipRefererBlocked` で
    /// `key_invalid` がラッチされ、`on_key_changed` はトリム後キー文字列が実際に変化した時だけ
    /// ラッチと遮断器と `session_epoch` をリセットすることを検証する。
    #[tokio::test]
    async fn key_invalid_latch_and_hot_reload_only_on_trimmed_change() {
        let cases = [
            (400, r#"{"error":{"errors":[{"reason":"keyInvalid"}]}}"#),
            (
                400,
                r#"{"error":{"errors":[{"reason":"badRequest"}],"details":[{"@type":"type.googleapis.com/google.rpc.ErrorInfo","reason":"API_KEY_INVALID"}]}}"#,
            ),
            (401, r#"{"error":{"errors":[{"reason":"authError"}]}}"#),
            (403, r#"{"error":{"errors":[{"reason":"keyExpired"}]}}"#),
            (
                403,
                r#"{"error":{"errors":[{"reason":"accessNotConfigured"}]}}"#,
            ),
            (
                403,
                r#"{"error":{"errors":[{"reason":"ipRefererBlocked"}]}}"#,
            ),
        ];

        for (status, body) in cases {
            let (base, recorded) = spawn_mock_server(vec![MockReply {
                status,
                body: body.to_string(),
                delay: Duration::ZERO,
            }])
            .await;
            let api = YtApi::with_test_base_url(Some("  AIzaSyKEY_A  ".to_string()), base);
            let err = api
                .get_json(QuotaBucket::Shared, "/videos", &[("part", "id")])
                .await
                .unwrap_err();
            assert!(
                matches!(err, YtApiError::KeyInvalid),
                "case {status}: {err}"
            );
            assert_eq!(api.status().state, YtApiAvailability::KeyInvalid);
            assert!(!api.can_use(QuotaBucket::Shared));

            // 既にラッチ済みのため追加の HTTP リクエストは発火しない
            let err2 = api
                .get_json(QuotaBucket::Shared, "/videos", &[("part", "id")])
                .await
                .unwrap_err();
            assert!(matches!(err2, YtApiError::KeyInvalid));
            assert_eq!(recorded.lock().unwrap().len(), 1);

            let epoch_before = lock(&api.state).session_epoch;
            // 空白違いの同一キーではリセットされない
            api.on_key_changed(Some("AIzaSyKEY_A"));
            assert_eq!(api.status().state, YtApiAvailability::KeyInvalid);
            assert_eq!(lock(&api.state).session_epoch, epoch_before);

            // 別キーへ変更したときだけリセットされる
            api.on_key_changed(Some("AIzaSyKEY_B"));
            assert_eq!(api.status().state, YtApiAvailability::Active);
            assert!(api.can_use(QuotaBucket::Shared));
            assert_eq!(lock(&api.state).session_epoch, epoch_before + 1);
        }
    }

    /// DB-YA-03: 共通枠（`shared_quota_until`）と検索枠（`search_quota_until`）が独立した
    /// サーキットブレーカーとして動作し、一方の枯渇が他方を遮断しないこと、および期限到来で解除されることを検証する。
    #[tokio::test]
    async fn dual_quota_breakers_are_independent_and_expire() {
        let quota_body = r#"{"error":{"errors":[{"reason":"quotaExceeded"}]}}"#.to_string();
        let (base, _) = spawn_mock_server(vec![
            MockReply {
                status: 403,
                body: quota_body.clone(),
                delay: Duration::ZERO,
            },
            MockReply {
                status: 200,
                body: r#"{"items":[{"id":"dQw4w9WgXcQ"}]}"#.to_string(),
                delay: Duration::ZERO,
            },
        ])
        .await;

        let api = YtApi::with_test_base_url(Some("AIzaSyQUOTA_TEST".to_string()), base);
        let err = api
            .get_json(QuotaBucket::Search, "/search", &[("part", "snippet")])
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            YtApiError::QuotaExhausted(QuotaBucket::Search)
        ));

        let st = api.status();
        assert_eq!(st.state, YtApiAvailability::SearchQuota);
        assert!(st.search_quota_exhausted);
        assert!(!st.shared_quota_exhausted);
        assert!(st.search_reset_ms.is_some());
        assert!(st.shared_reset_ms.is_none());

        // Search 枠枯渇中でも Shared 枠は利用可能
        assert!(!api.can_use(QuotaBucket::Search));
        assert!(api.can_use(QuotaBucket::Shared));
        let ok = api
            .get_json(QuotaBucket::Shared, "/videos", &[("part", "id")])
            .await;
        assert!(ok.is_ok(), "{ok:?}");

        // リセット時刻を過ぎると Search 枠が遅延クリアされる
        let reset_sec = lock(&api.state).search_quota_until_sec.unwrap();
        assert!(api.can_use_at(QuotaBucket::Search, reset_sec));
        assert_eq!(api.status_at(reset_sec).state, YtApiAvailability::Active);

        // 逆に Shared 枠を枯渇させても Search 枠は独立して利用可能
        let (base2, _) = spawn_mock_server(vec![MockReply {
            status: 403,
            body: quota_body,
            delay: Duration::ZERO,
        }])
        .await;
        let api2 = YtApi::with_test_base_url(Some("AIzaSyQUOTA_TEST2".to_string()), base2);
        let err2 = api2
            .get_json(QuotaBucket::Shared, "/videos", &[("part", "id")])
            .await
            .unwrap_err();
        assert!(matches!(
            err2,
            YtApiError::QuotaExhausted(QuotaBucket::Shared)
        ));
        assert_eq!(api2.status().state, YtApiAvailability::SharedQuota);
        assert!(!api2.can_use(QuotaBucket::Shared));
        assert!(api2.can_use(QuotaBucket::Search));
    }

    /// DB-YA-04: `next_la_midnight_plus_60s` が冬時間（PST）、夏時間（PDT）、
    /// 3 月第 2 日曜の春切替日、11 月第 1 日曜の秋切替日で正しく次回 `00:00 America/Los_Angeles (+60s)` を返すことを検証する。
    #[test]
    fn la_midnight_reset_handles_us_dst_boundaries() {
        // 冬時間 (PST = UTC-8): 2026-01-15 12:00:00 UTC (= 2026-01-15 04:00 PST)
        // -> 次回 LA 00:01 PST = 2026-01-16 08:01:00 UTC
        let jan = iso8601_ms("2026-01-15T12:00:00Z").unwrap() / 1_000;
        assert_eq!(
            iso_utc(next_la_midnight_plus_60s(jan) * 1_000),
            "2026-01-16T08:01:00+00:00"
        );

        // 夏時間 (PDT = UTC-7): 2026-07-15 12:00:00 UTC (= 2026-07-15 05:00 PDT)
        // -> 次回 LA 00:01 PDT = 2026-07-16 07:01:00 UTC
        let jul = iso8601_ms("2026-07-15T12:00:00Z").unwrap() / 1_000;
        assert_eq!(
            iso_utc(next_la_midnight_plus_60s(jul) * 1_000),
            "2026-07-16T07:01:00+00:00"
        );

        // 2026-03-08 は 3 月第 2 日曜（春切替日）: 09:00 UTC = 01:00 PST
        // -> 翌日 2026-03-09 00:01 PDT = 2026-03-09 07:01:00 UTC
        let mar_dst = iso8601_ms("2026-03-08T09:00:00Z").unwrap() / 1_000;
        assert_eq!(
            iso_utc(next_la_midnight_plus_60s(mar_dst) * 1_000),
            "2026-03-09T07:01:00+00:00"
        );

        // 2026-03-07 20:00 PST (= 2026-03-08 04:00 UTC) -> 翌日 2026-03-08 00:01 PST = 08:01 UTC
        let mar_pre = iso8601_ms("2026-03-08T04:00:00Z").unwrap() / 1_000;
        assert_eq!(
            iso_utc(next_la_midnight_plus_60s(mar_pre) * 1_000),
            "2026-03-08T08:01:00+00:00"
        );

        // 2026-11-01 は 11 月第 1 日曜（秋切替日）: 08:00 UTC = 01:00 PDT
        // -> 翌日 2026-11-02 00:01 PST = 2026-11-02 08:01:00 UTC
        let nov_dst = iso8601_ms("2026-11-01T08:00:00Z").unwrap() / 1_000;
        assert_eq!(
            iso_utc(next_la_midnight_plus_60s(nov_dst) * 1_000),
            "2026-11-02T08:01:00+00:00"
        );
    }

    /// DB-YA-05: `429` / `5xx` が `30s -> 60s -> 300s` のクールダウンを取り、
    /// `200` 成功で streak がリセットされること、および `404` が遮断器もクールダウンも開かないことを検証する。
    #[tokio::test]
    async fn transient_cooldown_steps_and_404_non_breaker() {
        let (base, _) = spawn_mock_server(vec![
            MockReply {
                status: 404,
                body: r#"{"error":{"code":404}}"#.to_string(),
                delay: Duration::ZERO,
            },
            MockReply {
                status: 429,
                body: "rate limit".to_string(),
                delay: Duration::ZERO,
            },
            MockReply {
                status: 503,
                body: "unavailable".to_string(),
                delay: Duration::ZERO,
            },
            MockReply {
                status: 500,
                body: "internal".to_string(),
                delay: Duration::ZERO,
            },
            MockReply {
                status: 200,
                body: r#"{"items":[]}"#.to_string(),
                delay: Duration::ZERO,
            },
        ])
        .await;

        let api = YtApi::with_test_base_url(Some("AIzaSyTRANSIENT".to_string()), base);

        // 404 は非遮断
        let err404 = api
            .get_json(QuotaBucket::Shared, "/videos", &[("part", "id")])
            .await
            .unwrap_err();
        assert!(matches!(err404, YtApiError::NotFound));
        assert!(api.can_use(QuotaBucket::Shared));
        assert_eq!(lock(&api.state).transient_streak, 0);

        // 1 回目 (429): +30s
        let t0 = now_utc_sec();
        let err1 = api
            .get_json(QuotaBucket::Shared, "/videos", &[("part", "id")])
            .await
            .unwrap_err();
        assert!(matches!(err1, YtApiError::Status(429)));
        {
            let st = lock(&api.state);
            assert_eq!(st.transient_streak, 1);
            let until = st.transient_cooldown_until_sec.unwrap();
            assert!(
                (t0 + 29..=t0 + 35).contains(&until),
                "until={until}, t0={t0}"
            );
        }
        assert!(!api.can_use(QuotaBucket::Shared));

        // クールダウン期限をクリアして 2 回目 (503): +60s
        lock(&api.state).transient_cooldown_until_sec = None;
        let t1 = now_utc_sec();
        let err2 = api
            .get_json(QuotaBucket::Shared, "/videos", &[("part", "id")])
            .await
            .unwrap_err();
        assert!(matches!(err2, YtApiError::Status(503)));
        {
            let st = lock(&api.state);
            assert_eq!(st.transient_streak, 2);
            let until = st.transient_cooldown_until_sec.unwrap();
            assert!(
                (t1 + 59..=t1 + 65).contains(&until),
                "until={until}, t1={t1}"
            );
        }

        // クールダウン期限をクリアして 3 回目 (500): +300s
        lock(&api.state).transient_cooldown_until_sec = None;
        let t2 = now_utc_sec();
        let err3 = api
            .get_json(QuotaBucket::Shared, "/videos", &[("part", "id")])
            .await
            .unwrap_err();
        assert!(matches!(err3, YtApiError::Status(500)));
        {
            let st = lock(&api.state);
            assert_eq!(st.transient_streak, 3);
            let until = st.transient_cooldown_until_sec.unwrap();
            assert!(
                (t2 + 299..=t2 + 305).contains(&until),
                "until={until}, t2={t2}"
            );
        }

        // 200 成功で streak とクールダウンがリセットされる
        lock(&api.state).transient_cooldown_until_sec = None;
        assert!(api
            .get_json(QuotaBucket::Shared, "/videos", &[("part", "id")])
            .await
            .is_ok());
        assert_eq!(lock(&api.state).transient_streak, 0);
        assert!(lock(&api.state).transient_cooldown_until_sec.is_none());
    }

    /// DB-YA-06: `parse_iso8601_duration` がライブ用 `"P0D"` / `"PT0S"` および `"P#DT#H#M#S"` を解釈し、
    /// 空形式・日付単位 `Y`/`M`/`W`・負数・逆順・オーバーフローを拒否することを検証する。
    #[test]
    fn parse_iso8601_duration_valid_and_invalid() {
        assert_eq!(parse_iso8601_duration("P0D"), Some(0));
        assert_eq!(parse_iso8601_duration("PT0S"), Some(0));
        assert_eq!(parse_iso8601_duration("P0DT0H0M0S"), Some(0));
        assert_eq!(parse_iso8601_duration("PT45S"), Some(45));
        assert_eq!(parse_iso8601_duration("PT15M33S"), Some(933));
        assert_eq!(parse_iso8601_duration("PT1H"), Some(3_600));
        assert_eq!(parse_iso8601_duration("P2D"), Some(172_800));
        assert_eq!(parse_iso8601_duration("P1DT2H3M4S"), Some(93_784));
        assert_eq!(parse_iso8601_duration("P1DT2S"), Some(86_402));

        for invalid in [
            "",
            "P",
            "PT",
            "P1DT",
            "15M33S",
            "P1Y",
            "P1M",
            "P1W",
            "P1Y2D",
            "PT-5S",
            "-PT5S",
            "PT5S10M",
            "PT1H2H",
            "PT1.5S",
            "PT15M33",
            "P999999999999999999D",
        ] {
            assert_eq!(
                parse_iso8601_duration(invalid),
                None,
                "expected None for {invalid:?}"
            );
        }
    }

    /// DB-YA-07: `test_connection` が `GET /videos?part=id&id=dQw4w9WgXcQ`（1 unit）を送り、
    /// 未保存の候補キーテストでは保存済みキー状態を汚さず、保存済みキーの失敗時は
    /// キー値を漏らさず `YtApiStatus` へ反映することを検証する。
    #[tokio::test]
    async fn test_connection_isolated_candidate_and_saved_key_outcomes() {
        let saved_key = "AIzaSySAVED_VALID_KEY";
        let candidate_bad = "AIzaSyCANDIDATE_BAD_KEY";
        let (base, recorded) = spawn_mock_server(vec![
            MockReply {
                status: 400,
                body: r#"{"error":{"errors":[{"reason":"keyInvalid"}]}}"#.to_string(),
                delay: Duration::ZERO,
            },
            MockReply {
                status: 200,
                body: r#"{"items":[{"id":"dQw4w9WgXcQ"}]}"#.to_string(),
                delay: Duration::ZERO,
            },
            MockReply {
                status: 401,
                body: "unauthorized".to_string(),
                delay: Duration::ZERO,
            },
        ])
        .await;

        let api = YtApi::with_test_base_url(Some(saved_key.to_string()), base);

        // 未保存の不正候補キーをテストしても保存済みキーは Active のまま
        let cand_res = api.test_connection(Some(candidate_bad)).await;
        assert!(!cand_res.ok);
        let cand_reason = cand_res.reason.unwrap();
        assert!(!cand_reason.contains(candidate_bad), "{cand_reason}");
        assert_eq!(api.status().state, YtApiAvailability::Active);

        // 保存済みキーのテスト成功
        let ok_res = api.test_connection(None).await;
        assert!(ok_res.ok, "{ok_res:?}");
        assert!(ok_res.reason.is_none());

        // 保存済みキー（一致する文字列経由）で 401 が返ると KeyInvalid が記録され、キー文字列は漏れない
        let fail_res = api.test_connection(Some(saved_key)).await;
        assert!(!fail_res.ok);
        let fail_reason = fail_res.reason.unwrap();
        assert!(!fail_reason.contains(saved_key), "{fail_reason}");
        assert_eq!(api.status().state, YtApiAvailability::KeyInvalid);

        // リクエストパス検証
        {
            let reqs = recorded.lock().unwrap();
            assert_eq!(reqs.len(), 3);
            for req in reqs.iter() {
                assert!(
                    req.request_line
                        .starts_with("GET /videos?part=id&id=dQw4w9WgXcQ "),
                    "{}",
                    req.request_line
                );
            }
        }

        // 未設定キーはネットワーク呼び出しなしで即失敗
        let unset_api = YtApi::new(None);
        let unset_res = unset_api.test_connection(None).await;
        assert!(!unset_res.ok);
        assert_eq!(unset_api.status().state, YtApiAvailability::Unset);
    }

    /// DB-YA-08: `iso8601_ms` / `iso_utc` の `+00:00` 正規化とタイムアウト時のフォールバック安全性を検証する。
    #[tokio::test]
    async fn iso_normalization_and_client_timeout() {
        let ms = iso8601_ms("2026-05-01T21:00:00.750+09:00").unwrap();
        assert_eq!(iso_utc(ms), "2026-05-01T12:00:00+00:00");
        assert_eq!(
            iso8601_ms("2026-05-01T12:00:00Z").map(iso_utc).as_deref(),
            Some("2026-05-01T12:00:00+00:00")
        );

        let key = "AIzaSyTIMEOUT_KEY";
        let (base, _) = spawn_mock_server(vec![MockReply {
            status: 200,
            body: r#"{"items":[]}"#.to_string(),
            delay: Duration::from_millis(350),
        }])
        .await;
        let api = YtApi::with_test_base_url(Some(key.to_string()), base)
            .with_test_timeouts(Duration::from_millis(100), Duration::from_millis(100));

        let out = api.test_connection(None).await;
        assert!(!out.ok);
        let reason = out.reason.unwrap();
        assert!(!reason.contains(key), "{reason}");
        assert!(lock(&api.state).transient_streak >= 1);

        // ヘッダー到着後に本文受信でタイムアウト・中途切断した場合も、
        // 日次クォータ超過に化けず一過性失敗（YtApiError::Http）となる
        for (status_line, truncate_immediately) in [
            ("200 OK", false),
            ("403 Forbidden", false),
            ("403 Forbidden", true),
        ] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let status_owned = status_line.to_string();
            let body_started = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let body_started_srv = std::sync::Arc::clone(&body_started);
            tokio::spawn(async move {
                if let Ok((mut stream, _)) = listener.accept().await {
                    let mut buf = [0u8; 1024];
                    let _ = stream.read(&mut buf).await;
                    let head = format!(
                        "HTTP/1.1 {status_owned}\r\nContent-Type: application/json\r\nContent-Length: 32\r\nConnection: close\r\n\r\n{{\"error\":"
                    );
                    let _ = stream.write_all(head.as_bytes()).await;
                    let _ = stream.flush().await;
                    body_started_srv.store(true, std::sync::atomic::Ordering::SeqCst);
                    if !truncate_immediately {
                        tokio::time::sleep(Duration::from_millis(350)).await;
                    }
                    let _ = stream.shutdown().await;
                }
            });

            let body_api =
                YtApi::with_test_base_url(Some(key.to_string()), format!("http://{addr}"))
                    .with_test_timeouts(Duration::from_millis(100), Duration::from_millis(100));
            let err = body_api
                .get_json(QuotaBucket::Shared, "/videos", &[("part", "id")])
                .await
                .unwrap_err();
            assert!(
                body_started.load(std::sync::atomic::Ordering::SeqCst),
                "server did not reach body phase: {status_line}"
            );
            match &err {
                YtApiError::Http(e) => {
                    if truncate_immediately {
                        assert!(
                            e.is_body() || e.is_decode(),
                            "expected body read/decode error, got: {e}"
                        );
                        assert!(!e.is_timeout(), "unexpected timeout on EOF: {e}");
                    } else {
                        assert!(e.is_timeout(), "expected body timeout error, got: {e}");
                    }
                }
                other => panic!("expected YtApiError::Http for {status_line}, got: {other:?}"),
            }
            assert_eq!(
                body_api.status().state,
                YtApiAvailability::Active,
                "body error must not trip quota breaker: {status_line}"
            );
            let st = lock(&body_api.state);
            assert!(st.shared_quota_until_sec.is_none());
            assert!(st.transient_streak >= 1);
            assert!(st.transient_cooldown_until_sec.is_some());
        }
    }

    /// DB-YA-09: `videos_list` が空入力を無通信で処理し、重複除去と 50 件チャンク分割を行って全件を返すことを検証する。
    #[tokio::test]
    async fn videos_list_dedup_and_50_item_chunking() {
        let key = "AIzaSyVIDEOS_BATCH_09";
        let make_item = |id: &str| {
            format!(
                r#"{{"id":"{id}","snippet":{{"title":"Title {id}","channelId":"UC000000000000000000001","channelTitle":"CH","publishedAt":"2026-11-27T10:00:00Z","description":"desc {id}","liveBroadcastContent":"none"}},"contentDetails":{{"duration":"PT3M20S"}}}}"#
            )
        };
        let chunk1: Vec<String> = (0..50).map(|i| make_item(&format!("vid_{i:03}"))).collect();
        let chunk2: Vec<String> = (50..55)
            .map(|i| make_item(&format!("vid_{i:03}")))
            .collect();
        let (base, recorded) = spawn_mock_server(vec![
            MockReply {
                status: 200,
                body: format!(r#"{{"items":[{}]}}"#, chunk1.join(",")),
                delay: Duration::ZERO,
            },
            MockReply {
                status: 200,
                body: format!(r#"{{"items":[{}]}}"#, chunk2.join(",")),
                delay: Duration::ZERO,
            },
        ])
        .await;

        let api = YtApi::with_test_base_url(Some(key.to_string()), base);

        // 空配列・空白のみは通信 0 回
        let empty = api.videos_list(&["", "   "]).await.unwrap();
        assert!(empty.is_empty());
        assert_eq!(recorded.lock().unwrap().len(), 0);

        // 重複・空白混在の 55 件を 50 + 5 の 2 回で取得
        let mut ids: Vec<String> = (0..55).map(|i| format!("vid_{i:03}")).collect();
        ids.push("vid_000".to_string());
        ids.push(" vid_054 ".to_string());
        ids.push("   ".to_string());

        let map = api.videos_list(&ids).await.unwrap();
        assert_eq!(map.len(), 55);
        assert_eq!(map["vid_000"].video_ref.title, "Title vid_000");
        assert_eq!(map["vid_054"].duration_sec, Some(200));

        let reqs = recorded.lock().unwrap();
        assert_eq!(reqs.len(), 2);
        assert!(reqs[0]
            .request_line
            .contains("part=snippet%2CcontentDetails%2CliveStreamingDetails%2Cstatistics"));
        assert!(reqs[0]
            .headers_lower
            .contains(&format!("x-goog-api-key: {key}").to_ascii_lowercase()));
        assert!(!reqs[0].request_line.contains("key="));
        // 1 回目に 50 ID、2 回目に 5 ID（カンマ %2C が 49 個と 4 個）
        let id_part_0 = reqs[0]
            .request_line
            .split("id=")
            .nth(1)
            .unwrap()
            .split(' ')
            .next()
            .unwrap();
        let id_part_1 = reqs[1]
            .request_line
            .split("id=")
            .nth(1)
            .unwrap()
            .split(' ')
            .next()
            .unwrap();
        assert_eq!(id_part_0.matches("%2C").count(), 49);
        assert_eq!(id_part_1.matches("%2C").count(), 4);
    }

    /// DB-YA-10: `videos_list` が `publishedAt` を `+00:00` へ正規化し、`duration_sec`・サムネイル優先順・説明文を正しく変換することを検証する。
    #[tokio::test]
    async fn videos_list_normalizes_timestamps_duration_and_thumbnails() {
        let body = r#"{
            "items": [
                {
                    "id": "norm_vid_01",
                    "snippet": {
                        "title": "  Normal Video  ",
                        "channelId": "UCnorm00000000000000001",
                        "channelTitle": "Norm CH",
                        "publishedAt": "2026-11-27T21:05:09.123+09:00",
                        "description": "hello world",
                        "liveBroadcastContent": "none",
                        "thumbnails": {
                            "default": {"url": "https://i.ytimg.com/vi/norm_vid_01/default.jpg"},
                            "high": {"url": "https://i.ytimg.com/vi/norm_vid_01/hqdefault.jpg"},
                            "maxres": {"url": "https://i.ytimg.com/vi/norm_vid_01/maxresdefault.jpg"}
                        }
                    },
                    "contentDetails": {"duration": "PT1H2M3S"}
                },
                {
                    "id": "norm_vid_02",
                    "snippet": {
                        "title": "Fallback Thumb",
                        "publishedAt": "2026-11-27T12:00:00Z",
                        "liveBroadcastContent": "none"
                    },
                    "contentDetails": {"duration": "P1DT0S"}
                }
            ]
        }"#;
        let (base, _) = spawn_mock_server(vec![MockReply {
            status: 200,
            body: body.to_string(),
            delay: Duration::ZERO,
        }])
        .await;

        let api = YtApi::with_test_base_url(Some("AIzaSyNORM_10".to_string()), base);
        let map = api
            .videos_list(&["norm_vid_01", "norm_vid_02"])
            .await
            .unwrap();

        let v1 = &map["norm_vid_01"];
        assert_eq!(v1.video_ref.title, "Normal Video");
        assert_eq!(
            v1.video_ref.channel_id.as_deref(),
            Some("UCnorm00000000000000001")
        );
        assert_eq!(v1.video_ref.channel_title.as_deref(), Some("Norm CH"));
        assert_eq!(
            v1.video_ref.thumbnail_url.as_deref(),
            Some("https://i.ytimg.com/vi/norm_vid_01/maxresdefault.jpg")
        );
        assert_eq!(
            v1.meta.published_at.as_deref(),
            Some("2026-11-27T12:05:09+00:00")
        );
        assert_eq!(v1.meta.description, "hello world");
        assert_eq!(v1.duration_sec, Some(3723));
        assert_eq!(v1.meta.is_live_content, Some(false));
        assert_eq!(v1.classify_kind_for_initial("video"), None);

        let v2 = &map["norm_vid_02"];
        assert_eq!(
            v2.video_ref.thumbnail_url.as_deref(),
            Some("https://i.ytimg.com/vi/norm_vid_02/hqdefault.jpg")
        );
        assert_eq!(
            v2.meta.published_at.as_deref(),
            Some("2026-11-27T12:00:00+00:00")
        );
        assert_eq!(v2.duration_sec, Some(86_400));
    }

    /// DB-YA-11: ガード 4（`P0D`/`PT0S` は `live`、尺 > 0 は `premiere`、尺欠落ライブ中は `live`、`actualEndTime` 時は `is_live_content = None` と `premiere` 保護）を検証する。
    #[tokio::test]
    async fn videos_list_guard4_live_premiere_and_ended_preservation() {
        let body = r#"{
            "items": [
                {
                    "id": "live_p0d",
                    "snippet": {"title": "Live P0D", "liveBroadcastContent": "live"},
                    "contentDetails": {"duration": "P0D"},
                    "liveStreamingDetails": {"actualStartTime": "2026-11-27T10:00:00Z"}
                },
                {
                    "id": "live_nodur",
                    "snippet": {"title": "Live NoDur", "liveBroadcastContent": "live"},
                    "liveStreamingDetails": {"actualStartTime": "2026-11-27T10:00:00Z"}
                },
                {
                    "id": "prem_live",
                    "snippet": {"title": "Premiere Live", "liveBroadcastContent": "live"},
                    "contentDetails": {"duration": "PT5M30S"},
                    "liveStreamingDetails": {"actualStartTime": "2026-11-27T10:00:00Z"}
                },
                {
                    "id": "prem_up",
                    "snippet": {"title": "Premiere Upcoming", "liveBroadcastContent": "upcoming"},
                    "contentDetails": {"duration": "PT5M30S"},
                    "liveStreamingDetails": {"scheduledStartTime": "2026-11-27T11:00:00Z"}
                },
                {
                    "id": "ended_arc",
                    "snippet": {"title": "Ended Stream", "liveBroadcastContent": "none"},
                    "contentDetails": {"duration": "PT1H10M"},
                    "liveStreamingDetails": {
                        "actualStartTime": "2026-11-27T08:00:00Z",
                        "actualEndTime": "2026-11-27T09:10:00Z"
                    }
                }
            ]
        }"#;
        let (base, _) = spawn_mock_server(vec![MockReply {
            status: 200,
            body: body.to_string(),
            delay: Duration::ZERO,
        }])
        .await;

        let api = YtApi::with_test_base_url(Some("AIzaSyGUARD4_11".to_string()), base);
        let map = api
            .videos_list(&[
                "live_p0d",
                "live_nodur",
                "prem_live",
                "prem_up",
                "ended_arc",
            ])
            .await
            .unwrap();

        assert_eq!(map["live_p0d"].meta.is_live_content, Some(true));
        assert_eq!(
            map["live_p0d"].classify_kind_for_initial("video"),
            Some("live")
        );
        assert_eq!(map["live_p0d"].classify_kind_for_refresh(), Some("live"));

        // 尺が欠落した配信中ライブも premiere に誤分類されず live になる
        assert_eq!(map["live_nodur"].meta.is_live_content, None);
        assert_eq!(
            map["live_nodur"].classify_kind_for_initial("video"),
            Some("live")
        );
        assert_eq!(map["live_nodur"].classify_kind_for_refresh(), Some("live"));

        assert_eq!(map["prem_live"].meta.is_live_content, Some(false));
        assert_eq!(
            map["prem_live"].classify_kind_for_initial("video"),
            Some("premiere")
        );
        assert_eq!(
            map["prem_live"].classify_kind_for_refresh(),
            Some("premiere")
        );

        assert_eq!(map["prem_up"].meta.is_live_content, Some(false));
        assert_eq!(
            map["prem_up"].classify_kind_for_initial("video"),
            Some("upcoming")
        );
        assert_eq!(map["prem_up"].classify_kind_for_refresh(), Some("upcoming"));

        // 終了後（actualEndTime あり）は is_live_content = None で、
        // refresh は None（既存保持）、initial は未確定なら live / 既存 premiere は None
        let ended = &map["ended_arc"];
        assert_eq!(ended.meta.is_live_content, None);
        assert_eq!(ended.classify_kind_for_refresh(), None);
        assert_eq!(ended.classify_kind_for_initial("video"), Some("live"));
        assert_eq!(ended.classify_kind_for_initial("premiere"), None);
    }

    /// DB-YA-12: 未設定・`keyInvalid`・日次クォータ超過・一時エラー（429）・HTTP 404 における `should_fallback()` と `can_use(Shared)` 遷移を検証する。
    #[tokio::test]
    async fn videos_list_error_and_fallback_signalling() {
        // 未設定
        let unset = YtApi::new(None);
        assert!(!unset.can_use(QuotaBucket::Shared));
        let err_unset = unset.videos_list(&["dQw4w9WgXcQ"]).await.unwrap_err();
        assert!(matches!(err_unset, YtApiError::Unset));
        assert!(err_unset.should_fallback());

        // 404 は遮断器を開かず個別フォールバックのみ通知
        let (base_404, _) = spawn_mock_server(vec![MockReply {
            status: 404,
            body: r#"{"error":{"code":404}}"#.to_string(),
            delay: Duration::ZERO,
        }])
        .await;
        let api_404 = YtApi::with_test_base_url(Some("AIzaSyERR_404".to_string()), base_404);
        assert!(api_404.can_use(QuotaBucket::Shared));
        let err_404 = api_404.videos_list(&["dQw4w9WgXcQ"]).await.unwrap_err();
        assert!(matches!(err_404, YtApiError::NotFound));
        assert!(err_404.should_fallback());
        assert!(api_404.can_use(QuotaBucket::Shared));

        // 429 でクールダウンに入り can_use(Shared) == false
        let (base_429, _) = spawn_mock_server(vec![MockReply {
            status: 429,
            body: r#"{"error":{"code":429}}"#.to_string(),
            delay: Duration::ZERO,
        }])
        .await;
        let api_429 = YtApi::with_test_base_url(Some("AIzaSyERR_429".to_string()), base_429);
        let err_429 = api_429.videos_list(&["dQw4w9WgXcQ"]).await.unwrap_err();
        assert!(matches!(err_429, YtApiError::Status(429)));
        assert!(err_429.should_fallback());
        assert!(!api_429.can_use(QuotaBucket::Shared));

        // 403 quotaExceeded で Shared クォータ遮断に入り can_use(Shared) == false
        let (base_q, _) = spawn_mock_server(vec![MockReply {
            status: 403,
            body: r#"{"error":{"errors":[{"reason":"quotaExceeded"}]}}"#.to_string(),
            delay: Duration::ZERO,
        }])
        .await;
        let api_q = YtApi::with_test_base_url(Some("AIzaSyERR_Q".to_string()), base_q);
        let err_q = api_q.videos_list(&["dQw4w9WgXcQ"]).await.unwrap_err();
        assert!(matches!(
            err_q,
            YtApiError::QuotaExhausted(QuotaBucket::Shared)
        ));
        assert!(err_q.should_fallback());
        assert!(!api_q.can_use(QuotaBucket::Shared));

        // 400 keyInvalid でラッチに入り can_use(Shared) == false
        let (base_k, _) = spawn_mock_server(vec![MockReply {
            status: 400,
            body: r#"{"error":{"errors":[{"reason":"keyInvalid"}]}}"#.to_string(),
            delay: Duration::ZERO,
        }])
        .await;
        let api_k = YtApi::with_test_base_url(Some("AIzaSyERR_K".to_string()), base_k);
        let err_k = api_k.videos_list(&["dQw4w9WgXcQ"]).await.unwrap_err();
        assert!(matches!(err_k, YtApiError::KeyInvalid));
        assert!(err_k.should_fallback());
        assert!(!api_k.can_use(QuotaBucket::Shared));
    }
}
