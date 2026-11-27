//! ライブ配信の待機（FR-44、仕様決定 BT）。
//! DB の待機リストを定期巡回し、InnerTube `player` 応答の
//! `liveBroadcastDetails` で開始・終了・自動解除を検出する。
//! 検出時の動作は設定 `live_wait.action`（`play` 既定）で
//! 新規インスタンスの自動再生またはトースト通知を行う。
//!
//! ポーリング間隔・失効・分類の純粋関数はフロント側の
//! `src/lib/livewait-logic.ts` と同一ルールで実装する（双方向で検証）。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::Notify;

use crate::db::Db;
use crate::innertube::{is_transient_or_blocked_playability, InnerTube};
use crate::model::{
    classify_broadcast_meta, classify_kind, BroadcastStatus, FeedNewItems, LiveWait,
    LiveWaitChanged, LiveWaitStatus, MetaSnapshot,
};
use crate::util::lock;
use crate::ytapi::{QuotaBucket, YtApi, VIDEOS_BATCH_MAX};

/// 巡回ティック。直前窓の短間隔（15 秒）を粒度内に収めるため 10 秒とする。
const TICK: Duration = Duration::from_secs(10);
/// 予定時刻の無い項目と直前窓の外にある項目の定間隔（仕様決定 BT「60 秒程度」）。
pub const POLL_BASE_MS: i64 = 60_000;
/// 直前窓内の短間隔（暫定 15 秒。仕様決定 BT は「短間隔」のみ指定）。
pub const POLL_SHORT_MS: i64 = 15_000;
/// 直前窓の手前側。予定開始のこの時間前から短間隔へ（暫定 2 分）。
pub const NEAR_BEFORE_MS: i64 = 120_000;
/// 直前窓の後側。予定超過後も遅延開始を短間隔で追う時間（暫定 30 分）。
/// 超過後の 24 時間ずっと短間隔では取得が間延びせず負荷になるため、
/// 窓を過ぎたら定間隔へ戻しつつ自動解除期限まで追い続ける。
pub const NEAR_AFTER_MS: i64 = 30 * 60_000;
/// 開始予定からの自動解除期限（仕様決定 BT「24 時間超過」）。
/// 予定の無い項目は登録時刻を起点とする（暫定解釈。予定未公開のまま
/// 長期間開始されない項目を際限なく追い続けないため）。
pub const EXPIRY_MS: i64 = 24 * 60 * 60_000;
/// 状態確認の連続失敗で劣化通知する閾値（feed ポーラーと同じ方針）。
const FAIL_NOTIFY_THRESHOLD: u32 = 2;

/// `live_wait.action` の設定キー（仕様決定 BT(3)）。
pub(crate) const SETTING_LIVE_WAIT_ACTION: &str = "live_wait.action";

/// `player` 応答から待機対象の状態を分類する。優先順位は
/// 放送中 > 終了済み > 開始前 > 非放送。
/// `isLiveNow` 真は `endTimestamp` が混在する矛盾形でも放送中とする
/// （`BroadcastWindow::is_ended` と同じ判断）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitBroadcast {
    /// 開始前（予約配信のロビー・プレミア公開予定）。
    /// `scheduled_ms` は予定開始で `scheduledStartTimestamp` を優先、
    /// 無ければ `startTimestamp`（実応答ではこちらに予定が入る）。
    Upcoming { scheduled_ms: Option<i64> },
    /// 放送中（開始検出）。
    Live,
    /// 終了済み（`endTimestamp` あり）。
    Ended,
    /// `liveBroadcastDetails` を持たない通常動画（待機対象外）。
    NotBroadcast,
}

pub fn classify_broadcast(s: &BroadcastStatus) -> WaitBroadcast {
    if s.is_live_now == Some(true) {
        return WaitBroadcast::Live;
    }
    if s.end_ms.is_some() {
        return WaitBroadcast::Ended;
    }
    if s.has_details || s.is_upcoming {
        return WaitBroadcast::Upcoming {
            scheduled_ms: s.scheduled_start_ms.or(s.start_ms),
        };
    }
    WaitBroadcast::NotBroadcast
}

/// 次回ポーリングまでの間隔（ms）。予定時刻が直前窓内なら短間隔、
/// それ以外（予定無しを含む）は定間隔（仕様決定 BT）。
pub fn next_poll_ms(now_ms: i64, scheduled_ms: Option<i64>) -> i64 {
    match scheduled_ms {
        Some(s) if (s - NEAR_BEFORE_MS..=s + NEAR_AFTER_MS).contains(&now_ms) => POLL_SHORT_MS,
        _ => POLL_BASE_MS,
    }
}

/// 開始予定から 24 時間を超えて未開始の項目の自動解除判定（仕様決定 BT）。
/// 予定の無い項目は登録時刻を起点とする（暫定解釈）。
pub fn is_expired(now_ms: i64, scheduled_ms: Option<i64>, registered_ms: i64) -> bool {
    now_ms > scheduled_ms.unwrap_or(registered_ms) + EXPIRY_MS
}

/// `live_wait.action` の動作（仕様決定 BT(3)）。未設定・その他の値は play。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitAction {
    Play,
    Notify,
}

