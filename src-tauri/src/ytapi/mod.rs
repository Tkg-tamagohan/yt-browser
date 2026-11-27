//! YouTube Data API v3 クライアント基盤（FR-97、仕様決定 EC）。
//!
//! 3 層一方向依存（`model` / `util` のみに依存する純粋リーフドメインモジュール）を守り、
//! 任意設定 `youtube.api_key` が有効な間だけ公式 API を利用するための認証ヘッダ付与、
//! 独立 2 系統（共通枠 / `search.list` 枠）サーキットブレーカー、`key_invalid` ラッチ、
//! 指数バックオフ型クールダウン、および接続テストを提供する。

use std::fmt;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::header::{HeaderName, HeaderValue};
use reqwest::{Client, StatusCode};
use serde_json::Value;
use thiserror::Error;
use tracing::warn;

use crate::model::{YtApiAvailability, YtApiStatus, YtApiTestOutcome};
use crate::util::lock;

/// YouTube Data API v3 キーの設定キー名（38 個目の設定キー、FR-97）。
pub const SETTING_YOUTUBE_API_KEY: &str = "youtube.api_key";

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
    #[allow(dead_code)] // Phase 93〜95 の各ドメイン経路から参照する
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

    /// 保存済みキーと指定バケットを使って GET リクエストを送る（遮断器チェック込み）。
    #[allow(dead_code)] // Phase 93〜95 の API 経路から使用し、Phase 92 ではテストでも検証する
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
        if status.is_success() {
            let value = resp
                .json::<Value>()
                .await
                .map_err(|e| YtApiError::Http(e.without_url()))?;
            if mutate_saved_state {
                self.record_success(api_key, bucket);
            }
            return Ok(value);
        }

        let body = resp.text().await.unwrap_or_default();
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
            && reasons.iter().any(|r| r == "keyInvalid"))
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
    }
}
