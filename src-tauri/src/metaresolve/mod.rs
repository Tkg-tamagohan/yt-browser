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
use std::time::Duration;

use tauri::{AppHandle, Emitter};
use thiserror::Error;
use tokio::sync::Notify;
use tracing::{debug, warn};

use crate::db::{Db, DbError, MetaResolveTarget};
use crate::innertube::{InnerTube, InnerTubeError, MetaSnapshot};
use crate::model::FeedNewItems;
use crate::util::lock;

/// 巡回間隔。フィードの新着投入時には `wake_now` で即時起こされる。
const TICK: Duration = Duration::from_secs(60);
/// 1 サイクルで処理する行数の上限（仕様決定 CD の処理量の上限化）。
const BATCH_MAX: usize = 20;
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

/// InnerTube メタ応答から放送系 kind を分類する（FR-56、仕様決定 CG）。
/// `liveBroadcastDetails` と `videoDetails` 系の値・再生状態を材料にする。
/// 戻り値は `kind` の確定値。放送系の手掛かりが無いときは `None`
/// （呼び出し側は `kind='video'` の行だけ shorts 判定へ回す）。
///
/// 仕様の明示しない部分は暫定解釈:
/// - `isLiveNow` 真かつ `isLiveContent` 偽 → 'premiere'（通常動画資産の公開中放送）
/// - 終了済み放送で `isLiveContent` 真 → 'live'（ライブのアーカイブ）
/// - 終了済み放送で `isLiveContent` 偽/欠落 → 'premiere'（実応答の組み合わせ由来）
/// - details を持たない応答でも `isLiveContent` 真 → 'live'、
///   `playabilityStatus.status` が LIVE_STREAM_OFFLINE → 'upcoming'
///   （再生状態だけは判定の補助に使う）
fn classify_kind(s: &MetaSnapshot) -> Option<&'static str> {
    let b = &s.broadcast;
    if b.is_live_now == Some(true) {
        return Some(if s.is_live_content == Some(false) {
            "premiere"
        } else {
            "live"
        });
    }
    if b.has_details || b.is_upcoming {
        if b.end_ms.is_some() {
            return Some(if s.is_live_content == Some(true) {
                "live"
            } else {
                "premiere"
            });
        }
        return Some("upcoming");
    }
    if s.is_live_content == Some(true) {
        return Some("live");
    }
    if s.playability.as_deref() == Some("LIVE_STREAM_OFFLINE") {
        return Some("upcoming");
    }
    None
}

