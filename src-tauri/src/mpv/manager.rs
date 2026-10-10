//! 全 mpv インスタンスの管理（`PlayerManager`）と履歴の永続化。
use super::player::{MpvPlayer, SpawnOptions};
use super::{
    is_valid_hdr_compute_peak, is_valid_pip_geometry, is_valid_tone_mapping,
    pip_fit_aspect_enabled, MpvError, DEFAULT_PIP_GEOMETRY, DEFAULT_YTDL_FORMAT,
    SETTING_HDR_COMPUTE_PEAK, SETTING_HDR_TONE_MAPPING, SETTING_MPV_EXTRA_ARGS,
    SETTING_PIP_FIT_ASPECT, SETTING_PIP_GEOMETRY, SETTING_WHEEL_VOLUME_DELTA,
};
use crate::db::Db;
use crate::model::{PlayerAction, PlayerEnded, PlayerState};
use crate::util::lock;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};
use tokio::task::JoinHandle;

/// `player://state` の送出間隔（設計書 §3.2 の 200〜500ms の中を取る）。
const EMIT_INTERVAL: Duration = Duration::from_millis(300);
/// 再生位置を `watch_history` へ永続化する間隔。
const PERSIST_INTERVAL: Duration = Duration::from_secs(5);

/// 全 mpv インスタンスの管理（設計書 §4.5 のマルチビュー前提）。
/// `play` が発行する `instance_id` が UI 側の操作対象識別子。
pub struct PlayerManager {
    /// emitter タスクが終端イベント時に自分のエントリを外すため Arc で共有する。
    players: Arc<Mutex<HashMap<u32, PlayerEntry>>>,
    next_id: AtomicU32,
    app: AppHandle,
    db: Db,
    socket_dir: PathBuf,
    /// yt-dlp パスの解決器（settings `ytdlp.path` → 同梱リソース → PATH）。
    ytdlp_resolver: crate::yt::YtDlpResolver,
    /// `--script` で読ませる wheel.lua のパス。書き出しに失敗した環境では None。
    wheel_script: Option<PathBuf>,
    /// SponsorBlock API 呼び出し用の HTTP クライアント（設計書 §4.4）。
    http: reqwest::Client,
}

struct PlayerEntry {
    player: Arc<MpvPlayer>,
    /// IPC イベントポンプ。
    pump: JoinHandle<()>,
    /// 状態サンプリング送出と履歴永続化のタスク。終了通知で自然終了する。
    emitter: JoinHandle<()>,
}

impl PlayerManager {
    pub fn new(
        app: AppHandle,
        db: Db,
        socket_dir: PathBuf,
        ytdlp_resolver: crate::yt::YtDlpResolver,
        wheel_script: Option<PathBuf>,
    ) -> Self {
        Self {
            players: Arc::new(Mutex::new(HashMap::new())),
            next_id: AtomicU32::new(1),
            app,
            db,
            socket_dir,
            ytdlp_resolver,
            wheel_script,
            http: reqwest::Client::new(),
        }
    }

    /// `play_video` の実体。mpv 起動→監視タスク起動→履歴行の確保まで行う。
    /// `start_sec` はレジューム位置（0 で先頭）。
    /// `pip` が true なら最前面・枠なしの小窓で起動する（設計書 §4.5）。
    pub async fn play(
        &self,
        video_id: &str,
        start_sec: f64,
        ytdl_format: Option<String>,
        pip: bool,
    ) -> Result<u32, MpvError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let ytdlp_path = self.ytdlp_resolver.resolve(&self.db).await;
        // 音量変化量は数値として解釈できる値だけを script-opts に渡す
        let wheel_volume_delta = self
            .db
            .setting_get(SETTING_WHEEL_VOLUME_DELTA)
            .ok()
            .flatten()
            .filter(|v| v.trim().parse::<f64>().is_ok());
        // HDR 設定（仕様決定 Y）。値は受理集合に検証してから渡し、
        // `auto` や空欄・不正値は未指定として mpv 既定に任せる
        let tone_mapping = self
            .db
            .setting_get(SETTING_HDR_TONE_MAPPING)
            .ok()
            .flatten()
            .map(|s| s.trim().to_string())
            .filter(|s| is_valid_tone_mapping(s));
        let hdr_compute_peak = self
            .db
            .setting_get(SETTING_HDR_COMPUTE_PEAK)
            .ok()
            .flatten()
            .map(|s| s.trim().to_string())
            .filter(|s| is_valid_hdr_compute_peak(s));
        let extra_args = self
            .db
            .setting_get(SETTING_MPV_EXTRA_ARGS)
            .ok()
            .flatten()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let opts = SpawnOptions {
            video_id: video_id.to_string(),
            start_sec,
            ytdl_format: ytdl_format.unwrap_or_else(|| DEFAULT_YTDL_FORMAT.to_string()),
            ytdlp_path,
            wheel_script: self.wheel_script.clone(),
            wheel_volume_delta,
            pip_geometry: pip.then(|| self.pip_geometry()),
            // 追従フラグは PiP 起動かどうかに関わらず解決して持たせる
            // （稼働中に PiP 化したときにも参照される、仕様決定 AL）
            pip_fit_aspect: self.pip_fit_aspect(),
            tone_mapping,
            hdr_compute_peak,
            extra_args,
        };
        let (player, pump) = MpvPlayer::spawn(id, &self.socket_dir, opts, self.app.clone()).await?;
        let emitter = self.spawn_emitter(player.clone());