pub fn wait_action_of(value: Option<String>) -> WaitAction {
    match value.as_deref() {
        Some("notify") => WaitAction::Notify,
        _ => WaitAction::Play,
    }
}

fn sync_feed_with_kind(
    db: &Db,
    video_id: &str,
    snap: &MetaSnapshot,
    kind_opt: Option<&'static str>,
    now_ms: i64,
) -> bool {
    if is_transient_or_blocked_playability(snap) {
        return false;
    }
    let (b_state, b_start) = classify_broadcast_meta(snap);
    match db.videos_apply_broadcast_refresh(video_id, kind_opt, b_state, b_start, now_ms, true) {
        Ok(changed) => changed,
        Err(e) => {
            tracing::warn!(error = %e, "ライブ待機からの放送状態相乗り更新に失敗");
            false
        }
    }
}

/// ライブ待機ポーラーが取得した `MetaSnapshot` を、フィード台帳（`ingested = 1` の行のみ）へ
/// 単調時刻ガード付きで相乗り反映する（FR-96、仕様決定 EB）。
/// ボット判定・一時制限応答（`LOGIN_REQUIRED` / `ERROR`）時は直前状態を保護し、変更があれば `true` を返す。
pub(crate) fn sync_feed_from_snapshot(
    db: &Db,
    video_id: &str,
    snap: &MetaSnapshot,
    now_ms: i64,
) -> bool {
    let kind_opt = classify_kind(snap);
    sync_feed_with_kind(db, video_id, snap, kind_opt, now_ms)
}

/// 現在の epoch ミリ秒。登録時刻と期限判定に使う。
pub(crate) fn epoch_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// `livewait://changed` の送出。コマンド側の登録・解除でも同じ
/// ペイロードで通知する。対象が分かる場合は video_id を載せる。
pub fn emit_changed(app: &AppHandle, video_id: Option<&str>) {
    let payload = LiveWaitChanged {
        video_id: video_id.map(|s| s.to_string()),
    };
    if let Err(e) = app.emit("livewait://changed", payload) {
        tracing::warn!(error = %e, "livewait://changed の送出に失敗");
    }
}

/// `livewait://status` の送出。開始・終了・自動解除・劣化をトーストへ流す。
pub fn emit_status(app: &AppHandle, video_id: Option<&str>, level: &str, message: String) {
    let payload = LiveWaitStatus {
        video_id: video_id.map(|s| s.to_string()),
        level: level.to_string(),
        message,
    };
    if let Err(e) = app.emit("livewait://status", payload) {
        tracing::warn!(error = %e, "livewait://status の送出に失敗");
    }
}

/// 項目ごとの次回予定と連続失敗数（プロセス内状態。DB には持たない）。
#[derive(Default)]
struct WaitSchedule {
    /// 次回ポーリング時刻。未設定は即時対象。
    next_at: Option<Instant>,
    consecutive_failures: u32,
    /// 劣化通知を出した状態か（復帰通知の重複抑止）。
    degraded: bool,
}

/// ライブ待機のポーラー（仕様決定 BT(4)(5)・ED）。`run` を起動時に spawn する。
/// 待機リストは DB へ write-through するため再起動でそのまま復元される。
pub struct LiveWaitPoller {
    db: Db,
    app: Option<AppHandle>,
    ytapi: Arc<YtApi>,
    innertube: Arc<InnerTube>,
    /// video_id → 次回ポーリング予定と失敗回数。
    sched: Mutex<HashMap<String, WaitSchedule>>,
    /// 新規登録時に即時巡回を起こすための通知。
    wake: Notify,
}

impl LiveWaitPoller {
    pub fn new(db: Db, app: AppHandle, ytapi: Arc<YtApi>, innertube: Arc<InnerTube>) -> Arc<Self> {
        Arc::new(Self {
            db,
            app: Some(app),
            ytapi,
            innertube,
            sched: Mutex::new(HashMap::new()),
            wake: Notify::new(),
        })
    }

    #[cfg(test)]
    pub(crate) fn new_test(db: Db, ytapi: Arc<YtApi>, innertube: Arc<InnerTube>) -> Arc<Self> {
        Arc::new(Self {
            db,
            app: None,
            ytapi,
            innertube,
            sched: Mutex::new(HashMap::new()),
            wake: Notify::new(),
        })
    }

    fn emit_changed_opt(&self, video_id: Option<&str>) {
        if let Some(app) = &self.app {
            emit_changed(app, video_id);
        }
    }

    fn emit_status_opt(&self, video_id: Option<&str>, level: &str, message: String) {
        if let Some(app) = &self.app {
            emit_status(app, video_id, level, message);
        }
    }

    fn emit_kind_updated_opt(&self, count: usize) {
        if let Some(app) = &self.app {
            let _ = app.emit("feed://kind_updated", FeedNewItems { count });
        }
    }

    /// 新規登録などでスケジュールを即時回したいときに呼ぶ。
    /// 次回予定は変えない（登録直後の項目はスケジュール未登録＝即時対象）。
    pub fn wake_now(&self) {
        self.wake.notify_one();
    }

