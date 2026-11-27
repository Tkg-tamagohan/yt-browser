//! メタの遅延解決基盤（FR-53・FR-56・FR-57・FR-85、仕様決定 CD・CG・CH・DJ、
//! 設計書 §4.18）。
//!
//! RSS・バックフィル・ライブラリ登録で欠けた `videos` の投稿日時（`published_at`）・
//! 種別（`kind`）・説明文（`description`）を、InnerTube `player` 応答で後から
//! 埋める常駐ワーカー。`video_meta_resolve` と同じ `player` 経路を使い、
//! 投稿日時・放送状態・再生状態・説明文を 1 応答から取る（仕様決定 CD・CG の
//! 共用方針。説明文の遅延保存は FR-85・仕様決定 DJ で追加）。kind の試行失敗は
//! 定期的に再試行する（仕様決定 CH）。処理量はバッチ件数と巡回間隔で上限化する（CD）。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tauri::{AppHandle, Emitter};
use thiserror::Error;
use tokio::sync::Notify;
use tracing::{debug, warn};

use crate::db::{Db, DbError, MetaResolveTarget, ResolveMetaParams};
use crate::innertube::{is_transient_or_blocked_playability, InnerTube, InnerTubeError};
use crate::model::{classify_broadcast_meta, classify_kind, FeedNewItems};
use crate::util::lock;

/// 巡回間隔。フィードの新着投入時には `wake_now` で即時起こされる。
const TICK: Duration = Duration::from_secs(60);
/// 1 サイクルで処理する未解決行数の上限（仕様決定 CD の処理量の上限化）。
const BATCH_MAX: usize = 20;
/// 1 サイクルで再確認するアクティブ放送行数の上限（FR-96、仕様決定 EB）。
const REFRESH_BATCH_MAX: usize = 10;
/// 手動「今すぐ更新」に伴う強制再確認のグローバルクールダウン（FR-96）。
const MANUAL_COOLDOWN: Duration = Duration::from_secs(60);
/// 1 サイクル内の連続通信失敗によるサーキットブレーカー閾値（FR-96）。
const CONSECUTIVE_FAIL_BREAKER: u32 = 3;
/// `live` 行に対する連続 `LOGIN_REQUIRED` / `ERROR` による個別解除閾値（FR-96）。
const BLOCKED_LIVE_THRESHOLD: u32 = 6;
/// HTTP 429 検知時の指数バックオフ秒数（5 分 → 10 分 → 15 分上限、FR-96）。
const RATE_LIMIT_STEPS_SECS: &[u64] = &[300, 600, 900];
/// DB から一度に取る候補数。起動内失敗上限 `FAIL_MAX` に達した行を
/// 飛ばしてもバッチが埋まるよう多めに取る。
const CANDIDATES: usize = BATCH_MAX * 5;
/// 1 起動内で同一行が連続失敗する上限。超えた行はその起動中は再試行
/// しない（DB 上は失敗（2）のまま残り、次回起動で再び対象になる。
/// 恒久に失敗し続ける行が選出順を占有しないための抑止）。
const FAIL_MAX: u32 = 5;
/// 行ごとの処理間隔。直列に捌きつつリクエスト流量を抑える。
const ITEM_SPACING: Duration = Duration::from_millis(200);
/// shorts 判定 1 件の上限時間（feed の detect_shorts と同値、仕様決定 V）。
const SHORTS_TIMEOUT: Duration = Duration::from_secs(8);
/// shorts 判定の URL 先頭（video_id を後置）。
const SHORTS_URL: &str = "https://www.youtube.com/shorts/";

/// `videos.kind_state` の値（仕様決定 CG・CH）。
/// 0=未確定（スキーマ既定、コードでは参照しない）/ 1=確定 / 2=直近の試行が失敗。
pub const KIND_RESOLVED: i64 = 1;
pub const KIND_FAILED: i64 = 2;

