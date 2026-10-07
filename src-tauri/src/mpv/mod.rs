//! mpv プロセス管理と JSON IPC クライアント（設計書 §4.1）。
//! 1 再生 = 1 mpv プロセス + 1 IPC ソケット。マルチビューはインスタンスを増やすだけで済む
//! （仕様決定 B / FR-1）。再生制御はすべてこのモジュールを経由する。

mod ipc;

use std::collections::HashMap;
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
}

impl MpvPlayer {
    /// mpv を起動し、IPC ソケットに接続してプロパティ監視とファイルロードを行う。
    /// 戻り値の `JoinHandle` はイベントポンプ（IPC イベント→状態変換）で、
    /// 呼び出し側（PlayerManager）が保持して終了時に abort する。
    pub async fn spawn(
        instance_id: u32,
        socket_dir: &Path,
        opts: SpawnOptions,
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
        if let Some(path) = &opts.ytdlp_path {
            // 同梱 / システム混在環境でどちらを使うか確定させる（設計書 §4.1）
            args.push(format!("--script-opts=ytdl_hook-ytdl_path={path}"));
        }
        let mut child = tokio::process::Command::new("mpv")
            .args(&args)
            .kill_on_drop(true)
            .spawn()
            .map_err(MpvError::Spawn)?;

        wait_for_socket(&socket_path, &mut child).await?;

        let (ev_tx, ev_rx) = mpsc::channel(64);
        let ipc = IpcClient::connect(&socket_path, ev_tx).await?;

        for (id, name) in OBSERVED_PROPERTIES {
            ipc.command(vec![json!("observe_property"), json!(id), json!(name)])
                .await?;
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
        });

        // IPC イベント → 状態スナップショット/終了通知への変換ポンプ
        let pump = tokio::spawn(event_pump(player.clone(), ev_rx));

        // ファイルロード（レジューム位置つき）。loadfile の第 4 引数は mpv のオプション表。
        let url = format!("https://www.youtube.com/watch?v={}", player.video_id());
        player
            .ipc
            .command(vec![
                json!("loadfile"),
                json!(url),
                json!("replace"),
                json!(0),
                json!({ "start": opts.start_sec }),
            ])
            .await?;
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
                self.ipc
                    .command(vec![
                        json!("loadfile"),
                        json!(url),
                        json!("replace"),
                        json!(0),
                        json!({ "start": pos }),
                    ])
                    .await?;
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
                player.apply_property(&name, data);
            }
            IpcEvent::Event { name, data } => {
                if name == "end-file" {
                    let reason = data
                        .get("reason")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("unknown")
                        .to_string();
                    {
                        let mut st = lock(&player.state);
                        st.state = PlayStatus::Ended;
                    }
                    let _ = player.ended_tx.send(reason);
                }
            }
            IpcEvent::Disconnected => {
                let mut st = lock(&player.state);
                if st.state != PlayStatus::Ended {
                    st.state = PlayStatus::Ended;
                    let _ = player.ended_tx.send("process_exit".to_string());
                }
            }
        }
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

/// 全 mpv インスタンスの管理（設計書 §4.5 のマルチビュー前提）。
/// `play` が発行する `instance_id` が UI 側の操作対象識別子。
pub struct PlayerManager {
    players: Mutex<HashMap<u32, PlayerEntry>>,
    next_id: AtomicU32,
    app: AppHandle,
    db: Db,
    socket_dir: PathBuf,
    /// yt-dlp パスの解決器（settings `ytdlp.path` → 同梱リソース → PATH）。
    ytdlp_resolver: crate::yt::YtDlpResolver,
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
    ) -> Self {
        Self {
            players: Mutex::new(HashMap::new()),
            next_id: AtomicU32::new(1),
            app,
            db,
            socket_dir,
            ytdlp_resolver,
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
        let opts = SpawnOptions {
            video_id: video_id.to_string(),
            start_sec,
            ytdl_format: ytdl_format.unwrap_or_else(|| DEFAULT_YTDL_FORMAT.to_string()),
            ytdlp_path,
        };
        let (player, pump) = MpvPlayer::spawn(id, &self.socket_dir, opts).await?;
        let emitter = self.spawn_emitter(player.clone());
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
    /// 既に終了済みのインスタンス（emitter 自然終了済み）でも掃除だけは行う。
    pub async fn close(&self, instance_id: u32) -> Result<(), MpvError> {
        let entry = lock(&self.players).remove(&instance_id);
        let Some(PlayerEntry {
            player,
            pump,
            emitter,
        }) = entry
        else {
            return Err(MpvError::NoSuchInstance(instance_id));
        };
        emitter.abort();
        pump.abort();
        self.persist_history(&player, false);
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
            self.persist_history(&player, false);
            player.shutdown().await;
        }
    }

    /// 状態のサンプリング送出と履歴の定期保存。
    /// 終了通知を受けたら最終保存をしてタスクを抜ける。
    fn spawn_emitter(&self, player: Arc<MpvPlayer>) -> JoinHandle<()> {
        let app = self.app.clone();
        let db = self.db.clone();
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
