//! mpv プロセス 1 台分の制御。起動・プロパティ監視・再生制御・
//! SponsorBlock 判定を `MpvPlayer` にまとめる。
use super::spawn::{cleanup_failed_spawn, ipc_endpoint, loadfile_replace, wait_for_socket};
use super::terminal::TerminalTracker;
use super::{IpcClient, IpcEvent, MpvError, DEFAULT_PIP_GEOMETRY};
use crate::model::{PlayStatus, PlayerAction, PlayerState};
use crate::util::lock;
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};
use tokio::sync::{broadcast, mpsc};
use tokio::task::JoinHandle;

/// quit 送信後、mpv の自発終了を待つ猶予。
const QUIT_GRACE: Duration = Duration::from_millis(800);

/// 最大化解除→geometry 送信の間に挟む猶予。X の最大化遷移中に届く
/// リサイズ要求は WM/mpv 側でドロップされるため、遷移完了を待つ
/// （実機検証で ~600ms が動作確認された値）。
const PWM_TRANSITION_WAIT: Duration = Duration::from_millis(600);

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

/// mpv プロセス 1 台分の制御ハンドル。
pub(crate) struct MpvPlayer {
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
    /// PiP 化で解除した最大化状態。PiP 解除時に復元する。
    pip_prev_maximized: Mutex<bool>,
    /// set_pip の遷移を直列化する。並行呼び出しが state.pip のチェックを
    /// 同時に通過して保存値や mpv プロパティを競合させるのを防ぐ
    pip_op: tokio::sync::Mutex<()>,
}

/// SponsorBlock の判定状態。区間は再生開始後のバックグラウンド取得で差し込まれる。
#[derive(Default)]
pub(crate) struct SponsorState {
    pub(crate) segments: Vec<crate::sponsor::ActiveSegment>,
    /// 各区間につき 1 回だけ発火させるための消化済みインデックス。
    fired: HashSet<usize>,
    /// seek 失敗した区間の再試行までの猶予（区間 index → 次回試行可能時刻）。
    backoff: HashMap<usize, Instant>,
}

/// `PlayerManager::play` に渡す起動条件。
pub(crate) struct SpawnOptions {
    pub(crate) video_id: String,
    /// レジューム開始位置（秒）。0 なら先頭から。
    pub(crate) start_sec: f64,
    /// `--ytdl-format` に渡す画質式。
    pub(crate) ytdl_format: String,
    /// `ytdl_hook-ytdl_path` に渡す yt-dlp のパス。None なら mpv の既定解決に任せる。
    pub(crate) ytdlp_path: Option<String>,
    /// `--script` に渡す wheel.lua のパス（app_data/mpv/wheel.lua）。
    pub(crate) wheel_script: Option<PathBuf>,
    /// `wheel-volume_delta` に渡す音量変化量。None なら Lua 既定（2）。
    pub(crate) wheel_volume_delta: Option<String>,
    /// PiP（最前面・枠なしの小窓）で起動するときの `--geometry` 値。
    /// None なら通常ウィンドウで起動する（設計書 §4.5）。
    pub(crate) pip_geometry: Option<String>,
}