    /// メインループ。TICK ごとに期限の来た項目を順次ポーリングする。
    /// 待機ゼロの間は wake 通知 or TICK の sleep で待つ。
    pub async fn run(self: Arc<Self>) {
        loop {
            tokio::select! {
                _ = self.wake.notified() => {},
                _ = tokio::time::sleep(TICK) => {},
            }
            self.poll_due().await;
        }
    }

    /// 期限の来た項目をすべて処理する。
    async fn poll_due(&self) {
        let waits = match self.db.live_wait_list() {
            Ok(w) => w,
            Err(e) => {
                tracing::warn!(error = %e, "待機一覧の読み込みに失敗");
                return;
            }
        };
        let now = Instant::now();
        let due: Vec<LiveWait> = {
            let sched = lock(&self.sched);
            waits
                .into_iter()
                .filter(|w| {
                    sched
                        .get(&w.video_id)
                        .and_then(|s| s.next_at)
                        .is_none_or(|t| t <= now)
                })
                .collect()
        };
        let mut active_due: Vec<LiveWait> = Vec::with_capacity(due.len());
        let now_ms = epoch_ms();
        for wait in due {
            // ポーリング中に解除される場合に備えて、処理直前に存在と期限を確認する
            if self
                .db
                .live_wait_get(&wait.video_id)
                .ok()
                .flatten()
                .is_none()
            {
                lock(&self.sched).remove(&wait.video_id);
                continue;
            }
            if is_expired(now_ms, wait.scheduled_start_ms, wait.registered_ms) {
                self.remove_wait(
                    &wait,
                    "info",
                    format!(
                        "開始予定を 24 時間超えても開始されなかったため待機を解除しました: {}",
                        wait.title
                    ),
                );
                continue;
            }
            active_due.push(wait);
        }

        for chunk in active_due.chunks(VIDEOS_BATCH_MAX) {
            if self.ytapi.can_use(QuotaBucket::Shared) {
                let ids: Vec<&str> = chunk.iter().map(|w| w.video_id.as_str()).collect();
                match self.ytapi.videos_list(&ids).await {
                    Ok(map) => {
                        for wait in chunk {
                            if self
                                .db
                                .live_wait_get(&wait.video_id)
                                .ok()
                                .flatten()
                                .is_none()
                            {
                                lock(&self.sched).remove(&wait.video_id);
                                continue;
                            }
                            let item_now_ms = epoch_ms();
                            match map.get(&wait.video_id) {
                                Some(item) => {
                                    self.after_success(wait);
                                    if sync_feed_with_kind(
                                        &self.db,
                                        &wait.video_id,
                                        &item.meta,
                                        item.classify_kind_for_refresh(),
                                        item_now_ms,
                                    ) {
                                        self.emit_kind_updated_opt(1);
                                    }
                                    self.handle_broadcast_outcome(wait, &item.meta, item_now_ms)
                                        .await;
                                }
                                None => {
                                    // HTTP 200 応答からの単発欠落では live_wait 削除も
                                    // videos.broadcast_state クリアも行わず次回へ回す（ガード 3）。
                                    self.reschedule(&wait.video_id, POLL_BASE_MS);
                                }
                            }
                        }
                        continue;
                    }
                    Err(e) => {
                        let _ = e.should_fallback();
                    }
                }
            }
            self.poll_chunk_innertube(chunk).await;
        }
    }

    async fn poll_chunk_innertube(&self, chunk: &[LiveWait]) {
        for wait in chunk {
            if self
                .db
                .live_wait_get(&wait.video_id)
                .ok()
                .flatten()
                .is_none()
            {
                lock(&self.sched).remove(&wait.video_id);
                continue;
            }
            self.poll_one_innertube(wait).await;
        }
    }

    /// 1 項目の InnerTube ポーリング。自動解除 → 放送状態の分類 → 遷移処理の順。
    async fn poll_one_innertube(&self, wait: &LiveWait) {
        let now_ms = epoch_ms();
        if is_expired(now_ms, wait.scheduled_start_ms, wait.registered_ms) {
            self.remove_wait(
                wait,
                "info",
                format!(
                    "開始予定を 24 時間超えても開始されなかったため待機を解除しました: {}",
                    wait.title
                ),
            );
            return;
        }
        match self.innertube.meta_snapshot(&wait.video_id).await {
            Err(e) => self.after_failure(wait, e.to_string()),
            Ok(snap) => {
                self.after_success(wait);
                if sync_feed_from_snapshot(&self.db, &wait.video_id, &snap, now_ms) {
                    self.emit_kind_updated_opt(1);
                }
                self.handle_broadcast_outcome(wait, &snap, now_ms).await;
            }
        }
    }

