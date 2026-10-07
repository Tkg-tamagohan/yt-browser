//! mpv プロセス管理と JSON IPC クライアント（設計書 §4.1）。
//! 1 再生 = 1 mpv プロセス + 1 IPC ソケット。マルチビューはインスタンスを増やすだけで済む
//! （仕様決定 B / FR-1）。再生制御はすべてこのモジュールを経由する。

mod ipc;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde_json::json;
use tauri::{AppHandle, Emitter};
use thiserror::Error;
use tokio::sync::{broadcast, mpsc};
use tokio::task::JoinHandle;

use crate::db::Db;
use crate::model::{PlayStatus, PlayerAction, PlayerEnded, PlayerState};

pub use ipc::IpcClient;
use ipc::IpcEvent;

/// `player://state` の送出間隔（設計書 §3.2 の 200〜500ms の中を取る）。
const EMIT_INTERVAL: Duration = Duration::from_millis(300);
/// 再生位置を `watch_history` へ永続化する間隔。
const PERSIST_INTERVAL: Duration = Duration::from_secs(5);
/// mpv プロセスが IPC ソケットを作るまでの待ち時間。
const SOCKET_WAIT_TIMEOUT: Duration = Duration::from_secs(5);
/// ソケット出現のポーリング間隔。
const SOCKET_POLL: Duration = Duration::from_millis(50);
/// quit 送信後、mpv の自発終了を待つ猶予。
const QUIT_GRACE: Duration = Duration::from_millis(800);

/// `observe_property` で監視する mpv プロパティ（設計書 §4.1）。
const OBSERVED_PROPERTIES: &[(u64, &str)] = &[
    (1, "time-pos"),
    (2, "duration"),
    (3, "pause"),
    (4, "eof-reached"),
    (5, "media-title"),
    (6, "paused-for-cache"),
    (7, "video-params"),
    (8, "volume"),
    (9, "speed"),
];

/// 未設定時の画質式（設計書 §4.3 の 1080p 上限プリセット）。
pub const DEFAULT_YTDL_FORMAT: &str = "bv*[height<=1080]+ba/b[height<=1080]";

/// ホイール分岐スクリプト（設計書 §4.2）。バイナリに埋め込み、
/// 起動時に app_data/mpv/wheel.lua へ書き出して `--script` で読ませる。
pub const WHEEL_LUA: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/mpv/wheel.lua"));
/// 設定キー: ホイール音量の変化量（script-opts `wheel-volume_delta` に渡す）。
pub const SETTING_WHEEL_VOLUME_DELTA: &str = "wheel.volume_delta";