impl MpvPlayer {
    /// mpv を起動し、IPC ソケットに接続してプロパティ監視とファイルロードを行う。
    /// 戻り値の `JoinHandle` はイベントポンプ（IPC イベント→状態変換）で、
    /// 呼び出し側（PlayerManager）が保持して終了時に abort する。
    pub(crate) async fn spawn(
        instance_id: u32,
        socket_dir: &Path,
        opts: SpawnOptions,
        app: AppHandle,
    ) -> Result<(Arc<Self>, JoinHandle<()>), MpvError> {
        let socket_path = ipc_endpoint(socket_dir, instance_id);
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
        // PiP は起動時フラグで指定する（設計書 §4.5: ontop・枠なし・小窓配置）
        if let Some(geo) = &opts.pip_geometry {
            args.push("--ontop=yes".into());
            args.push("--border=no".into());
            args.push(format!("--geometry={geo}"));
        }
        let mut mpv_cmd = tokio::process::Command::new("mpv");
        mpv_cmd.args(&args).kill_on_drop(true);
        #[cfg(windows)]
        mpv_cmd.creation_flags(crate::CREATE_NO_WINDOW);
        let mut child = mpv_cmd.spawn().map_err(MpvError::Spawn)?;

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
                pip: opts.pip_geometry.is_some(),
            }),
            ended_tx,
            terminal: TerminalTracker::default(),
            app,
            sponsor: Mutex::new(SponsorState::default()),
            pip_prev_maximized: Mutex::new(false),
            pip_op: tokio::sync::Mutex::new(()),
        });

        // IPC イベント → 状態スナップショット/終了通知への変換ポンプ
        let pump = tokio::spawn(event_pump(player.clone(), ev_rx));

        // ファイルロード（レジューム位置つき）。mpv 0.38 で loadfile の引数形式が
        // 変わったため loadfile_replace が新旧両形式に対応する。
        // options の値は文字列のみ受ける mpv（0.34 系など）があるため文字列で渡す。
        let url = format!("https://www.youtube.com/watch?v={}", player.video_id());
        if let Err(e) = loadfile_replace(
            &player.ipc,
            &url,
            json!({ "start": format!("{}", opts.start_sec) }),
        )
        .await
        {
            // この時点ではまだマネージャ未登録なので、ここで mpv を確実に止める。
            pump.abort();
            player.shutdown().await;
            return Err(MpvError::Ipc(e));
        }
        Ok((player, pump))
    }

    pub(crate) fn instance_id(&self) -> u32 {
        self.instance_id
    }

    pub(crate) fn video_id(&self) -> String {
        self.video_id.clone()
    }

    /// 現在状態のスナップショット。
    pub(crate) fn snapshot(&self) -> PlayerState {
        lock(&self.state).clone()
    }

    /// 終了通知を購読する。
    pub(crate) fn subscribe_ended(&self) -> broadcast::Receiver<String> {
        self.ended_tx.subscribe()
    }

    /// 終端理由が eof（最後まで再生）なら true。close 時の保存判定に使う。
    pub(crate) fn terminal_completed(&self) -> bool {
        self.terminal.completed()
    }

    /// バックグラウンドで取得した SponsorBlock 区間を差し込む（設計書 §4.4）。
    pub(crate) fn set_sponsor_segments(&self, segments: Vec<crate::sponsor::ActiveSegment>) {
        lock(&self.sponsor).segments = segments;
    }

    /// `player_control` の操作を mpv コマンドへ変換して送る。
    pub(crate) async fn control(&self, action: &PlayerAction) -> Result<(), MpvError> {
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
                if let Err(e) =
                    loadfile_replace(&self.ipc, &url, json!({ "start": format!("{}", pos) })).await
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
            PlayerAction::Pip { enabled } => {
                // 設定値（pip.geometry）の解決は PlayerManager::control で行う。
                // ここに直接届いた場合は既定値で切り替える。
                self.set_pip(*enabled, DEFAULT_PIP_GEOMETRY).await?;
            }
        }
        Ok(())
    }

    /// PiP 表示の切り替え（設計書 §4.5）。ontop・枠なし・小窓配置をまとめて適用し、
    /// 解除時は geometry を空に戻す（mpv は空文字で既定配置に戻す）。
    /// いずれのプロパティも実行時に変更可能（mpv 0.34 系で確認済み）。
    ///
    /// 最大化中のウィンドウでは geometry が効かず枠なし最前面の巨大ウィンドウが
    /// デスクトップを覆うため（実機検証で確認）、PiP 化前に最大化を解除し、
    /// 解除時に復元する。
    pub(crate) async fn set_pip(&self, enabled: bool, geometry: &str) -> Result<(), MpvError> {
        // 遷移全体を直列化する。並行する set_pip が state.pip チェックを
        // 同時に通過して pip_prev_maximized を上書きしたり、mpv への
        // プロパティ送信を交互させたりするのを防ぐ
        let _op = self.pip_op.lock().await;
        if enabled {
            // 最大化の記録は false→true の遷移時だけ行う。
            // PiP 中の再適用（enabled=true の重複送信）で保存値を上書きしない。
            // 解除自体は毎回読んで適用する（PiP 中の外部操作で再最大化された
            // 場合も正しく解除できるように）
            let maximized = self
                .ipc
                .command(vec![json!("get_property"), json!("window-maximized")])
                .await
                .ok()
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if !lock(&self.state).pip {
                *lock(&self.pip_prev_maximized) = maximized;
            }
            if maximized {
                self.ipc
                    .command(vec![
                        json!("set_property"),
                        json!("window-maximized"),
                        json!(false),
                    ])
                    .await?;
                // 最大化解除は非同期の X 遷移で、遷移中に届く geometry の
                // リサイズ要求はドロップされる（実機検証で確認）。
                // 遷移完了を待つ実機確認済みの猶予を挟んでから後続を送る
                tokio::time::sleep(PWM_TRANSITION_WAIT).await;
            }
        }
        self.ipc
            .command(vec![json!("set_property"), json!("ontop"), json!(enabled)])
            .await?;
        self.ipc
            .command(vec![
                json!("set_property"),
                json!("border"),
                json!(!enabled),
            ])
            .await?;
        self.ipc
            .command(vec![
                json!("set_property"),
                json!("geometry"),
                json!(if enabled { geometry } else { "" }),
            ])
            .await?;
        if !enabled {
            // PiP 化で解除した最大化を復元する（復元失敗は解除自体を失敗にしない）
            let restore = std::mem::take(&mut *lock(&self.pip_prev_maximized));
            if restore {
                let _ = self
                    .ipc
                    .command(vec![
                        json!("set_property"),
                        json!("window-maximized"),
                        json!(true),
                    ])
                    .await;
            }
        }
        lock(&self.state).pip = enabled;
        Ok(())
    }

    /// mpv を終了させる。quit を送り、猶予後に生きていれば kill する。
    pub(crate) async fn shutdown(&self) {
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

/// スキップの seek に失敗した区間を再試行するまでの間隔。
const SPONSOR_RETRY_BACKOFF: Duration = Duration::from_secs(2);

/// time-pos が SponsorBlock 区間に入ったときの処理（設計書 §4.4）。
/// Skip は区間末尾へ seek して `sponsor://skipped`、Notify は同イベントを通知だけ送る。
/// 各区間は再生ごとに 1 回だけ発火する（戻って再侵入しても再発火しない）。
/// 発火済み区間とバックオフ中の区間は `next_candidate` が除外するので、
/// 区間が重なっていても未発火の候補に到達できる。
async fn sponsor_check(player: &Arc<MpvPlayer>, pos: f64) {
    let picked = {
        let mut sp = lock(&player.sponsor);
        crate::sponsor::next_candidate(&sp.segments, &sp.fired, &sp.backoff, pos).map(|i| {
            // Notify は IPC を伴わないので、この時点で消化済みにしてよい
            if sp.segments[i].action == crate::sponsor::CategoryAction::Notify {
                sp.fired.insert(i);
            }
            (i, sp.segments[i].clone())
        })
    };
    let Some((i, seg)) = picked else { return };
    if seg.action == crate::sponsor::CategoryAction::Skip {
        match player
            .control(&PlayerAction::Seek { seconds: seg.end })
            .await
        {
            Ok(()) => {
                lock(&player.sponsor).fired.insert(i);
            }
            Err(e) => {
                // 失敗時は発火済みにせず通知も出さない。短いバックオフ後に再試行する
                tracing::warn!(error = %e, "SponsorBlock スキップの seek に失敗");
                lock(&player.sponsor)
                    .backoff
                    .insert(i, Instant::now() + SPONSOR_RETRY_BACKOFF);
                return;
            }
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