#[derive(Debug, Error)]
enum ResolveError {
    #[error("InnerTube メタ取得に失敗: {0}")]
    Tube(#[from] InnerTubeError),
    #[error("DB 更新に失敗: {0}")]
    Db(#[from] DbError),
    #[error("shorts 判定リクエストに失敗: {0}")]
    Shorts(reqwest::Error),
}

/// メタの遅延解決ワーカー（仕様決定 CD・CG・CH・DJ）。`run` を起動時に
/// spawn する。対象の選出は `db.meta_resolve_targets`（kind_state /
/// published_at / meta_checked / description で絞る）、書き戻しは
/// `videos_set_published_at` / `videos_resolve_kind` /
/// `videos_set_kind_state` / `videos_set_description`。
pub struct MetaResolver {
    db: Db,
    app: AppHandle,
    inner: Arc<InnerTube>,
    /// shorts 判定用クライアント（仕様決定 V）。feed の detect_client と同じく
    /// 非 short は /watch へ 303 で転送されるためリダイレクトを追跡しない。
    detect_client: reqwest::Client,
    /// 新規投入などで即時回したいときの通知（feed の shorts 判定起動と併せて鳴らす）。
    wake: Notify,
    /// この起動内での video_id → 連続失敗回数。`FAIL_MAX` 以上は選出から外す
    /// （失敗行が選出順を占有して新しい行へ届かなくなるのを防ぐ）。
    fails: Mutex<HashMap<String, u32>>,
}

impl MetaResolver {
    pub fn new(db: Db, app: AppHandle, innertube: Arc<InnerTube>) -> Arc<Self> {
        Arc::new(Self {
            db,
            app,
            inner: innertube,
            detect_client: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .expect("リダイレクト非追跡クライアントの生成に失敗"),
            wake: Notify::new(),
            fails: Mutex::new(HashMap::new()),
        })
    }

    /// 新規投入などでワーカーを即時回したいときに呼ぶ。
    pub fn wake_now(&self) {
        self.wake.notify_one();
    }

    /// メインループ。TICK ごと（または wake で即時）に対象行を処理する。
    /// 対象ゼロの間は次の契機まで眠る。
    pub async fn run(self: Arc<Self>) {
        loop {
            tokio::select! {
                _ = self.wake.notified() => {},
                _ = tokio::time::sleep(TICK) => {},
            }
            self.resolve_once().await;
        }
    }

    /// 1 サイクル分の解決。候補を DB から取り、起動内失敗上限に達した行を
    /// 外して最大 `BATCH_MAX` 件まで順次処理する。kind 変化・published_at
    /// または description の埋めが 1 件でもあれば `feed://kind_updated` を
    /// 出す（イベント名の維持。ペイロードは変化件数。description が NG
    /// フィルタ `video_desc` の評価に載るため、説明の解決でも一覧の
    /// 再読込が要る。FR-85、仕様決定 DJ）。
    async fn resolve_once(&self) {
        let targets = match self.db.meta_resolve_targets(CANDIDATES) {
            Ok(t) => t,
            Err(e) => {
                warn!(error = %e, "遅延解決対象の選出に失敗");
                return;
            }
        };
        let targets: Vec<MetaResolveTarget> = {
            let fails = lock(&self.fails);
            targets
                .into_iter()
                .filter(|t| fails.get(&t.video_id).copied().unwrap_or(0) < FAIL_MAX)
                .take(BATCH_MAX)
                .collect()
        };
        if targets.is_empty() {
            return;
        }
        let mut changed = 0usize;
        for t in &targets {
            match self.resolve_one(t).await {
                Ok(n) => {
                    changed += n;
                    lock(&self.fails).remove(&t.video_id);
                }
                Err(e) => {
                    *lock(&self.fails).entry(t.video_id.clone()).or_insert(0) += 1;
                    // 失敗の記録（kind_state=2）。既に別経路で確定済みなら触らない。
                    if t.needs_kind {
                        if let Err(e2) = self.db.videos_set_kind_state(&t.video_id, KIND_FAILED) {
                            warn!(error = %e2, "kind 失敗状態の記録に失敗");
                        }
                    }
                    debug!(video_id = %t.video_id, error = %e, "遅延解決の試行に失敗");
                }
            }
            tokio::time::sleep(ITEM_SPACING).await;
        }
        if changed > 0 {
            let payload = FeedNewItems { count: changed };
            if let Err(e) = self.app.emit("feed://kind_updated", payload) {
                warn!(error = %e, "feed://kind_updated の送出に失敗");
            }
        }
    }

    /// 1 行分の解決。InnerTube メタ応答 1 件で published_at・kind・
    /// description をまとめて埋める（仕様決定 CD の共用。description は
    /// FR-85・仕様決定 DJ で追加）。戻り値は UI 更新を要する変化の件数。
    async fn resolve_one(&self, t: &MetaResolveTarget) -> Result<usize, ResolveError> {
        let mut changed = 0usize;
        let snap = if t.needs_published || t.needs_kind || t.needs_desc {
            let s = self.inner.meta_snapshot(&t.video_id).await?;
            if t.needs_published {
                if let Some(at) = &s.published_at {
                    if self.db.videos_set_published_at(&t.video_id, at)? {
                        changed += 1;
                    }
                }
            }
            // 応答を得た時点で試行済み。publishDate を持たない応答でも立て、
            // 投稿日時だけが欠けた行を「もう一度問う」対象から外す。
            self.db.videos_mark_meta_checked(&t.video_id)?;
            // 説明文は応答に無くても空文字を書いて解決済みにする
            // （説明なしの行へ繰り返し問い合わせないため。FR-85、仕様決定 DJ）。
            // 書き込みは NULL の行だけで、NG 評価が変わり得るので変化に数える。
            if t.needs_desc
                && self
                    .db
                    .videos_set_description(&t.video_id, &s.description)?
            {
                changed += 1;
            }
            Some(s)
        } else {
            None
        };

        if t.needs_kind {
            let s = snap.as_ref().expect("needs_kind ならメタ応答は取得済み");
            match classify_kind(s) {
                Some(kind) => {
                    // 'short' は既存の値を守り、それ以外は確定値へ置き換える
                    if self.db.videos_resolve_kind(&t.video_id, kind)? {
                        changed += 1;
                    }
                }
                None => {
                    // 放送系の手掛かり無し。kind='video' の行だけ shorts 判定へ
                    // 回す（仕様決定 V・CH の再試行）。'live'/'upcoming' 等の
                    // 推定値が付いた行は由来を残したまま確定する（暫定解釈:
                    // 放送予定が応答にまだ載らない時期を誤確定しない）。
                    if t.kind == "video" {
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
                    } else {
                        self.db.videos_set_kind_state(&t.video_id, KIND_RESOLVED)?;
                    }
                }
            }
        }
        Ok(changed)
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
    use crate::innertube::BroadcastStatus;

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
}