#[derive(Debug, Error)]
pub enum MpvError {
    #[error("mpv の起動に失敗: {0}。mpv がインストールされているか確認してください")]
    Spawn(#[source] std::io::Error),
    #[error("mpv の IPC ソケットが時間内に作成されなかった（mpv 起動失敗の可能性）")]
    SocketTimeout,
    #[error("mpv IPC: {0}")]
    Ipc(#[from] ipc::IpcError),
    #[error("インスタンス {0} は存在しない（既に終了した可能性がある）")]
    NoSuchInstance(u32),
}

/// 終端イベント（end-file / ソケット切断）と意図的なリロードの区別を管理する。
/// `loadfile ... replace`（画質変更）は旧ファイルの `end-file` を発生させるが、
/// これを終了と取り違えないために `pending_replaces` で回数を数える。
#[derive(Default)]
struct TerminalTracker {
    /// 記録済みの終了理由。Some ならこのインスタンスは終端済み。
    ended_reason: Mutex<Option<String>>,
    /// 発行済みで未処理の `loadfile ... replace` の回数。
    pending_replaces: AtomicU32,
}

impl TerminalTracker {
    /// `loadfile ... replace` を発行する直前に呼ぶ。発生する end-file は無視される。
    fn begin_replace(&self) {
        self.pending_replaces.fetch_add(1, Ordering::SeqCst);
    }

    /// replace コマンド自体が失敗したときに予約を取り消す。
    fn cancel_replace(&self) {
        self.pending_replaces.fetch_sub(1, Ordering::SeqCst);
    }

    /// 新しいファイルのロード完了。end-file の取りこぼしでカウンタが残った場合の掃除。
    fn on_file_loaded(&self) {
        self.pending_replaces.store(0, Ordering::SeqCst);
    }

    /// end-file を終端イベントとして処理するなら true を返し理由を記録する。
    /// replace 由来の end-file は消化して false を返す。二重記録はしない。
    fn on_end_file(&self, reason: &str) -> bool {
        if self.pending_replaces.load(Ordering::SeqCst) > 0 {
            self.pending_replaces.fetch_sub(1, Ordering::SeqCst);
            return false;
        }
        self.record(reason)
    }

    /// `eof-reached=true`（keep-open 下で end-file が来ない経路の終端）。
    /// replace 予約を消化しない: キュー済みの旧ファイル EOF 通知が予約を消して
    /// 続く replace 由来の end-file を終端にしてしまう競合を防ぐ。
    /// 予約中に無視した EOF は、新ファイルが同じ末尾位置から再開されて
    /// すぐ再度 eof-reached になるため、最終的には終端として受理される。
    fn on_eof(&self) -> bool {
        if self.pending_replaces.load(Ordering::SeqCst) > 0 {
            return false;
        }
        self.record("eof")
    }

    /// 終端理由を一度だけ記録する。既に記録済みなら false。
    fn record(&self, reason: &str) -> bool {
        let mut ended = lock(&self.ended_reason);
        if ended.is_some() {
            return false;
        }
        *ended = Some(reason.to_string());
        true
    }

    /// ソケット切断。まだ終端が記録されていなければ "process_exit" で記録して true。
    fn on_disconnect(&self) -> bool {
        self.record("process_exit")
    }

    /// 履歴保存用の completed 判定。終端理由が eof のときのみ true。
    fn completed(&self) -> bool {
        lock(&self.ended_reason).as_deref() == Some("eof")
    }
}

/// mpv プロセス 1 台分の制御ハンドル。
pub struct MpvPlayer {
    instance_id: u32,
    video_id: String,
    ipc: IpcClient,
    /// wait/kill のため tokio Mutex で保持（async 文脈で &mut を取る）。
    child: tokio::sync::Mutex<tokio::process::Child>,
    socket_path: PathBuf,
    /// reader タスクが更新し、emitter タスクがサンプリングして UI へ流す。
    state: Mutex<PlayerState>,
    /// 終了通知（end-file / ソケット切断）。payload は mpv の reason 文字列。
    ended_tx: broadcast::Sender<String>,
    /// 終端判定と意図的リロードの区別。
    terminal: TerminalTracker,
    /// `sponsor://skipped` の送出に使う（設計書 §3.2）。
    app: AppHandle,
    /// SponsorBlock の区間と発火済みフラグ（設計書 §4.4）。
    sponsor: Mutex<SponsorState>,
}

/// SponsorBlock の判定状態。区間は再生開始後のバックグラウンド取得で差し込まれる。
#[derive(Default)]
pub struct SponsorState {
    pub segments: Vec<crate::sponsor::ActiveSegment>,
    /// 各区間につき 1 回だけ発火させるための消化済みインデックス。
    fired: HashSet<usize>,
}

/// `PlayerManager::play` に渡す起動条件。
pub struct SpawnOptions {
    pub video_id: String,
    /// レジューム開始位置（秒）。0 なら先頭から。
    pub start_sec: f64,
    /// `--ytdl-format` に渡す画質式。
    pub ytdl_format: String,
    /// `ytdl_hook-ytdl_path` に渡す yt-dlp のパス。None なら mpv の既定解決に任せる。
    pub ytdlp_path: Option<String>,
    /// `--script` に渡す wheel.lua のパス（app_data/mpv/wheel.lua）。
    pub wheel_script: Option<PathBuf>,
    /// `wheel-volume_delta` に渡す音量変化量。None なら Lua 既定（2）。
    pub wheel_volume_delta: Option<String>,
}

impl MpvPlayer {
    /// mpv を起動し、IPC ソケットに接続してプロパティ監視とファイルロードを行う。
    /// 戻り値の `JoinHandle` はイベントポンプ（IPC イベント→状態変換）で、
    /// 呼び出し側（PlayerManager）が保持して終了時に abort する。
    pub async fn spawn(
        instance_id: u32,
        socket_dir: &Path,
        opts: SpawnOptions,
        app: AppHandle,
    ) -> Result<(Arc<Self>, JoinHandle<()>), MpvError> {
        let socket_path = socket_dir.join(format!("mpv-{instance_id}.sock"));
        // 同名ソケットが残っていれば掃除する（前回プロセスの残骸対策）
        let _ = tokio::fs::remove_file(&socket_path).await;

        let mut args: Vec<String> = vec![
            "--idle=yes".into(),
            format!("--input-ipc-server={}", socket_path.display()),
            "--ytdl=yes".into(),
            format!("--ytdl-format={}", opts.ytdl_format),
            "--hr-seek=yes".into(),
            "--cache=yes".into(),
            "--hwdec=auto-safe".into(),
            "--keep-open=yes".into(),
            // アプリ識別のためタイトルに動画 ID を入れる
            format!("--title={}-yt-browser", opts.video_id),
        ];
        // --script-opts はリスト型で、同じ指定を重ねると後が前を上書きする。
        // そのため全エントリを 1 つのカンマ区切り値にまとめて渡す。
        let mut script_opts = Vec::new();
        if let Some(path) = &opts.ytdlp_path {
            // 同梱 / システム混在環境でどちらを使うか確定させる（設計書 §4.1）
            script_opts.push(format!("ytdl_hook-ytdl_path={path}"));
        }
        if let Some(delta) = &opts.wheel_volume_delta {
            script_opts.push(format!("wheel-volume_delta={delta}"));
        }
        if !script_opts.is_empty() {
            args.push(format!("--script-opts={}", script_opts.join(",")));
        }
        if let Some(script) = &opts.wheel_script {
            args.push(format!("--script={}", script.display()));
        }
        let mut child = tokio::process::Command::new("mpv")
            .args(&args)
            .kill_on_drop(true)
            .spawn()
            .map_err(MpvError::Spawn)?;

        if let Err(e) = wait_for_socket(&socket_path, &mut child).await {
            cleanup_failed_spawn(&mut child, &socket_path).await;
            return Err(e);
        }

        let (ev_tx, ev_rx) = mpsc::channel(64);
        let ipc = match IpcClient::connect(&socket_path, ev_tx).await {
            Ok(ipc) => ipc,
            Err(e) => {
                cleanup_failed_spawn(&mut child, &socket_path).await;
                return Err(e.into());
            }
        };

        for (id, name) in OBSERVED_PROPERTIES {
            if let Err(e) = ipc
                .command(vec![json!("observe_property"), json!(id), json!(name)])
                .await
            {
                cleanup_failed_spawn(&mut child, &socket_path).await;
                return Err(MpvError::Ipc(e));
            }
        }

        let (ended_tx, _) = broadcast::channel(8);
        let player = Arc::new(Self {
            instance_id,
            video_id: opts.video_id.clone(),
            ipc,
            child: tokio::sync::Mutex::new(child),
            socket_path,
            state: Mutex::new(PlayerState {
                instance_id,
                video_id: opts.video_id,
                pause: false,
                position: 0.0,
                duration: 0.0,
                fps: 0.0,
                state: PlayStatus::Idle,
                volume: 100.0,
                speed: 1.0,
                media_title: String::new(),
            }),
            ended_tx,
            terminal: TerminalTracker::default(),
            app,
            sponsor: Mutex::new(SponsorState::default()),
        });

        // IPC イベント → 状態スナップショット/終了通知への変換ポンプ
        let pump = tokio::spawn(event_pump(player.clone(), ev_rx));

        // ファイルロード（レジューム位置つき）。loadfile の第 3 引数は mpv のオプション表。
        // options の値は文字列のみ受ける mpv（0.34 系など）があるため文字列で渡す。
        let url = format!("https://www.youtube.com/watch?v={}", player.video_id());
        if let Err(e) = player
            .ipc
            .command(vec![
                json!("loadfile"),
                json!(url),
                json!("replace"),
                json!({ "start": format!("{}", opts.start_sec) }),
            ])
            .await
        {
            // この時点ではまだマネージャ未登録なので、ここで mpv を確実に止める。
            pump.abort();
            player.shutdown().await;
            return Err(MpvError::Ipc(e));
        }
        Ok((player, pump))
    }

    pub fn instance_id(&self) -> u32 {
        self.instance_id
    }

    pub fn video_id(&self) -> String {
        self.video_id.clone()
    }

    /// 現在状態のスナップショット。
    pub fn snapshot(&self) -> PlayerState {
        lock(&self.state).clone()
    }

    /// 終了通知を購読する。
    pub fn subscribe_ended(&self) -> broadcast::Receiver<String> {
        self.ended_tx.subscribe()
    }

    /// 終端理由が eof（最後まで再生）なら true。close 時の保存判定に使う。
    fn terminal_completed(&self) -> bool {
        self.terminal.completed()
    }

    /// バックグラウンドで取得した SponsorBlock 区間を差し込む（設計書 §4.4）。
    pub fn set_sponsor_segments(&self, segments: Vec<crate::sponsor::ActiveSegment>) {
        lock(&self.sponsor).segments = segments;
    }

    /// `player_control` の操作を mpv コマンドへ変換して送る。
    pub async fn control(&self, action: &PlayerAction) -> Result<(), MpvError> {
        match action {
            PlayerAction::Pause { value } => {
                self.ipc
                    .command(vec![json!("set_property"), json!("pause"), json!(value)])
                    .await?;
            }
            PlayerAction::Seek { seconds } => {
                self.ipc
                    .command(vec![json!("seek"), json!(seconds), json!("absolute")])
                    .await?;
            }
            PlayerAction::Volume { value } => {
                let v = value.clamp(0.0, 130.0);
                self.ipc
                    .command(vec![json!("set_property"), json!("volume"), json!(v)])
                    .await?;
            }
            PlayerAction::Speed { value } => {
                let v = value.clamp(0.1, 16.0);
                self.ipc
                    .command(vec![json!("set_property"), json!("speed"), json!(v)])
                    .await?;
            }
            PlayerAction::Quality { format } => {
                // 画質式を差し替え、現在位置を保持してリロードする（設計書 §4.3）
                self.ipc
                    .command(vec![
                        json!("set_property"),
                        json!("ytdl-format"),
                        json!(format),
                    ])
                    .await?;
                let pos = self.snapshot().position;
                let url = format!("https://www.youtube.com/watch?v={}", self.video_id());
                // replace で発生する旧ファイルの end-file は終了とみなさない
                self.terminal.begin_replace();
                if let Err(e) = self
                    .ipc
                    .command(vec![
                        json!("loadfile"),
                        json!(url),
                        json!("replace"),
                        json!({ "start": format!("{}", pos) }),
                    ])
                    .await
                {
                    self.terminal.cancel_replace();
                    return Err(MpvError::Ipc(e));
                }
            }
            PlayerAction::FrameStep => {
                self.ipc.command(vec![json!("frame-step")]).await?;
            }
            PlayerAction::FrameBackStep => {
                self.ipc.command(vec![json!("frame-back-step")]).await?;
            }
        }
        Ok(())
    }

    /// mpv を終了させる。quit を送り、猶予後に生きていれば kill する。
    pub async fn shutdown(&self) {
        let _ = self.ipc.command(vec![json!("quit")]).await;
        let mut child = self.child.lock().await;
        tokio::select! {
            _ = child.wait() => {}
            _ = tokio::time::sleep(QUIT_GRACE) => {
                let _ = child.kill().await;
            }
        }
        drop(child);
        let _ = tokio::fs::remove_file(&self.socket_path).await;
    }

    fn apply_property(&self, name: &str, data: serde_json::Value) {
        let mut st = lock(&self.state);
        match name {
            "time-pos" => st.position = data.as_f64().unwrap_or(0.0),
            "duration" => st.duration = data.as_f64().unwrap_or(0.0),
            "media-title" => st.media_title = data.as_str().unwrap_or_default().to_string(),
            "volume" => st.volume = data.as_f64().unwrap_or(st.volume),
            "speed" => st.speed = data.as_f64().unwrap_or(st.speed),
            "paused-for-cache" => {
                if data.as_bool().unwrap_or(false) {
                    st.state = PlayStatus::Buffering;
                } else if st.state == PlayStatus::Buffering {
                    st.state = if st.pause {
                        PlayStatus::Paused
                    } else {
                        PlayStatus::Playing
                    };
                }
            }
            "video-params" => {
                st.fps = data
                    .get("fps")
                    .and_then(serde_json::Value::as_f64)
                    .unwrap_or(st.fps);
            }
            "eof-reached" => {
                if data.as_bool().unwrap_or(false) {
                    st.state = PlayStatus::Ended;
                }
            }
            "pause" => {
                st.pause = data.as_bool().unwrap_or(false);
                // Ended / Buffering は上書きしない
                if st.state != PlayStatus::Ended && st.state != PlayStatus::Buffering {
                    st.state = if st.pause {
                        PlayStatus::Paused
                    } else if st.duration > 0.0 {
                        PlayStatus::Playing
                    } else {
                        PlayStatus::Idle
                    };
                }
            }
            _ => {}
        }
    }
}

/// IPC イベントを状態スナップショットと終了通知へ変換するループ。
async fn event_pump(player: Arc<MpvPlayer>, mut rx: mpsc::Receiver<IpcEvent>) {
    while let Some(ev) = rx.recv().await {
        match ev {
            IpcEvent::PropertyChange { name, data, .. } => {
                // --keep-open=yes では EOF で end-file が発生せず mpv が pause で残るため、
                // observe 済みの eof-reached を終端（reason=eof）として扱う
                if name == "eof-reached"
                    && data.as_bool().unwrap_or(false)
                    && player.terminal.on_eof()
                {
                    let _ = player.ended_tx.send("eof".to_string());
                }
                let pos = if name == "time-pos" {
                    data.as_f64()
                } else {
                    None
                };
                player.apply_property(&name, data);
                if let Some(pos) = pos {
                    sponsor_check(&player, pos).await;
                }
            }
            IpcEvent::Event { name, data } => match name.as_str() {
                "end-file" => {
                    let reason = data
                        .get("reason")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("unknown")
                        .to_string();
                    // replace（画質変更）由来の end-file は終了イベントとしない
                    if !player.terminal.on_end_file(&reason) {
                        continue;
                    }
                    {
                        let mut st = lock(&player.state);
                        st.state = PlayStatus::Ended;
                    }
                    let _ = player.ended_tx.send(reason);
                }
                "file-loaded" => {
                    // 新ファイルのロード完了。replace 予約の取りこぼしを掃除する
                    player.terminal.on_file_loaded();
                }
                _ => {}
            },
            IpcEvent::Disconnected => {
                if !player.terminal.on_disconnect() {
                    continue;
                }
                let mut st = lock(&player.state);
                st.state = PlayStatus::Ended;
                let _ = player.ended_tx.send("process_exit".to_string());
            }
        }
    }
}

/// time-pos が SponsorBlock 区間に入ったときの処理（設計書 §4.4）。
/// Skip は区間末尾へ seek して `sponsor://skipped`、Notify は同イベントを通知だけ送る。
/// 各区間は再生ごとに 1 回だけ発火する（戻って再侵入しても再発火しない）。
async fn sponsor_check(player: &Arc<MpvPlayer>, pos: f64) {
    let seg = {
        let mut sp = lock(&player.sponsor);
        match crate::sponsor::hit_index(&sp.segments, pos) {
            Some(i) if !sp.fired.contains(&i) => {
                sp.fired.insert(i);
                Some(sp.segments[i].clone())
            }
            _ => None,
        }
    };
    let Some(seg) = seg else { return };
    if seg.action == crate::sponsor::CategoryAction::Skip {
        if let Err(e) = player
            .control(&PlayerAction::Seek { seconds: seg.end })
            .await
        {
            tracing::warn!(error = %e, "SponsorBlock スキップの seek に失敗");
        }
    }
    let payload = crate::sponsor::SkippedPayload {
        instance_id: player.instance_id(),
        video_id: player.video_id(),
        category: seg.category,
        segment: [seg.start, seg.end],
        action: if seg.action == crate::sponsor::CategoryAction::Skip {
            "skip"
        } else {
            "notify"
        },
    };
    if let Err(e) = player.app.emit("sponsor://skipped", payload) {
        tracing::warn!(error = %e, "sponsor://skipped の送出に失敗");
    }
}

/// mpv の IPC ソケット出現を待つ。プロセスが早期終了した場合はタイムアウトではなく
/// 起動失敗として扱えるよう、子プロセスの終了も併せて監視する。
async fn wait_for_socket(
    socket_path: &Path,
    child: &mut tokio::process::Child,
) -> Result<(), MpvError> {
    let deadline = Instant::now() + SOCKET_WAIT_TIMEOUT;
    loop {
        if socket_path.exists() {
            return Ok(());
        }
        if let Some(status) = child.try_wait().ok().flatten() {
            return Err(MpvError::Spawn(std::io::Error::other(format!(
                "mpv がソケット作成前に終了しました: {status}"
            ))));
        }
        if Instant::now() > deadline {
            return Err(MpvError::SocketTimeout);
        }
        tokio::time::sleep(SOCKET_POLL).await;
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// 起動途中で失敗した mpv を掃除する。kill で確実に回収し、ソケット残骸を消す。
async fn cleanup_failed_spawn(child: &mut tokio::process::Child, socket_path: &Path) {
    let _ = child.kill().await;
    let _ = tokio::fs::remove_file(socket_path).await;
}

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
    pub async fn play(
        &self,
        video_id: &str,
        start_sec: f64,
        ytdl_format: Option<String>,
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
        let opts = SpawnOptions {
            video_id: video_id.to_string(),
            start_sec,
            ytdl_format: ytdl_format.unwrap_or_else(|| DEFAULT_YTDL_FORMAT.to_string()),
            ytdlp_path,
            wheel_script: self.wheel_script.clone(),
            wheel_volume_delta,
        };
        let (player, pump) = MpvPlayer::spawn(id, &self.socket_dir, opts, self.app.clone()).await?;
        let emitter = self.spawn_emitter(player.clone());

        // SponsorBlock の区間取得をバックグラウンドで行い、判定用にプレイヤーへ差し込む。
        // 取得失敗は再生を阻害しない（スキップが動かないだけ）。
        {
            let player = player.clone();
            let http = self.http.clone();
            let video_id = video_id.to_string();
            let categories = self
                .db
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

        lock(&self.players).insert(
            id,
            PlayerEntry {
                player,
                pump,
                emitter,
            },
        );
        // 再生開始時点で履歴行を確保（タイトルは media-title 変化で追従）
        if let Err(e) = self.db.history_upsert(video_id) {
            tracing::warn!(video_id, error = %e, "履歴行の作成に失敗");
        }
        Ok(id)
    }

    /// `player_control` の実体。
    pub async fn control(&self, instance_id: u32, action: &PlayerAction) -> Result<(), MpvError> {
        let player = {
            let players = lock(&self.players);
            players.get(&instance_id).map(|e| e.player.clone())
        };
        let player = player.ok_or(MpvError::NoSuchInstance(instance_id))?;
        player.control(action).await
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
                        let payload = PlayerEnded {
                            instance_id: player.instance_id(),
                            video_id: player.video_id(),
                            reason,
                        };
                        if let Err(e) = app.emit("player://ended", &payload) {
                            tracing::warn!(error = %e, "player://ended の送出に失敗");
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
    if let Err(e) = db.history_update_progress(
        &snap.video_id,
        &snap.media_title,
        snap.position,
        (snap.duration > 0.0).then_some(snap.duration as i64),
        completed,
    ) {
        tracing::warn!(video_id = %snap.video_id, error = %e, "履歴の保存に失敗");
    }
}

#[cfg(test)]
mod tests {
    use super::TerminalTracker;

    /// 終端判定: eof は completed、それ以外の終了は不完全のまま。
    #[test]
    fn terminal_completed_only_for_eof() {
        let t = TerminalTracker::default();
        assert!(!t.completed());
        assert!(t.on_end_file("eof"));
        assert!(t.completed());

        let t = TerminalTracker::default();
        assert!(t.on_end_file("error"));
        assert!(!t.completed());
    }

    /// 終端判定: 画質変更（loadfile replace）由来の end-file は終了扱いしない。
    #[test]
    fn replace_end_file_is_ignored() {
        let t = TerminalTracker::default();
        t.begin_replace();
        assert!(!t.on_end_file("stop"));
        // 予約は1回分だけ。次の真の end-file は終端として処理される
        assert!(t.on_end_file("eof"));
        assert!(t.completed());
    }

    /// 終端判定: replace 発行失敗時の取消でカウンタが残らない。
    #[test]
    fn cancel_replace_keeps_terminal_detection() {
        let t = TerminalTracker::default();
        t.begin_replace();
        t.cancel_replace();
        assert!(t.on_end_file("eof"));
    }

    /// 終端判定: 二重の終端イベントと切断通知は一度だけ受理する。
    #[test]
    fn terminal_is_recorded_once() {
        let t = TerminalTracker::default();
        assert!(t.on_end_file("eof"));
        assert!(!t.on_end_file("stop"));
        assert!(!t.on_disconnect());

        let t = TerminalTracker::default();
        assert!(t.on_disconnect());
        assert!(!t.on_end_file("eof"));
        assert!(!t.completed());
    }

    /// 終端判定: file-loaded で取りこぼした replace 予約を掃除する。
    #[test]
    fn file_loaded_clears_pending_replaces() {
        let t = TerminalTracker::default();
        t.begin_replace();
        t.on_file_loaded();
        // 予約が残っていないので次の end-file は終端になる
        assert!(t.on_end_file("eof"));
    }

    /// 終端判定: replace 予約中に届いた旧ファイルの eof-reached は
    /// 予約を消費しない（続く replace 由来の end-file が終端になる競合の防止）。
    /// begin_replace → 旧 eof-reached → 旧 end-file(stop) → file-loaded の順を検証する。
    #[test]
    fn stale_eof_during_replace_does_not_consume_reservation() {
        let t = TerminalTracker::default();
        t.begin_replace();
        // 旧ファイルの EOF 通知がキュー残りで到着しても予約を消化しない
        assert!(!t.on_eof());
        // replace 由来の end-file は引き続き予約を消費して無視される
        assert!(!t.on_end_file("stop"));
        t.on_file_loaded();
        // 新ファイルが同じ末尾位置から再度 EOF になれば終端として受理される
        assert!(t.on_eof());
        assert!(t.completed());
    }
}
