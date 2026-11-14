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
use crate::innertube::{BroadcastStatus, InnerTube};
use crate::model::{LiveWait, LiveWaitChanged, LiveWaitStatus};
use crate::util::lock;

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
        Some(s) if (s - NEAR_BEFORE_MS..=s + NEAR_AFTER_MS).contains(&now_ms) => {
            POLL_SHORT_MS
        }
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

/// ライブ待機のポーラー（仕様決定 BT(4)(5)）。`run` を起動時に spawn する。
/// 待機リストは DB へ write-through するため再起動でそのまま復元される。
pub struct LiveWaitPoller {
    db: Db,
    app: AppHandle,
    innertube: Arc<InnerTube>,
    /// video_id → 次回ポーリング予定と失敗回数。
    sched: Mutex<HashMap<String, WaitSchedule>>,
    /// 新規登録時に即時巡回を起こすための通知。
    wake: Notify,
}

impl LiveWaitPoller {
    pub fn new(db: Db, app: AppHandle, innertube: Arc<InnerTube>) -> Arc<Self> {
        Arc::new(Self {
            db,
            app,
            innertube,
            sched: Mutex::new(HashMap::new()),
            wake: Notify::new(),
        })
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
        for wait in due {
            // ポーリング中に解除される場合に備えて、処理直前に存在を確認する
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
            self.poll_one(&wait).await;
        }
    }

    /// 1 項目のポーリング。自動解除 → 放送状態の分類 → 遷移処理の順。
    async fn poll_one(&self, wait: &LiveWait) {
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
        match self.innertube.broadcast_status(&wait.video_id).await {
            Err(e) => self.after_failure(wait, e.to_string()),
            Ok(status) => {
                self.after_success(wait);
                match classify_broadcast(&status) {
                    WaitBroadcast::Live => self.on_live(wait).await,
                    WaitBroadcast::Ended => self.remove_wait(
                        wait,
                        "info",
                        format!("配信は終了しています（待機を解除しました）: {}", wait.title),
                    ),
                    WaitBroadcast::Upcoming { scheduled_ms } => {
                        // InnerTube 側の予定変更へ追随する（表示の予定時刻と
                        // 直前窓の判定がずれないよう保存値を更新）
                        if scheduled_ms != wait.scheduled_start_ms {
                            if let Err(e) = self
                                .db
                                .live_wait_set_scheduled(&wait.video_id, scheduled_ms)
                            {
                                tracing::warn!(error = %e, "待機の予定時刻更新に失敗");
                            } else {
                                emit_changed(&self.app, Some(&wait.video_id));
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
        }
    }

    /// 開始検出（仕様決定 BT(3)）。待機を解除してから動作へ移る。
    /// `play` は新規インスタンスの自動再生、`notify` はトースト通知のみ。
    async fn on_live(&self, wait: &LiveWait) {
        self.remove_entry(&wait.video_id);
        let action = wait_action_of(
            self.db
                .setting_get(SETTING_LIVE_WAIT_ACTION)
                .ok()
                .flatten(),
        );
        match action {
            WaitAction::Play => match self.auto_play(wait).await {
                Ok(()) => emit_status(
                    &self.app,
                    Some(&wait.video_id),
                    "info",
                    format!("配信開始を検出し、自動再生を開始しました: {}", wait.title),
                ),
                Err(e) => emit_status(
                    &self.app,
                    Some(&wait.video_id),
                    "warn",
                    format!(
                        "配信開始を検出しましたが自動再生に失敗しました: {}（{}）",
                        wait.title, e
                    ),
                ),
            },
            WaitAction::Notify => emit_status(
                &self.app,
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
        let players = self.app.state::<crate::mpv::PlayerManager>();
        players
            .play(&wait.video_id, 0.0, ytdl_format, pip)
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
        emit_changed(&self.app, Some(video_id));
        removed
    }

    /// 解除＋一覧変更通知＋状態通知のまとめ（終了・自動解除の出口）。
    fn remove_wait(&self, wait: &LiveWait, level: &str, message: String) {
        self.remove_entry(&wait.video_id);
        emit_status(&self.app, Some(&wait.video_id), level, message);
    }

    /// 次回予定の設定。
    fn reschedule(&self, video_id: &str, delay_ms: i64) {
        let mut sched = lock(&self.sched);
        sched
            .entry(video_id.to_string())
            .or_default()
            .next_at = Some(
            Instant::now() + Duration::from_millis(u64::try_from(delay_ms).unwrap_or(0)),
        );
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
            emit_status(
                &self.app,
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
            emit_status(
                &self.app,
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
            classify_broadcast(&status(Some(false), Some(1000), None, Some(2000), true, true)),
            WaitBroadcast::Upcoming {
                scheduled_ms: Some(2000)
            }
        );
        // isUpcoming だけでも開始前（liveBroadcastDetails を持たない予定形）
        assert_eq!(
            classify_broadcast(&status(None, None, None, None, false, true)),
            WaitBroadcast::Upcoming {
                scheduled_ms: None
            }
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
            classify_broadcast(&status(Some(false), Some(500), Some(900), None, true, false)),
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
        assert_eq!(wait_action_of(Some("notify".to_string())), WaitAction::Notify);
        assert_eq!(wait_action_of(Some("play".to_string())), WaitAction::Play);
        assert_eq!(wait_action_of(Some("bogus".to_string())), WaitAction::Play);
        assert_eq!(wait_action_of(Some("".to_string())), WaitAction::Play);
    }
}