fn epoch_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[derive(Debug, Error)]
enum ResolveError {
    #[error("InnerTube メタ取得に失敗: {0}")]
    Tube(#[from] InnerTubeError),
    #[error("InnerTube が一時制限・ボット判定応答を返した")]
    TransientBlocked,
    #[error("DB 更新に失敗: {0}")]
    Db(#[from] DbError),
    #[error("shorts 判定リクエストに失敗: {0}")]
    Shorts(reqwest::Error),
}

/// メタの遅延解決およびアクティブ放送再確認の中核状態（`AppHandle` 非依存で単体テスト可能）。
pub(crate) struct ResolverCore {
    db: Db,
    inner: Arc<InnerTube>,
    /// shorts 判定用クライアント（仕様決定 V）。feed の detect_client と同じく
    /// 非 short は /watch へ 303 で転送されるためリダイレクトを追跡しない。
    detect_client: reqwest::Client,
    /// 新規投入などで即時回したいときの通知（feed の shorts 判定起動と併せて鳴らす）。
    wake: Notify,
    /// この起動内での video_id → 連続失敗回数。`FAIL_MAX` 以上は選出から外す
    /// （失敗行が選出順を占有して新しい行へ届かなくなるのを防ぐ）。
    fails: Mutex<HashMap<String, u32>>,
    /// HTTP 429 検知時のクールダウン終了時刻と連続検知回数（初回・再確認で共有）。
    rate_limit_until: Mutex<Option<Instant>>,
    rate_limit_streak: Mutex<u32>,
    /// 手動「今すぐ更新」からの即時再確認要求と前回強制実行時刻。
    manual_pending: Mutex<bool>,
    last_manual_at: Mutex<Option<Instant>>,
    /// `live` 行に対する連続 `LOGIN_REQUIRED` / `ERROR` 回数。
    blocked_live_streaks: Mutex<HashMap<String, u32>>,
}

/// メタの遅延解決およびアクティブ放送再確認ワーカー（仕様決定 CD・CG・CH・DJ・EB）。
/// `run` を起動時に spawn する。
pub struct MetaResolver {
    core: ResolverCore,
    app: AppHandle,
}

impl MetaResolver {
    pub fn new(db: Db, app: AppHandle, innertube: Arc<InnerTube>) -> Arc<Self> {
        Arc::new(Self {
            core: ResolverCore::new(db, innertube),
            app,
        })
    }

    /// 新規投入などでワーカーを即時回したいときに呼ぶ。
    pub fn wake_now(&self) {
        self.core.wake_now();
    }

    /// 手動「今すぐ更新」から呼ぶ（60 秒のグローバルクールダウンを満たす場合のみ
    /// アクティブ放送の間隔ガードをバイパスして即時再確認する。FR-96、仕様決定 EB）。
    pub fn wake_manual(&self) {
        self.core.wake_manual();
    }

    /// メインループ。TICK ごと（または wake で即時）に対象行を処理する。
    /// 対象ゼロの間は次の契機まで眠る。
    pub async fn run(self: Arc<Self>) {
        loop {
            tokio::select! {
                _ = self.core.wake.notified() => {},
                _ = tokio::time::sleep(TICK) => {},
            }
            let changed = self.core.resolve_once(Instant::now(), epoch_ms()).await;
            if changed > 0 {
                let payload = FeedNewItems { count: changed };
                if let Err(e) = self.app.emit("feed://kind_updated", payload) {
                    warn!(error = %e, "feed://kind_updated の送出に失敗");
                }
            }
        }
    }
}

impl ResolverCore {
    pub(crate) fn new(db: Db, innertube: Arc<InnerTube>) -> Self {
        Self {
            db,
            inner: innertube,
            detect_client: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                // 検出は応答の有無だけ見るため接続確立を短く打ち切る
                .connect_timeout(Duration::from_secs(5))
                .build()
                .expect("リダイレクト非追跡クライアントの生成に失敗"),
            wake: Notify::new(),
            fails: Mutex::new(HashMap::new()),
            rate_limit_until: Mutex::new(None),
            rate_limit_streak: Mutex::new(0),
            manual_pending: Mutex::new(false),
            last_manual_at: Mutex::new(None),
            blocked_live_streaks: Mutex::new(HashMap::new()),
        }
    }

    pub(crate) fn wake_now(&self) {
        self.wake.notify_one();
    }

    pub(crate) fn wake_manual(&self) {
        *lock(&self.manual_pending) = true;
        self.wake.notify_one();
    }

    fn is_rate_limited_at(&self, now: Instant) -> bool {
        lock(&self.rate_limit_until).is_some_and(|until| now < until)
    }

    fn record_rate_limit_at(&self, now: Instant) {
        let mut streak = lock(&self.rate_limit_streak);
        let idx = (*streak as usize).min(RATE_LIMIT_STEPS_SECS.len() - 1);
        let wait = Duration::from_secs(RATE_LIMIT_STEPS_SECS[idx]);
        *streak = streak.saturating_add(1);
        *lock(&self.rate_limit_until) = Some(now + wait);
        warn!(
            backoff_secs = wait.as_secs(),
            streak = *streak,
            "InnerTube 429 を検知したため遅延解決を一時停止します"
        );
    }

    fn clear_rate_limit(&self) {
        *lock(&self.rate_limit_streak) = 0;
        *lock(&self.rate_limit_until) = None;
    }

    fn take_manual_force_at(&self, now: Instant) -> bool {
        let mut pending = lock(&self.manual_pending);
        if !*pending {
            return false;
        }
        *pending = false;
        let mut last = lock(&self.last_manual_at);
        if last.is_none_or(|t| now.duration_since(t) >= MANUAL_COOLDOWN) {
            *last = Some(now);
            true
        } else {
            false
        }
    }

    fn record_blocked_live(&self, video_id: &str) -> bool {
        let mut map = lock(&self.blocked_live_streaks);
        let count = map.entry(video_id.to_string()).or_insert(0);
        *count += 1;
        if *count >= BLOCKED_LIVE_THRESHOLD {
            map.remove(video_id);
            true
        } else {
            false
        }
    }

    /// 1 サイクル分の解決およびアクティブ放送の再確認。
    /// UI 更新を要する変更件数を返す。
    pub(crate) async fn resolve_once(&self, now_instant: Instant, now_ms: i64) -> usize {
        if self.is_rate_limited_at(now_instant) {
            return 0;
        }
        let cycle_started = Instant::now();
        let mut changed = 0usize;
        let mut rate_limited = false;

        match self.db.meta_resolve_targets(CANDIDATES) {
            Ok(all_targets) => {
                let targets: Vec<MetaResolveTarget> = {
                    let fails = lock(&self.fails);
                    all_targets
                        .into_iter()
                        .filter(|t| fails.get(&t.video_id).copied().unwrap_or(0) < FAIL_MAX)
                        .take(BATCH_MAX)
                        .collect()
                };
                for t in &targets {
                    match self.resolve_one(t, now_ms).await {
                        Ok(n) => {
                            changed += n;
                            lock(&self.fails).remove(&t.video_id);
                            self.clear_rate_limit();
                        }
                        Err(e) => {
                            let is_429 = matches!(
                                &e,
                                ResolveError::Tube(te) if te.is_rate_limited()
                            );
                            if is_429 {
                                self.record_rate_limit_at(now_instant + cycle_started.elapsed());
                                rate_limited = true;
                                break;
                            }
                            *lock(&self.fails).entry(t.video_id.clone()).or_insert(0) += 1;
                            if t.needs_kind {
                                if let Err(e2) =
                                    self.db.videos_set_kind_state(&t.video_id, KIND_FAILED)
                                {
                                    warn!(error = %e2, "kind 失敗状態の記録に失敗");
                                }
                            }
                            debug!(video_id = %t.video_id, error = %e, "遅延解決の試行に失敗");
                        }
                    }
                    tokio::time::sleep(ITEM_SPACING).await;
                }
            }
            Err(e) => {
                warn!(error = %e, "遅延解決対象の選出に失敗");
            }
        }

        if !rate_limited {
            changed += self
                .refresh_broadcasts_once(now_instant, cycle_started, now_ms)
                .await;
        }
        changed
    }

    /// 1 行分の解決。InnerTube メタ応答 1 件で published_at・kind・
    /// description・broadcast_* を単一トランザクションで埋める（仕様決定 CD・CG・DJ・EB）。
    async fn resolve_one(&self, t: &MetaResolveTarget, now_ms: i64) -> Result<usize, ResolveError> {
        if !(t.needs_published || t.needs_kind || t.needs_desc) {
            return Ok(0);
        }
        let s = self.inner.meta_snapshot(&t.video_id).await?;
        if is_transient_or_blocked_playability(&s) {
            return Err(ResolveError::TransientBlocked);
        }

        let resolved_kind = if t.needs_kind {
            classify_kind(&s)
        } else {
            None
        };
        let (b_state, b_start) = classify_broadcast_meta(&s);
        let mut changed = 0usize;

        if self.db.videos_resolve_meta_tx(&ResolveMetaParams {
            video_id: &t.video_id,
            needs_published: t.needs_published,
            published_at: s.published_at.as_deref(),
            needs_desc: t.needs_desc,
            description: &s.description,
            needs_kind: t.needs_kind,
            resolved_kind,
            broadcast_state: b_state,
            broadcast_start_ms: b_start,
            now_ms,
        })? {
            changed += 1;
        }

        if t.needs_kind && resolved_kind.is_none() && t.kind == "video" {
            match self.shorts_probe(&t.video_id).await {
                Ok(true) => {
                    if self.db.videos_resolve_kind(&t.video_id, "short")? {
                        changed += 1;
                    }
                }
                Ok(false) => {
                    self.db.videos_set_kind_state(&t.video_id, KIND_RESOLVED)?;
                }
                Err(e) => return Err(e),
            }
        }
        Ok(changed)
    }

    /// アクティブな放送行（`live` / `upcoming`）の 7 日超過打ち切りと定期再確認（FR-96、仕様決定 EB）。
    async fn refresh_broadcasts_once(
        &self,
        now_instant: Instant,
        cycle_started: Instant,
        now_ms: i64,
    ) -> usize {
        let mut changed = match self.db.videos_expire_stale_upcoming(now_ms) {
            Ok(n) => n,
            Err(e) => {
                warn!(error = %e, "期限切れ配信予定の解除に失敗");
                0
            }
        };
        let force_manual = self.take_manual_force_at(now_instant);
        let targets =
            match self
                .db
                .broadcast_refresh_targets(now_ms, force_manual, REFRESH_BATCH_MAX)
            {
                Ok(t) => t,
                Err(e) => {
                    warn!(error = %e, "再確認対象の選出に失敗");
                    return changed;
                }
            };
        if targets.is_empty() {
            return changed;
        }

        let mut consecutive_fails = 0u32;
        for t in targets {
            match self.inner.meta_snapshot(&t.video_id).await {
                Err(e) => {
                    let _ = self.db.videos_touch_broadcast_checked(&t.video_id, now_ms);
                    if e.is_rate_limited() {
                        self.record_rate_limit_at(now_instant + cycle_started.elapsed());
                        break;
                    }
                    consecutive_fails += 1;
                    debug!(video_id = %t.video_id, error = %e, "放送状態の再確認に失敗");
                    if consecutive_fails >= CONSECUTIVE_FAIL_BREAKER {
                        warn!("放送再確認の連続失敗上限に達したためサイクルを中断します");
                        break;
                    }
                }
                Ok(s) if is_transient_or_blocked_playability(&s) => {
                    consecutive_fails = 0;
                    if t.broadcast_state == "live" && self.record_blocked_live(&t.video_id) {
                        if let Ok(true) = self.db.videos_apply_broadcast_refresh(
                            &t.video_id,
                            None,
                            None,
                            None,
                            now_ms,
                            false,
                        ) {
                            changed += 1;
                        }
                    } else {
                        let _ = self.db.videos_touch_broadcast_checked(&t.video_id, now_ms);
                    }
                }
                Ok(s) => {
                    consecutive_fails = 0;
                    self.clear_rate_limit();
                    lock(&self.blocked_live_streaks).remove(&t.video_id);
                    let new_kind = classify_kind(&s);
                    let (b_state, b_start) = classify_broadcast_meta(&s);
                    match self.db.videos_apply_broadcast_refresh(
                        &t.video_id,
                        new_kind,
                        b_state,
                        b_start,
                        now_ms,
                        false,
                    ) {
                        Ok(true) => changed += 1,
                        Ok(false) => {}
                        Err(e) => {
                            warn!(video_id = %t.video_id, error = %e, "放送状態の更新に失敗");
                        }
                    }
                }
            }
            tokio::time::sleep(ITEM_SPACING).await;
        }
        changed
    }

    /// shorts 判定の再試行版（仕様決定 V・CH）。先頭応答が 200 なら short。
    /// それ以外の応答は非 short、通信失敗・タイムアウトは Err（呼び出し側が
    /// 失敗状態として記録し、次回巡回で再試行する）。
    async fn shorts_probe(&self, video_id: &str) -> Result<bool, ResolveError> {
        let url = format!("{SHORTS_URL}{video_id}");
        let resp = self
            .detect_client
            .head(&url)
            .timeout(SHORTS_TIMEOUT)
            .send()
            .await
            .map_err(ResolveError::Shorts)?;
        Ok(resp.status() == reqwest::StatusCode::OK)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{BroadcastStatus, MetaSnapshot};

    fn snap(
        is_live_now: Option<bool>,
        has_details: bool,
        start_ms: Option<i64>,
        end_ms: Option<i64>,
        is_upcoming: bool,
        is_live_content: Option<bool>,
        playability: Option<&str>,
    ) -> MetaSnapshot {
        MetaSnapshot {
            published_at: None,
            description: String::new(),
            broadcast: BroadcastStatus {
                is_live_now,
                start_ms,
                end_ms,
                scheduled_start_ms: None,
                has_details,
                is_upcoming,
            },
            is_live_content,
            playability: playability.map(|s| s.to_string()),
        }
    }

    /// MR-01: InnerTube メタ応答の放送系材料から kind を分類する
    /// （FR-56、仕様決定 CG）。通常動画の手掛かりが無いときは None。
    #[test]
    fn classify_kind_matrix() {
        // 放送中のライブ
        assert_eq!(
            classify_kind(&snap(
                Some(true),
                true,
                Some(1),
                None,
                false,
                Some(true),
                Some("OK")
            )),
            Some("live")
        );
        // 放送中のプレミア（通常動画資産の放送中）は premiere（暫定解釈）
        assert_eq!(
            classify_kind(&snap(
                Some(true),
                true,
                Some(1),
                None,
                false,
                Some(false),
                Some("OK")
            )),
            Some("premiere")
        );
        // 予定あり・未終了の放送詳細は upcoming
        assert_eq!(
            classify_kind(&snap(Some(false), true, Some(1), None, false, None, None)),
            Some("upcoming")
        );
        // isUpcoming 単独でも upcoming
        assert_eq!(
            classify_kind(&snap(None, false, None, None, true, None, None)),
            Some("upcoming")
        );
        // 終了済みの放送詳細で isLiveContent 真 → ライブのアーカイブ
        assert_eq!(
            classify_kind(&snap(
                Some(false),
                true,
                Some(1),
                Some(2),
                false,
                Some(true),
                Some("OK")
            )),
            Some("live")
        );
        // 終了済みの放送詳細で isLiveContent 偽/欠落 → 公開済みプレミア
        assert_eq!(
            classify_kind(&snap(
                Some(false),
                true,
                Some(1),
                Some(2),
                false,
                Some(false),
                Some("UNPLAYABLE")
            )),
            Some("premiere")
        );
        assert_eq!(
            classify_kind(&snap(
                Some(false),
                true,
                Some(1),
                Some(2),
                false,
                None,
                Some("OK")
            )),
            Some("premiere")
        );
        // 放送系の手掛かりが無い応答は None（呼び出し側が shorts 判定へ回す）
        assert_eq!(
            classify_kind(&snap(
                None,
                false,
                None,
                None,
                false,
                Some(false),
                Some("OK")
            )),
            None
        );
        assert_eq!(
            classify_kind(&snap(Some(false), false, None, None, false, None, None)),
            None
        );
    }

    /// MR-02: 詳細を持たない応答での補助材料（FR-56、仕様決定 CG）。
    /// isLiveContent 真だけで 'live'、LIVE_STREAM_OFFLINE だけで 'upcoming'。
    #[test]
    fn classify_kind_details_less() {
        // details を持たないライブアーカイブ資産
        assert_eq!(
            classify_kind(&snap(
                Some(false),
                false,
                None,
                None,
                false,
                Some(true),
                Some("OK")
            )),
            Some("live")
        );
        // details 未展開の未開始放送（再生状態の補助、暫定解釈）
        assert_eq!(
            classify_kind(&snap(
                None,
                false,
                None,
                None,
                false,
                Some(false),
                Some("LIVE_STREAM_OFFLINE")
            )),
            Some("upcoming")
        );
        // 再生不可・材料ゼロは None
        assert_eq!(
            classify_kind(&snap(
                None,
                false,
                None,
                None,
                false,
                Some(false),
                Some("UNPLAYABLE")
            )),
            None
        );
    }

    /// MR-03: 予定開始の表現差を分類側が気にしないこと（FR-56、仕様決定 CG）。
    /// startTimestamp / scheduledStartTimestamp のどちらでも has_details 真
    /// かつ未終了なら 'upcoming'（時刻の解釈は BroadcastStatus 側の責務）。
    #[test]
    fn classify_kind_scheduled_variants() {
        let mut s = snap(Some(false), true, Some(1_000), None, false, None, None);
        assert_eq!(classify_kind(&s), Some("upcoming"));
        s.broadcast.start_ms = None;
        s.broadcast.scheduled_start_ms = Some(1_000);
        assert_eq!(classify_kind(&s), Some("upcoming"));
        // isLiveNow 欠落（未開始のロビー形）でも details 有りは upcoming
        let s = snap(
            None,
            true,
            Some(1_000),
            None,
            true,
            None,
            Some("LIVE_STREAM_OFFLINE"),
        );
        assert_eq!(classify_kind(&s), Some("upcoming"));
    }

    /// DB-MR-12: FR-96/EB — `ResolverCore` 統合テスト（モック InnerTube HTTP サーバ使用）。
    /// 状態遷移（upcoming → live → ended）、HTTP 429 指数バックオフ、連続 3 回通信失敗中断、
    /// 連続 6 回 LOGIN_REQUIRED 個別解除、および手動 60 秒クールダウンを検証する。
    #[tokio::test]
    async fn resolver_core_refresh_backoff_breaker_and_manual_cooldown() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let mode = Arc::new(Mutex::new("live".to_string()));
        let hits = Arc::new(AtomicUsize::new(0));
        let mode_srv = Arc::clone(&mode);
        let hits_srv = Arc::clone(&hits);

        let srv = tokio::spawn(async move {
            let html = r#"<script>ytcfg.set({"INNERTUBE_API_KEY":"k","INNERTUBE_CONTEXT_CLIENT_VERSION":"v","VISITOR_DATA":"d"});</script>"#;
            while let Ok((mut stream, _)) = listener.accept().await {
                let mut buf = [0u8; 4096];
                let n = stream.read(&mut buf).await.unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]);
                let resp = if req.starts_with("GET ") {
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        html.len(),
                        html
                    )
                } else {
                    hits_srv.fetch_add(1, Ordering::SeqCst);
                    let cur = lock(&mode_srv).clone();
                    match cur.as_str() {
                        "429" => "HTTP/1.1 429 Too Many Requests\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}".to_string(),
                        "500" => "HTTP/1.1 500 Internal Server Error\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}".to_string(),
                        "login_required" => {
                            let body = r#"{"playabilityStatus":{"status":"LOGIN_REQUIRED"},"videoDetails":{"videoId":"mr_vid_0001"}}"#;
                            format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body)
                        }
                        "ended" => {
                            let body = r#"{"playabilityStatus":{"status":"OK"},"videoDetails":{"videoId":"mr_vid_0001","isLiveContent":true,"shortDescription":"d"},"microformat":{"playerMicroformatRenderer":{"publishDate":"2026-11-27T08:00:00+00:00","liveBroadcastDetails":{"isLiveNow":false,"startTimestamp":"2026-11-27T08:00:00+00:00","endTimestamp":"2026-11-27T09:00:00+00:00"}}}}"#;
                            format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body)
                        }
                        _ => {
                            let body = r#"{"playabilityStatus":{"status":"OK"},"videoDetails":{"videoId":"mr_vid_0001","isLiveContent":true,"shortDescription":"d"},"microformat":{"playerMicroformatRenderer":{"publishDate":"2026-11-27T08:00:00+00:00","liveBroadcastDetails":{"isLiveNow":true,"startTimestamp":"2026-11-27T08:00:00+00:00"}}}}"#;
                            format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body)
                        }
                    }
                };
                let _ = stream.write_all(resp.as_bytes()).await;
            }
        });

        let db = Db::connect_in_memory().unwrap();
        let ch = "UCmr0000000000000000001";
        db.feed_subscribe(&crate::db::SubscribeArgs {
            channel_id: ch,
            title: "CH",
            thumbnail_url: None,
            category_id: None,
            entries: &[crate::db::NewVideo {
                video_id: "mr_vid_0001",
                channel_id: ch,
                channel_title: "CH",
                title: "Live",
                thumbnail_url: None,
                published_at: Some("2026-11-27T08:00:00+00:00"),
                kind: "upcoming",
            }],
            etag: None,
            last_modified: None,
        })
        .unwrap();

        let tube = Arc::new(InnerTube::with_test_urls(
            format!("http://{addr}/watch"),
            format!("http://{addr}/api"),
            Duration::from_secs(5),
        ));
        let core = ResolverCore::new(db.clone(), tube);
        let t0 = Instant::now();
        let now_ms = 1_795_767_000_000i64;

        // 1) 初回解決で upcoming -> live へ遷移する
        let changed = core.resolve_once(t0, now_ms).await;
        assert_eq!(changed, 1);
        let row = db
            .feed_list_filtered(&crate::model::FeedFilter::default(), true, 10, |_| true)
            .unwrap();
        assert_eq!(row[0].broadcast_state.as_deref(), Some("live"));
        assert_eq!(row[0].kind, "live");

        // 2) 手動更新の 60 秒クールダウン検証（初回は通り、30 秒後はガード、61 秒後は通る）
        core.wake_manual();
        assert!(core.take_manual_force_at(t0));
        core.wake_manual();
        assert!(!core.take_manual_force_at(t0 + Duration::from_secs(30)));
        core.wake_manual();
        assert!(core.take_manual_force_at(t0 + Duration::from_secs(61)));

        // 3) LOGIN_REQUIRED 連続 5 回では live を保持、6 回目で個別解除
        *lock(&mode) = "login_required".to_string();
        let t_blk = t0 + Duration::from_secs(121);
        for step in 0..5 {
            core.wake_manual();
            let step_t = t_blk + Duration::from_secs(120 * step);
            let step_ms = now_ms + 120_000 * (step as i64 + 1);
            let c = core.resolve_once(step_t, step_ms).await;
            assert_eq!(c, 0);
            let item = &db
                .feed_list_filtered(&crate::model::FeedFilter::default(), true, 10, |_| true)
                .unwrap()[0];
            assert_eq!(item.broadcast_state.as_deref(), Some("live"));
        }
        core.wake_manual();
        let c6 = core
            .resolve_once(t_blk + Duration::from_secs(600), now_ms + 720_000)
            .await;
        assert_eq!(c6, 1);
        let cleared = &db
            .feed_list_filtered(&crate::model::FeedFilter::default(), true, 10, |_| true)
            .unwrap()[0];
        assert_eq!(cleared.broadcast_state, None);

        // 4) HTTP 429 で指数バックオフ（5分 → 10分 → 15分上限）と実応答中断・クールダウン中スキップ
        let b0 = t_blk + Duration::from_secs(1000);
        core.record_rate_limit_at(b0);
        assert!(core.is_rate_limited_at(b0 + Duration::from_secs(299)));
        assert!(!core.is_rate_limited_at(b0 + Duration::from_secs(301)));
        let hits_before = hits.load(Ordering::SeqCst);
        assert_eq!(
            core.resolve_once(b0 + Duration::from_secs(10), now_ms + 1_000_000)
                .await,
            0
        );
        assert_eq!(hits.load(Ordering::SeqCst), hits_before);

        core.record_rate_limit_at(b0);
        assert!(core.is_rate_limited_at(b0 + Duration::from_secs(599)));
        core.record_rate_limit_at(b0);
        assert!(core.is_rate_limited_at(b0 + Duration::from_secs(899)));
        core.clear_rate_limit();

        // 5) 連続 3 回通信失敗（500）でのサイクル中断と、実 HTTP 429 応答での即中断
        for i in 0..4 {
            let id = format!("brk_vid_{i:03}");
            db.feed_ingest(
                ch,
                &[crate::db::NewVideo {
                    video_id: &id,
                    channel_id: ch,
                    channel_title: "CH",
                    title: &id,
                    thumbnail_url: None,
                    published_at: Some("2026-11-27T07:00:00+00:00"),
                    kind: "live",
                }],
                None,
                None,
            )
            .unwrap();
            db.videos_resolve_meta_tx(&ResolveMetaParams {
                video_id: &id,
                needs_published: false,
                published_at: None,
                needs_desc: true,
                description: "",
                needs_kind: true,
                resolved_kind: Some("live"),
                broadcast_state: Some("live"),
                broadcast_start_ms: Some(now_ms),
                now_ms,
            })
            .unwrap();
        }
        *lock(&mode) = "500".to_string();
        let hits_before_500 = hits.load(Ordering::SeqCst);
        core.resolve_once(b0 + Duration::from_secs(1000), now_ms + 2_000_000)
            .await;
        // 4 件中 3 件失敗した時点でブレーカーが働き、4 件目は呼ばれない
        assert_eq!(hits.load(Ordering::SeqCst) - hits_before_500, 3);

        // 実 HTTP 429 応答を 1 回受けると即中断し rate_limit 状態へ入る
        *lock(&mode) = "429".to_string();
        let hits_before_429 = hits.load(Ordering::SeqCst);
        let t_429 = b0 + Duration::from_secs(1300);
        core.resolve_once(t_429, now_ms + 2_500_000).await;
        assert_eq!(hits.load(Ordering::SeqCst) - hits_before_429, 1);
        assert!(core.is_rate_limited_at(t_429 + Duration::from_secs(299)));
        core.clear_rate_limit();

        // 6) ended 遷移で broadcast_state='ended' になること
        *lock(&mode) = "ended".to_string();
        let c_end = core
            .resolve_once(b0 + Duration::from_secs(1600), now_ms + 3_000_000)
            .await;
        assert!(c_end >= 1);
        let ended_item = db
            .feed_list_filtered(&crate::model::FeedFilter::default(), true, 10, |_| true)
            .unwrap()
            .into_iter()
            .find(|i| i.video_id == "brk_vid_000")
            .unwrap();
        assert_eq!(ended_item.broadcast_state.as_deref(), Some("ended"));

        srv.abort();
    }
}