    async fn handle_broadcast_outcome(&self, wait: &LiveWait, snap: &MetaSnapshot, now_ms: i64) {
        match classify_broadcast(&snap.broadcast) {
            WaitBroadcast::Live => self.on_live(wait).await,
            WaitBroadcast::Ended => self.remove_wait(
                wait,
                "info",
                format!("配信は終了しています（待機を解除しました）: {}", wait.title),
            ),
            WaitBroadcast::Upcoming { scheduled_ms } => {
                // 予定変更へ追随する（表示の予定時刻と直前窓の判定がずれないよう保存値を更新）
                if scheduled_ms != wait.scheduled_start_ms {
                    if let Err(e) = self
                        .db
                        .live_wait_set_scheduled(&wait.video_id, scheduled_ms)
                    {
                        tracing::warn!(error = %e, "待機の予定時刻更新に失敗");
                    } else {
                        self.emit_changed_opt(Some(&wait.video_id));
                    }
                }
                self.reschedule(&wait.video_id, next_poll_ms(now_ms, scheduled_ms));
            }
            WaitBroadcast::NotBroadcast => {
                // 放送詳細を失った異常系（中止・非公開化などの可能性）。
                // 項目は残して定間隔で追い、自動解除期限の掃除に委ねる
                self.reschedule(&wait.video_id, POLL_BASE_MS);
            }
        }
    }

    /// 開始検出（仕様決定 BT(3)）。待機を解除してから動作へ移る。
    /// `play` は新規インスタンスの自動再生、`notify` はトースト通知のみ。
    async fn on_live(&self, wait: &LiveWait) {
        self.remove_entry(&wait.video_id);
        let action = wait_action_of(self.db.setting_get(SETTING_LIVE_WAIT_ACTION).ok().flatten());
        match action {
            WaitAction::Play => match self.auto_play(wait).await {
                Ok(()) => self.emit_status_opt(
                    Some(&wait.video_id),
                    "info",
                    format!("配信開始を検出し、自動再生を開始しました: {}", wait.title),
                ),
                Err(e) => self.emit_status_opt(
                    Some(&wait.video_id),
                    "warn",
                    format!(
                        "配信開始を検出しましたが自動再生に失敗しました: {}（{}）",
                        wait.title, e
                    ),
                ),
            },
            WaitAction::Notify => self.emit_status_opt(
                Some(&wait.video_id),
                "info",
                format!("配信が始まりました: {}", wait.title),
            ),
        }
    }

    /// 新規インスタンスでの自動再生。`play_video` と同じ解決順
    /// （pip.default → pip.quality.format → quality.format）で起動引数を作る。
    /// レジューム位置は使わない（配信は先頭・ライブ端で開く）。
    async fn auto_play(&self, wait: &LiveWait) -> Result<(), crate::mpv::MpvError> {
        let pip = crate::mpv::pip_default_enabled(
            self.db
                .setting_get(crate::mpv::SETTING_PIP_DEFAULT)
                .ok()
                .flatten(),
        );
        let global = self
            .db
            .setting_get(crate::yt::SETTING_QUALITY_FORMAT)
            .ok()
            .flatten();
        let pip_format = if pip {
            self.db
                .setting_get(crate::yt::SETTING_PIP_QUALITY_FORMAT)
                .ok()
                .flatten()
        } else {
            None
        };
        let ytdl_format = crate::yt::resolve_launch_format(None, pip, pip_format, global);
        let Some(app) = &self.app else {
            return Ok(());
        };
        let players = app
            .try_state::<crate::mpv::PlayerManager>()
            .ok_or(crate::mpv::MpvError::NoSuchInstance(0))?;
        players
            .play_with_channel_hint(
                &wait.video_id,
                0.0,
                ytdl_format,
                pip,
                Some(crate::mpv::HistoryChannelHint {
                    channel_id: wait.channel_id.as_deref(),
                    channel_title: wait.channel_title.as_deref(),
                    fallback_title: Some(&wait.title),
                }),
            )
            .await
            .map(|_| ())
    }