        Self::spawn_sponsor_fetch(&self.db, &self.http, player.clone(), video_id.to_string());

        lock(&self.players).insert(
            id,
            PlayerEntry {
                player: player.clone(),
                pump,
                emitter,
            },
        );
        // `pip.fit_aspect` の起動中変更を取りこぼさない。spawn 時の解決値を
        // 持って登録されるが、set_pip_fit_all は管理表を見るため、登録前に
        // 保存された変更は届かない。登録時点の DB 値を読み直して反映する
        // （変化がなければ set_pip_fit 側で早期 return する）
        player.set_pip_fit(self.pip_fit_aspect()).await;
        // 再生開始時点で履歴行を確保（タイトルは media-title 変化で追従）
        if let Err(e) = self.db.history_upsert(video_id) {
            tracing::warn!(video_id, error = %e, "履歴行の作成に失敗");
        }
        Ok(id)
    }

    /// SponsorBlock の区間取得をバックグラウンドで行い、判定用にプレイヤーへ差し込む。
    /// 取得失敗は再生を阻害しない（スキップが動かないだけ）。連続再生の次項目でも再利用する。
    fn spawn_sponsor_fetch(
        db: &Db,
        http: &reqwest::Client,
        player: Arc<MpvPlayer>,
        video_id: String,
    ) {
        let http = http.clone();
        let categories = db
            .setting_get(crate::sponsor::SETTING_CATEGORIES)
            .ok()
            .flatten();
        tokio::spawn(async move {
            let cats = crate::sponsor::parse_categories(categories.as_deref());
            match crate::sponsor::fetch_segments(&http, &video_id, &cats).await {
                Ok(segs) => player.set_sponsor_segments(segs),
                Err(e) => tracing::warn!(video_id, error = %e, "SponsorBlock の区間取得に失敗"),
            }
        });
    }

    /// 連続再生の武装キューを登録する（`player_set_queue` 経路、FR-10、仕様決定 S・AD）。
    /// `items` は今後再生する項目の順序列。終端（自然終了・途中失敗）のたびに
    /// 同一 mpv が先頭から再生する。`loop_all` で取り出し分を末尾へ戻して巡回する。
    /// 空列は武装の解除。存在しないインスタンスでは MpvError::NoSuchInstance。
    /// `base_seq` 指定時は取り出し世代が一致するときだけ適用し、
    /// 拒否した場合は Ok(false) を返す（世代ずれはエラーではない）。
    pub async fn set_queue(
        &self,
        instance_id: u32,
        items: Vec<String>,
        loop_all: bool,
        base_seq: Option<u64>,
    ) -> Result<bool, MpvError> {
        let player = {
            let g = lock(&self.players);
            g.get(&instance_id).map(|e| e.player.clone())
        }
        .ok_or(MpvError::NoSuchInstance(instance_id))?;
        Ok(player.set_queue(items, loop_all, base_seq))
    }

    /// 稼働中インスタンスのスナップショット一覧。
    /// ページ再読み込み後に `player://state` が流れない一時停止中の
    /// カードを復元するための一覧取得に使う。
    pub fn list(&self) -> Vec<PlayerState> {
        lock(&self.players)
            .values()
            .map(|e| e.player.snapshot())
            .collect()
    }

    /// 指定動画を再生中のインスタンスの再生位置（秒）。
    /// 複数インスタンスが同一動画を再生中なら最初に見つかったもの。
    /// 見つからなければ None（チャットリプレイの照合用、FR-24）。
    pub fn position_of(&self, video_id: &str) -> Option<f64> {
        lock(&self.players)
            .values()
            .map(|e| e.player.snapshot())
            .find(|s| s.video_id == video_id)
            .map(|s| s.position)
    }

    /// 指定インスタンスが指定動画を再生中ならその位置（秒）。
    /// キュー遷移等で別動画へ移ったインスタンスは対象外にする。
    /// リプレイの同期先を、パネルを開いたインスタンスへ固定するのに使う
    /// （同一動画の複数窓で位置がずれないよう、FR-24）。
    pub fn position_of_instance(&self, instance_id: u32, video_id: &str) -> Option<f64> {
        lock(&self.players)
            .get(&instance_id)
            .map(|e| e.player.snapshot())
            .filter(|s| s.video_id == video_id)
            .map(|s| s.position)
    }

    /// `player_control` の実体。
    /// `Pip` は設定値 `pip.geometry` を参照してここで処理し、
    /// 残りはプレイヤー固有の `control` に委譲する。
    pub async fn control(&self, instance_id: u32, action: &PlayerAction) -> Result<(), MpvError> {
        let player = {
            let players = lock(&self.players);
            players.get(&instance_id).map(|e| e.player.clone())
        };
        let player = player.ok_or(MpvError::NoSuchInstance(instance_id))?;
        match action {
            PlayerAction::Pip { enabled } => {
                let geometry = self.pip_geometry();
                player.set_pip(*enabled, &geometry).await
            }
            _ => player.control(action).await,
        }
    }

    /// `pip.geometry` 設定値を検証して返す。無効・未設定は既定値に戻す。
    fn pip_geometry(&self) -> String {
        self.db
            .setting_get(SETTING_PIP_GEOMETRY)
            .ok()
            .flatten()
            .map(|s| s.trim().to_string())
            .filter(|s| is_valid_pip_geometry(s))
            .unwrap_or_else(|| DEFAULT_PIP_GEOMETRY.to_string())
    }

    /// `pip.fit_aspect` 設定値の解釈（仕様決定 AL）。未設定・その他は on。
    fn pip_fit_aspect(&self) -> bool {
        pip_fit_aspect_enabled(self.db.setting_get(SETTING_PIP_FIT_ASPECT).ok().flatten())
    }

    /// `pip.fit_aspect` の変更を稼働中の全インスタンスへ反映する
    /// （`settings_set` 経路、仕様決定 AL）。PiP 中のインスタンスは
    /// on なら映像比率へのフィット、off なら基底 geometry の固定サイズへ
    /// 即座に戻る。
    pub async fn set_pip_fit_all(&self, enabled: bool) {
        let players: Vec<Arc<MpvPlayer>> = lock(&self.players)
            .values()
            .map(|e| e.player.clone())
            .collect();
        for p in players {
            p.set_pip_fit(enabled).await;
        }
    }

    /// `player_close` の実体。最終位置を保存してから mpv を止める。
    /// 既に終端処理済み（emitter が除去済み）のインスタンスに対しても冪等に成功する。
    pub async fn close(&self, instance_id: u32) -> Result<(), MpvError> {
        let entry = lock(&self.players).remove(&instance_id);
        let Some(PlayerEntry {
            player,
            pump,
            emitter,
        }) = entry
        else {
            // 終端イベントで既に掃除済み。close は冪等に成功させる。
            tracing::debug!(instance_id, "close 対象のインスタンスは既に存在しない");
            return Ok(());
        };
        emitter.abort();
        pump.abort();
        // 終端理由が eof なら completed を維持して保存する（途中保存で上書きしない）
        self.persist_history(&player, player.terminal_completed());
        player.shutdown().await;
        Ok(())
    }

    /// アプリ終了時のクリーンアップ。全インスタンスを止める。
    pub async fn close_all(&self) {
        let entries: Vec<PlayerEntry> = lock(&self.players).drain().map(|(_, e)| e).collect();
        for PlayerEntry {
            player,
            pump,
            emitter,
        } in entries
        {
            emitter.abort();
            pump.abort();
            self.persist_history(&player, player.terminal_completed());
            player.shutdown().await;
        }
    }

    /// 状態のサンプリング送出と履歴の定期保存。
    /// 終了通知を受けたら最終保存・UI への ended 送出をして、エントリを外して mpv を止める。
    /// 終端で mpv プロセスを残さないのは、UI がカードを外した後もプロセスが浮遊するのを防ぐため。
    fn spawn_emitter(&self, player: Arc<MpvPlayer>) -> JoinHandle<()> {
        let app = self.app.clone();
        let db = self.db.clone();
        let http = self.http.clone();
        let players = Arc::clone(&self.players);
        let mut ended_rx = player.subscribe_ended();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(EMIT_INTERVAL);
            let mut last_sent: Option<PlayerState> = None;
            let mut last_persist = Instant::now();
            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        let snap = player.snapshot();
                        if last_sent.as_ref() != Some(&snap) {
                            if let Err(e) = app.emit("player://state", &snap) {
                                tracing::warn!(error = %e, "player://state の送出に失敗");
                            }
                            last_sent = Some(snap);
                        }
                        if last_persist.elapsed() >= PERSIST_INTERVAL {
                            persist_now(&db, &player, false);
                            last_persist = Instant::now();
                        }
                    }
                    reason = ended_rx.recv() => {
                        let reason = reason.unwrap_or_else(|_| "unknown".into());
                        let completed = reason == "eof";
                        persist_now(&db, &player, completed);
                        let ended_video = player.video_id();
                        // 連続再生: 武装した次項目があれば同一 mpv で先頭から読み込む
                        // （FR-10、仕様決定 S。途中失敗の終端でも次へ進む暫定仕様）。
                        // ロードに失敗した場合は通常どおり終端処理へ進む。
                        let mut continued = false;
                        let mut continued_video_id: Option<String> = None;
                        if let Some(next_id) = player.take_next() {
                            match player.load_video(&next_id).await {
                                Ok(()) => {
                                    continued = true;
                                    continued_video_id = Some(next_id.clone());
                                    if let Err(e) = db.history_upsert(&next_id) {
                                        tracing::warn!(video_id = %next_id, error = %e, "履歴行の作成に失敗");
                                    }
                                    Self::spawn_sponsor_fetch(&db, &http, player.clone(), next_id);
                                }
                                Err(e) => {
                                    tracing::warn!(video_id = %next_id, error = %e, "連続再生の次項目ロードに失敗");
                                }
                            }
                        }
                        let payload = PlayerEnded {
                            instance_id: player.instance_id(),
                            video_id: ended_video,
                            reason,
                            continued,
                            continued_video_id,
                            armed_seq: player.armed_seq(),
                        };
                        if let Err(e) = app.emit("player://ended", &payload) {
                            tracing::warn!(error = %e, "player://ended の送出に失敗");
                        }
                        if continued {
                            continue;
                        }
                        // 管理表から外し、ポンプを止めて mpv を終了させる。
                        // close() との競合は先に外した側が掃除を担い、後着側は冪等に成功する。
                        if let Some(entry) = lock(&players).remove(&player.instance_id()) {
                            entry.pump.abort();
                            drop(entry.emitter);
                        }
                        player.shutdown().await;
                        break;
                    }
                }
            }
        })
    }

    /// 終了時 / close 時の履歴保存。`completed` は終了理由 eof のときのみ立てる。
    fn persist_history(&self, player: &MpvPlayer, completed: bool) {
        persist_now(&self.db, player, completed);
    }
}

fn persist_now(db: &Db, player: &MpvPlayer, completed: bool) {
    let snap = player.snapshot();
    // mpv はタイトル未取得時に URL 片（`watch?v=...` や `https://...`）を
    // media-title に置く。その値を履歴へ書くとライブラリ表示が壊れるため、
    // URL 形式のタイトルは空として保存しない（行は既存タイトルを維持する）。
    let title = sanitize_media_title(&snap.media_title);
    if let Err(e) = db.history_update_progress(
        &snap.video_id,
        title,
        snap.position,
        (snap.duration > 0.0).then_some(snap.duration as i64),
        completed,
    ) {
        tracing::warn!(video_id = %snap.video_id, error = %e, "履歴の保存に失敗");
    }
}

/// 履歴保存用に media-title を検証する。URL 形式の値はタイトル未取得の
/// フォールバックなので空文字へ潰す。
fn sanitize_media_title(title: &str) -> &str {
    let t = title.trim();
    if t.starts_with("watch?") || t.starts_with("http://") || t.starts_with("https://") {
        ""
    } else {
        t
    }
}