    /// 待機解除＋一覧変更通知。戻り値は削除できたか。
    fn remove_entry(&self, video_id: &str) -> bool {
        let removed = match self.db.live_wait_remove(video_id) {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(error = %e, "待機項目の削除に失敗");
                false
            }
        };
        lock(&self.sched).remove(video_id);
        self.emit_changed_opt(Some(video_id));
        removed
    }

    /// 解除＋一覧変更通知＋状態通知のまとめ（終了・自動解除の出口）。
    fn remove_wait(&self, wait: &LiveWait, level: &str, message: String) {
        self.remove_entry(&wait.video_id);
        self.emit_status_opt(Some(&wait.video_id), level, message);
    }

    /// 次回予定の設定。
    fn reschedule(&self, video_id: &str, delay_ms: i64) {
        let mut sched = lock(&self.sched);
        sched.entry(video_id.to_string()).or_default().next_at =
            Some(Instant::now() + Duration::from_millis(u64::try_from(delay_ms).unwrap_or(0)));
    }

    /// 取得成功時の失敗カウンタ整理と復帰通知。
    fn after_success(&self, wait: &LiveWait) {
        let was_degraded = {
            let mut sched = lock(&self.sched);
            let s = sched.entry(wait.video_id.clone()).or_default();
            s.consecutive_failures = 0;
            let was = s.degraded;
            s.degraded = false;
            was
        };
        if was_degraded {
            self.emit_status_opt(
                Some(&wait.video_id),
                "info",
                format!("配信待機の状態確認が復帰しました: {}", wait.title),
            );
        }
    }

    /// 取得失敗後の再スケジュールと劣化通知。
    fn after_failure(&self, wait: &LiveWait, message: String) {
        tracing::warn!(
            video_id = %wait.video_id,
            error = %message,
            "待機中動画の放送状態確認に失敗"
        );
        self.reschedule(&wait.video_id, POLL_BASE_MS);
        let emit = {
            let mut sched = lock(&self.sched);
            let s = sched.entry(wait.video_id.clone()).or_default();
            s.consecutive_failures += 1;
            // 閾値到達時のみ通知（連続失敗ごとに送ると通知が洪水になる）
            if s.consecutive_failures == FAIL_NOTIFY_THRESHOLD {
                s.degraded = true;
                true
            } else {
                false
            }
        };
        if emit {
            self.emit_status_opt(
                Some(&wait.video_id),
                "warn",
                format!(
                    "配信待機の状態確認が連続して失敗しています: {}（{}）",
                    wait.title, message
                ),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(
        is_live_now: Option<bool>,
        start_ms: Option<i64>,
        end_ms: Option<i64>,
        scheduled: Option<i64>,
        has_details: bool,
        is_upcoming: bool,
    ) -> BroadcastStatus {
        BroadcastStatus {
            is_live_now,
            start_ms,
            end_ms,
            scheduled_start_ms: scheduled,
            has_details,
            is_upcoming,
        }
    }

    /// FR-44/BT: 放送状態の分類。優先順位は放送中 > 終了済み > 開始前 > 非放送。
    #[test]
    fn classify_broadcast_states() {
        // 予約配信のロビー（実応答形: isLiveNow:false＋startTimestamp）
        assert_eq!(
            classify_broadcast(&status(Some(false), Some(1000), None, None, true, true)),
            WaitBroadcast::Upcoming {
                scheduled_ms: Some(1000)
            }
        );
        // scheduledStartTimestamp があれば優先（未観測フィールド）
        assert_eq!(
            classify_broadcast(&status(
                Some(false),
                Some(1000),
                None,
                Some(2000),
                true,
                true
            )),
            WaitBroadcast::Upcoming {
                scheduled_ms: Some(2000)
            }
        );
        // isUpcoming だけでも開始前（liveBroadcastDetails を持たない予定形）
        assert_eq!(
            classify_broadcast(&status(None, None, None, None, false, true)),
            WaitBroadcast::Upcoming { scheduled_ms: None }
        );
        // 放送中
        assert_eq!(
            classify_broadcast(&status(Some(true), Some(500), None, None, true, false)),
            WaitBroadcast::Live
        );
        // isLiveNow:true と endTimestamp の矛盾形は放送中優先
        assert_eq!(
            classify_broadcast(&status(Some(true), Some(500), Some(900), None, true, false)),
            WaitBroadcast::Live
        );
        // 終了済み
        assert_eq!(
            classify_broadcast(&status(
                Some(false),
                Some(500),
                Some(900),
                None,
                true,
                false
            )),
            WaitBroadcast::Ended
        );
        // 詳細無し・非 upcoming は通常動画
        assert_eq!(
            classify_broadcast(&status(None, None, None, None, false, false)),
            WaitBroadcast::NotBroadcast
        );
    }

    /// FR-44/BT: ポーリング間隔。直前窓内は短間隔、それ以外は 60 秒。
    #[test]
    fn next_poll_ms_windows() {
        let start = 1_000_000i64;
        // 予定無しは定間隔
        assert_eq!(next_poll_ms(0, None), POLL_BASE_MS);
        // 窓の手前は定間隔
        assert_eq!(
            next_poll_ms(start - NEAR_BEFORE_MS - 1, Some(start)),
            POLL_BASE_MS
        );
        // 直前窓内（手前側・開始時刻・後側）と境界は短間隔
        assert_eq!(
            next_poll_ms(start - NEAR_BEFORE_MS, Some(start)),
            POLL_SHORT_MS
        );
        assert_eq!(next_poll_ms(start, Some(start)), POLL_SHORT_MS);
        assert_eq!(
            next_poll_ms(start + NEAR_AFTER_MS, Some(start)),
            POLL_SHORT_MS
        );
        // 窓を過ぎると定間隔へ戻る（遅延開始は定間隔で追い続ける）
        assert_eq!(
            next_poll_ms(start + NEAR_AFTER_MS + 1, Some(start)),
            POLL_BASE_MS
        );
    }

    /// FR-44/BT: 24 時間超過の自動解除。予定無しは登録時刻起点（暫定）。
    #[test]
    fn is_expired_deadline() {
        let start = 1_000_000i64;
        let registered = 500i64;
        assert!(!is_expired(start + EXPIRY_MS, Some(start), registered));
        assert!(is_expired(start + EXPIRY_MS + 1, Some(start), registered));
        // 予定無しは登録時刻を起点にする
        assert!(!is_expired(registered + EXPIRY_MS, None, registered));
        assert!(is_expired(registered + EXPIRY_MS + 1, None, registered));
    }

    /// FR-44/BT(3): `live_wait.action` の受理集合。未設定・その他は play。
    #[test]
    fn wait_action_parsing() {
        assert_eq!(wait_action_of(None), WaitAction::Play);
        assert_eq!(
            wait_action_of(Some("notify".to_string())),
            WaitAction::Notify
        );
        assert_eq!(wait_action_of(Some("play".to_string())), WaitAction::Play);
        assert_eq!(wait_action_of(Some("bogus".to_string())), WaitAction::Play);
        assert_eq!(wait_action_of(Some("".to_string())), WaitAction::Play);
    }

    /// DB-LW-07: FR-96/EB — `sync_feed_from_snapshot` による相乗り更新、
    /// `ingested = 0`・未登録動画の抑止、およびボット判定応答での状態保護を検証する。
    #[test]
    fn sync_feed_from_snapshot_respects_ingested_and_bot_guard() {
        let db = Db::connect_in_memory().unwrap();
        let ch = "UClw0000000000000000001";
        db.feed_subscribe(&crate::db::SubscribeArgs {
            channel_id: ch,
            title: "CH",
            thumbnail_url: None,
            category_id: None,
            entries: &[crate::db::NewVideo {
                video_id: "lw_ingested",
                channel_id: ch,
                channel_title: "CH",
                title: "Stream",
                thumbnail_url: None,
                published_at: Some("2026-11-27T08:00:00+00:00"),
                kind: "upcoming",
            }],
            etag: None,
            last_modified: None,
        })
        .unwrap();
        db.favorite_add(&crate::model::VideoRef {
            video_id: "lw_placehld".to_string(),
            title: "Placeholder".to_string(),
            channel_id: Some(ch.to_string()),
            channel_title: Some("CH".to_string()),
            thumbnail_url: None,
        })
        .unwrap();

        let live_snap = MetaSnapshot {
            published_at: None,
            broadcast: BroadcastStatus {
                is_live_now: Some(true),
                start_ms: Some(1_795_767_000_000),
                end_ms: None,
                scheduled_start_ms: None,
                has_details: true,
                is_upcoming: false,
            },
            is_live_content: Some(true),
            playability: Some("OK".to_string()),
            description: String::new(),
        };

        // ingested=1 は更新され true
        assert!(sync_feed_from_snapshot(&db, "lw_ingested", &live_snap, 100));
        let item = db
            .feed_list_filtered(&crate::model::FeedFilter::default(), true, 10, |_| true)
            .unwrap()
            .into_iter()
            .find(|i| i.video_id == "lw_ingested")
            .unwrap();
        assert_eq!(item.broadcast_state.as_deref(), Some("live"));
        assert_eq!(item.broadcast_start_ms, Some(1_795_767_000_000));

        // ingested=0 や未登録動画は false（通知不要）
        assert!(!sync_feed_from_snapshot(
            &db,
            "lw_placehld",
            &live_snap,
            100
        ));
        assert!(!sync_feed_from_snapshot(
            &db,
            "lw_unknown_",
            &live_snap,
            100
        ));

        // ボット判定応答（LOGIN_REQUIRED かつ詳細なし）は状態を消さず false
        let bot_snap = MetaSnapshot {
            playability: Some("LOGIN_REQUIRED".to_string()),
            ..Default::default()
        };
        assert!(!sync_feed_from_snapshot(&db, "lw_ingested", &bot_snap, 200));
        let kept = db
            .feed_list_filtered(&crate::model::FeedFilter::default(), true, 10, |_| true)
            .unwrap()
            .into_iter()
            .find(|i| i.video_id == "lw_ingested")
            .unwrap();
        assert_eq!(kept.broadcast_state.as_deref(), Some("live"));
    }

    async fn spawn_lw_ytapi_mock(
        handler: impl Fn(&str, usize) -> (u16, String) + Send + Sync + 'static,
    ) -> (String, Arc<Mutex<Vec<String>>>, tokio::task::JoinHandle<()>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let reqs = Arc::new(Mutex::new(Vec::new()));
        let reqs_srv = Arc::clone(&reqs);
        let handler = Arc::new(handler);
        let srv = tokio::spawn(async move {
            let mut idx = 0usize;
            while let Ok((mut stream, _)) = listener.accept().await {
                let cur_idx = idx;
                idx += 1;
                let reqs_c = Arc::clone(&reqs_srv);
                let handler_c = Arc::clone(&handler);
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
                    let text = String::from_utf8_lossy(&raw);
                    let line = text.lines().next().unwrap_or_default().to_string();
                    lock(&reqs_c).push(line.clone());
                    let (status, body) = handler_c(&line, cur_idx);
                    let reason = match status {
                        200 => "OK",
                        403 => "Forbidden",
                        500 => "Internal Server Error",
                        _ => "Error",
                    };
                    let resp = format!(
                        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = stream.write_all(resp.as_bytes()).await;
                    let _ = stream.shutdown().await;
                });
            }
        });
        (format!("http://{addr}"), reqs, srv)
    }

    /// DB-LW-08: `LiveWaitPoller::poll_due` が 50 件超の待機項目を 50 件チャンクで一括巡回し、`ingested = 1` のフィード同期および終了済みプレミアの `premiere` 保持を行うことを検証する。
    #[tokio::test]
    async fn poll_due_batches_50_chunks_and_syncs_ingested_feed() {
        let db = Db::connect_in_memory().unwrap();
        let ch = "UClw0800000000000000001";
        let now_ms = epoch_ms();
        db.setting_set(SETTING_LIVE_WAIT_ACTION, "notify").unwrap();

        // 55 件登録: lw_000 (ingested live), lw_001 (ingested ended premiere), lw_002..054 (upcoming)
        db.feed_subscribe(&crate::db::SubscribeArgs {
            channel_id: ch,
            title: "CH",
            thumbnail_url: None,
            category_id: None,
            entries: &[
                crate::db::NewVideo {
                    video_id: "lw_batch000",
                    channel_id: ch,
                    channel_title: "CH",
                    title: "Go Live",
                    thumbnail_url: None,
                    published_at: Some("2026-11-27T08:00:00+00:00"),
                    kind: "upcoming",
                },
                crate::db::NewVideo {
                    video_id: "lw_batch001",
                    channel_id: ch,
                    channel_title: "CH",
                    title: "Ended Premiere",
                    thumbnail_url: None,
                    published_at: Some("2026-11-27T08:00:00+00:00"),
                    kind: "premiere",
                },
            ],
            etag: None,
            last_modified: None,
        })
        .unwrap();

        for i in 0..55 {
            let vid = format!("lw_batch{i:03}");
            db.live_wait_insert(&LiveWait {
                video_id: vid.clone(),
                title: vid,
                channel_id: Some(ch.to_string()),
                channel_title: Some("CH".to_string()),
                thumbnail_url: None,
                scheduled_start_ms: Some(now_ms + 3_600_000),
                registered_ms: now_ms,
            })
            .unwrap();
        }

        let (yt_base, reqs, srv) = spawn_lw_ytapi_mock(|line, _| {
            let id_q = line.split("id=").nth(1).unwrap().split(' ').next().unwrap();
            let ids: Vec<String> = id_q.split("%2C").map(|s| s.to_string()).collect();
            let items: Vec<String> = ids
                .iter()
                .map(|id| {
                    if id == "lw_batch000" {
                        r#"{"id":"lw_batch000","snippet":{"title":"Go Live","liveBroadcastContent":"live"},"contentDetails":{"duration":"P0D"},"liveStreamingDetails":{"actualStartTime":"2026-11-27T10:00:00Z"}}"#.to_string()
                    } else if id == "lw_batch001" {
                        r#"{"id":"lw_batch001","snippet":{"title":"Ended Premiere","liveBroadcastContent":"none"},"contentDetails":{"duration":"PT5M"},"liveStreamingDetails":{"actualStartTime":"2026-11-27T08:00:00Z","actualEndTime":"2026-11-27T08:05:00Z"}}"#.to_string()
                    } else {
                        format!(
                            r#"{{"id":"{id}","snippet":{{"title":"{id}","liveBroadcastContent":"upcoming"}},"contentDetails":{{"duration":"P0D"}},"liveStreamingDetails":{{"scheduledStartTime":"2026-11-27T12:00:00Z"}}}}"#
                        )
                    }
                })
                .collect();
            (200, format!(r#"{{"items":[{}]}}"#, items.join(",")))
        })
        .await;

        let ytapi = Arc::new(YtApi::with_test_base_url(
            Some("AIzaSyLW08_KEY".to_string()),
            yt_base,
        ));
        let tube = Arc::new(InnerTube::with_test_urls(
            "http://127.0.0.1:9/watch".to_string(),
            "http://127.0.0.1:9/api".to_string(),
            Duration::from_millis(100),
        ));
        let poller = LiveWaitPoller::new_test(db.clone(), ytapi, tube);
        poller.poll_due().await;

        // 55 件 -> 50 件 + 5 件の 2 チャンク
        {
            let recorded = lock(&reqs);
            assert_eq!(recorded.len(), 2);
            let c0 = recorded[0]
                .split("id=")
                .nth(1)
                .unwrap()
                .split(' ')
                .next()
                .unwrap()
                .matches("%2C")
                .count();
            let c1 = recorded[1]
                .split("id=")
                .nth(1)
                .unwrap()
                .split(' ')
                .next()
                .unwrap()
                .matches("%2C")
                .count();
            let mut counts = vec![c0 + 1, c1 + 1];
            counts.sort();
            assert_eq!(counts, vec![5, 50]);
        }

        // lw_batch000 (Live) と lw_batch001 (Ended) は待機解除され、残余 53 件が残る
        let remaining = db.live_wait_list().unwrap();
        assert_eq!(remaining.len(), 53);
        assert!(db.live_wait_get("lw_batch000").unwrap().is_none());
        assert!(db.live_wait_get("lw_batch001").unwrap().is_none());

        // ingested フィード同期と premiere 保持を検証
        let feed = db
            .feed_list_filtered(&crate::model::FeedFilter::default(), true, 10, |_| true)
            .unwrap();
        let f0 = feed.iter().find(|r| r.video_id == "lw_batch000").unwrap();
        let f1 = feed.iter().find(|r| r.video_id == "lw_batch001").unwrap();
        assert_eq!(f0.broadcast_state.as_deref(), Some("live"));
        assert_eq!(f0.kind, "live");
        assert_eq!(f1.broadcast_state.as_deref(), Some("ended"));
        assert_eq!(f1.kind, "premiere");

        srv.abort();
    }

    /// DB-LW-09: `videos.list` HTTP 200 応答からの単発欠落で `live_wait` 行も `videos.broadcast_state` も削除せず次回へ繰り越すことを検証する。
    #[tokio::test]
    async fn poll_due_preserves_live_wait_and_broadcast_state_on_single_omission() {
        let db = Db::connect_in_memory().unwrap();
        let ch = "UClw0900000000000000001";
        let now_ms = epoch_ms();
        db.feed_subscribe(&crate::db::SubscribeArgs {
            channel_id: ch,
            title: "CH",
            thumbnail_url: None,
            category_id: None,
            entries: &[crate::db::NewVideo {
                video_id: "lw_omit_001",
                channel_id: ch,
                channel_title: "CH",
                title: "Omitted Wait",
                thumbnail_url: None,
                published_at: Some("2026-11-27T08:00:00+00:00"),
                kind: "upcoming",
            }],
            etag: None,
            last_modified: None,
        })
        .unwrap();
        db.videos_apply_broadcast_refresh(
            "lw_omit_001",
            Some("upcoming"),
            Some("upcoming"),
            Some(now_ms + 3_600_000),
            now_ms,
            false,
        )
        .unwrap();
        db.live_wait_insert(&LiveWait {
            video_id: "lw_omit_001".to_string(),
            title: "Omitted Wait".to_string(),
            channel_id: Some(ch.to_string()),
            channel_title: Some("CH".to_string()),
            thumbnail_url: None,
            scheduled_start_ms: Some(now_ms + 3_600_000),
            registered_ms: now_ms,
        })
        .unwrap();

        let (yt_base, _, srv) =
            spawn_lw_ytapi_mock(|_, _| (200, r#"{"items":[]}"#.to_string())).await;
        let ytapi = Arc::new(YtApi::with_test_base_url(
            Some("AIzaSyLW09_KEY".to_string()),
            yt_base,
        ));
        let tube = Arc::new(InnerTube::with_test_urls(
            "http://127.0.0.1:9/watch".to_string(),
            "http://127.0.0.1:9/api".to_string(),
            Duration::from_millis(100),
        ));
        let poller = LiveWaitPoller::new_test(db.clone(), ytapi, tube);
        poller.poll_due().await;

        assert!(db.live_wait_get("lw_omit_001").unwrap().is_some());
        let item = db
            .feed_list_filtered(&crate::model::FeedFilter::default(), true, 10, |_| true)
            .unwrap()
            .into_iter()
            .find(|r| r.video_id == "lw_omit_001")
            .unwrap();
        assert_eq!(item.broadcast_state.as_deref(), Some("upcoming"));
        assert!(lock(&poller.sched)
            .get("lw_omit_001")
            .and_then(|s| s.next_at)
            .is_some());

        srv.abort();
    }

    /// DB-LW-10: `ytapi` のクォータ上限・エラー時に `InnerTube` 個別巡回へ透過フォールバックすることを検証する。
    #[tokio::test]
    async fn poll_due_falls_back_to_innertube_on_ytapi_quota_error() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let db = Db::connect_in_memory().unwrap();
        let now_ms = epoch_ms();
        db.setting_set(SETTING_LIVE_WAIT_ACTION, "notify").unwrap();
        db.live_wait_insert(&LiveWait {
            video_id: "lw_fb_00001".to_string(),
            title: "Fallback Wait".to_string(),
            channel_id: None,
            channel_title: None,
            thumbnail_url: None,
            scheduled_start_ms: Some(now_ms + 3_600_000),
            registered_ms: now_ms,
        })
        .unwrap();

        let (yt_base, _, yt_srv) = spawn_lw_ytapi_mock(|_, _| {
            (
                403,
                r#"{"error":{"errors":[{"reason":"quotaExceeded"}]}}"#.to_string(),
            )
        })
        .await;

        let tube_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let tube_addr = tube_listener.local_addr().unwrap();
        let tube_srv = tokio::spawn(async move {
            let html = r#"<script>ytcfg.set({"INNERTUBE_API_KEY":"k","INNERTUBE_CONTEXT_CLIENT_VERSION":"v","VISITOR_DATA":"d"});</script>"#;
            while let Ok((mut stream, _)) = tube_listener.accept().await {
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
                    let body = r#"{"playabilityStatus":{"status":"OK"},"videoDetails":{"videoId":"lw_fb_00001","isLiveContent":true},"microformat":{"playerMicroformatRenderer":{"liveBroadcastDetails":{"isLiveNow":true,"startTimestamp":"2026-11-27T10:00:00+00:00"}}}}"#;
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    )
                };
                let _ = stream.write_all(resp.as_bytes()).await;
            }
        });

        let ytapi = Arc::new(YtApi::with_test_base_url(
            Some("AIzaSyLW10_KEY".to_string()),
            yt_base,
        ));
        let tube = Arc::new(InnerTube::with_test_urls(
            format!("http://{tube_addr}/watch"),
            format!("http://{tube_addr}/api"),
            Duration::from_secs(5),
        ));
        let poller = LiveWaitPoller::new_test(db.clone(), ytapi.clone(), tube);
        poller.poll_due().await;

        assert!(!ytapi.can_use(QuotaBucket::Shared));
        assert!(db.live_wait_get("lw_fb_00001").unwrap().is_none());

        yt_srv.abort();
        tube_srv.abort();
    }
}
