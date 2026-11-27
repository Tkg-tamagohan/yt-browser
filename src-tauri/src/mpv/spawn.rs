//! mpv 起動・IPC 接続確立の下請け。エンドポイント解決・ソケット待機・
//! 失敗時の掃除・`loadfile ... replace` 送信をまとめる。
use super::{IpcClient, IpcError, IpcEvent, MpvError};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

/// mpv 子プロセスを起動し、IPC ソケットの出現待ち・接続・プロパティ監視まで行う。
/// 初回 spawn と自動再起動（仕様決定 BE）の両方から使う共通経路で、
/// 失敗時は子プロセスを kill してソケット残骸を掃除してから返す。
/// `observed` は `observe_property` に渡す (id, プロパティ名) の並び。
pub(crate) async fn launch(
    socket_path: &Path,
    args: &[String],
    observed: &[(u64, &str)],
) -> Result<(tokio::process::Child, IpcClient, mpsc::Receiver<IpcEvent>), MpvError> {
    let mut mpv_cmd = tokio::process::Command::new("mpv");
    mpv_cmd.args(args).kill_on_drop(true);
    #[cfg(windows)]
    mpv_cmd.creation_flags(crate::util::CREATE_NO_WINDOW);
    let mut child = mpv_cmd.spawn().map_err(MpvError::Spawn)?;

    if let Err(e) = wait_for_socket(socket_path, &mut child).await {
        cleanup_failed_spawn(&mut child, socket_path).await;
        return Err(e);
    }

    let (ev_tx, ev_rx) = mpsc::channel(256);
    let ipc = match IpcClient::connect(socket_path, ev_tx).await {
        Ok(ipc) => ipc,
        Err(e) => {
            cleanup_failed_spawn(&mut child, socket_path).await;
            return Err(e.into());
        }
    };

    for (id, name) in observed {
        if let Err(e) = ipc
            .command(vec![json!("observe_property"), json!(id), json!(name)])
            .await
        {
            cleanup_failed_spawn(&mut child, socket_path).await;
            return Err(MpvError::Ipc(e));
        }
    }
    configure_ytdl_logging(&ipc).await;
    Ok((child, ipc, ev_rx))
}

/// 再生開始失敗の原因診断（FR-95、仕様決定 EA）向けに、mpv の IPC 接続直後・
/// `loadfile` 前にログ購読と `ytdl-raw-options` への `verbose=` 追記を送る。
/// 起動引数を変えずに既存の PO Token 伝達経路を維持しつつ、失敗時の警告行とエラー行を
/// 受け取れるようにする。いずれかのコマンドが失敗しても警告ログのみ記録して続行する
/// （古い mpv や非対応環境でも起動自体は止めない退避動作）。
pub(crate) async fn configure_ytdl_logging(ipc: &IpcClient) {
    if let Err(e) = ipc
        .command(vec![json!("request_log_messages"), json!("error")])
        .await
    {
        tracing::warn!(
            error = %e,
            "mpv request_log_messages の設定に失敗（診断ログなしで続行）"
        );
    }
    if let Err(e) = ipc
        .command(vec![
            json!("change-list"),
            json!("ytdl-raw-options"),
            json!("append"),
            json!("verbose="),
        ])
        .await
    {
        tracing::warn!(
            error = %e,
            "mpv ytdl-raw-options への verbose 追加に失敗（詳細警告なしで続行）"
        );
    }
}

/// mpv プロセスが IPC ソケットを作るまでの待ち時間。
const SOCKET_WAIT_TIMEOUT: Duration = Duration::from_secs(5);
/// ソケット出現のポーリング間隔。
const SOCKET_POLL: Duration = Duration::from_millis(50);

/// mpv に `loadfile <url> replace <options>` を送る。
/// mpv 0.38 で `index` 引数が options の前に挿入された（`loadfile url flags index
/// options`）ため、先に 4 引数形を試し、`invalid parameter` なら旧 3 引数形へ
/// フォールバックする。`mpv-version` 文字列はディストリ由来のハッシュ表示に
/// なることがあるため、バージョン解析ではなく応答エラーで分岐する。
pub(crate) async fn loadfile_replace(
    ipc: &IpcClient,
    url: &str,
    options: serde_json::Value,
) -> Result<(), IpcError> {
    let res = ipc
        .command(vec![
            json!("loadfile"),
            json!(url),
            json!("replace"),
            json!(-1),
            options.clone(),
        ])
        .await;
    match res {
        Err(IpcError::Mpv(msg)) if msg == "invalid parameter" => ipc
            .command(vec![
                json!("loadfile"),
                json!(url),
                json!("replace"),
                options,
            ])
            .await
            .map(|_| ()),
        other => other.map(|_| ()),
    }
}

/// mpv IPC エンドポイントのパス。Linux は Unix ドメインソケット、
/// Windows は名前付きパイプ（`--input-ipc-server` が OS ごとに解釈する）。
#[cfg(unix)]
pub(crate) fn ipc_endpoint(socket_dir: &Path, instance_id: u32) -> PathBuf {
    socket_dir.join(format!("mpv-{instance_id}.sock"))
}

/// Windows の名前付きパイプはファイルシステムに実体を持たず `\\.\pipe\` 仮想
/// 名前空間に置かれる。アプリ多重起動でパイプ名が衝突しないよう PID を混ぜる。
#[cfg(windows)]
pub(crate) fn ipc_endpoint(_socket_dir: &Path, instance_id: u32) -> PathBuf {
    PathBuf::from(format!(
        r"\\.\pipe\yt-browser-mpv-{instance_id}-{}",
        std::process::id()
    ))
}

/// mpv の IPC ソケット出現を待つ。プロセスが早期終了した場合はタイムアウトではなく
/// 起動失敗として扱えるよう、子プロセスの終了も併せて監視する。
#[cfg(unix)]
pub(crate) async fn wait_for_socket(
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

/// Windows 版の待機。名前付きパイプは `Path::exists` で確実に検出できないため、
/// 実際に接続を試みて成否で判断する（成功分は即 drop。mpv は複数クライアントを
/// 受け付けるので実接続の妨げにならない）。`ERROR_PIPE_BUSY` (231) は
/// パイプが存在するが全インスタンス使用中＝待機成功とみなす。
#[cfg(windows)]
pub(crate) async fn wait_for_socket(
    socket_path: &Path,
    child: &mut tokio::process::Child,
) -> Result<(), MpvError> {
    use tokio::net::windows::named_pipe::ClientOptions;
    const ERROR_PIPE_BUSY: i32 = 231;

    let deadline = Instant::now() + SOCKET_WAIT_TIMEOUT;
    loop {
        match ClientOptions::new().open(socket_path) {
            Ok(_) => return Ok(()),
            Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY) => return Ok(()),
            Err(_) => {}
        }
        if let Some(status) = child.try_wait().ok().flatten() {
            return Err(MpvError::Spawn(std::io::Error::other(format!(
                "mpv がパイプ作成前に終了しました: {status}"
            ))));
        }
        if Instant::now() > deadline {
            return Err(MpvError::SocketTimeout);
        }
        tokio::time::sleep(SOCKET_POLL).await;
    }
}

/// 起動途中で失敗した mpv を掃除する。kill で確実に回収し、ソケット残骸を消す。
pub(crate) async fn cleanup_failed_spawn(child: &mut tokio::process::Child, socket_path: &Path) {
    let _ = child.kill().await;
    let _ = child.wait().await;
    let _ = tokio::fs::remove_file(socket_path).await;
}

#[cfg(test)]
mod tests {
    /// IPC 疑似サーバが UnixListener 前提のため Unix のみ。
    #[cfg(unix)]
    mod ipc_tests {
        use crate::mpv::spawn::{configure_ytdl_logging, ipc_endpoint, loadfile_replace};
        use crate::mpv::{IpcClient, IpcEvent};
        use crate::util::lock;
        use serde_json::{json, Value};
        use std::path::Path;
        use std::sync::Arc;
        use std::sync::Mutex;
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        use tokio::net::UnixListener;
        use tokio::sync::mpsc;
        use tokio::task::JoinHandle;

        /// 疑似 mpv: 受信したコマンドを記録し、`replies` の応答を順に返す。
        /// replies を返し終えたら切断する。
        async fn spawn_fake_mpv(
            sock: &Path,
            replies: &'static [&'static str],
        ) -> (JoinHandle<()>, Arc<Mutex<Vec<Value>>>) {
            let received = Arc::new(Mutex::new(Vec::new()));
            let received_task = received.clone();
            let listener = UnixListener::bind(sock).unwrap();
            let handle = tokio::spawn(async move {
                let (stream, _) = listener.accept().await.unwrap();
                let (r, mut w) = stream.into_split();
                let mut lines = BufReader::new(r).lines();
                let mut count = 0usize;
                while let Ok(Some(line)) = lines.next_line().await {
                    let v: Value = serde_json::from_str(&line).unwrap();
                    let req_id = v["request_id"].as_u64().unwrap();
                    lock(&received_task).push(v["command"].clone());
                    let error = replies[count.min(replies.len() - 1)];
                    count += 1;
                    let reply = json!({"request_id": req_id, "error": error});
                    w.write_all((reply.to_string() + "\n").as_bytes())
                        .await
                        .unwrap();
                    if count >= replies.len() {
                        break;
                    }
                }
            });
            (handle, received)
        }

        async fn connect(sock: &Path) -> (IpcClient, mpsc::Receiver<IpcEvent>) {
            let (tx, rx) = mpsc::channel(8);
            (IpcClient::connect(sock, tx).await.unwrap(), rx)
        }

        /// mpv 0.38+ では 4 引数形が受理され、リトライは起きない。
        #[tokio::test]
        async fn loadfile_replace_uses_four_arg_form_when_accepted() {
            let dir = tempfile::tempdir().unwrap();
            let sock = ipc_endpoint(dir.path(), 0);
            let (server, received) = spawn_fake_mpv(&sock, &["success"]).await;
            let (client, _rx) = connect(&sock).await;
            loadfile_replace(&client, "http://example/v", json!({"start": "1.5"}))
                .await
                .unwrap();
            server.await.unwrap();
            let received = lock(&received).clone();
            assert_eq!(received.len(), 1);
            // [loadfile, url, "replace", -1(index), {options}]
            let cmd = received[0].as_array().unwrap();
            assert_eq!(cmd.len(), 5);
            assert_eq!(cmd[0], json!("loadfile"));
            assert_eq!(cmd[2], json!("replace"));
            assert_eq!(cmd[3], json!(-1));
            assert_eq!(cmd[4], json!({"start": "1.5"}));
        }

        /// invalid parameter 応答では旧 3 引数形にフォールバックする。
        #[tokio::test]
        async fn loadfile_replace_falls_back_to_legacy_args() {
            let dir = tempfile::tempdir().unwrap();
            let sock = ipc_endpoint(dir.path(), 0);
            let (server, received) = spawn_fake_mpv(&sock, &["invalid parameter", "success"]).await;
            let (client, _rx) = connect(&sock).await;
            loadfile_replace(&client, "http://example/v", json!({"start": "1.5"}))
                .await
                .unwrap();
            server.await.unwrap();
            let received = lock(&received).clone();
            assert_eq!(received.len(), 2);
            // 2 回目は [loadfile, url, "replace", {options}]（index なし）
            let retry = received[1].as_array().unwrap();
            assert_eq!(retry.len(), 4);
            assert_eq!(retry[3], json!({"start": "1.5"}));
        }

        /// DB-ED-09: `configure_ytdl_logging` が `request_log_messages` と
        /// `change-list ytdl-raw-options append verbose=` を順に送信し、
        /// 応答エラー時も後続コマンドと起動処理を止めない（仕様決定 EA）。
        #[tokio::test]
        async fn configure_ytdl_logging_sends_both_commands_and_tolerates_errors() {
            let dir = tempfile::tempdir().unwrap();
            let sock = ipc_endpoint(dir.path(), 1);
            let (server, received) =
                spawn_fake_mpv(&sock, &["property unavailable", "invalid parameter"]).await;
            let (client, _rx) = connect(&sock).await;
            configure_ytdl_logging(&client).await;
            server.await.unwrap();
            let received = lock(&received).clone();
            assert_eq!(
                received,
                vec![
                    json!(["request_log_messages", "error"]),
                    json!(["change-list", "ytdl-raw-options", "append", "verbose="]),
                ]
            );
        }
    }

    /// Windows 向け IPC 疑似サーバ（NamedPipeServer）での診断コマンド送出・退避確認。
    #[cfg(windows)]
    mod win_ipc_tests {
        use crate::mpv::spawn::configure_ytdl_logging;
        use crate::mpv::IpcClient;
        use crate::util::lock;
        use serde_json::{json, Value};
        use std::path::PathBuf;
        use std::sync::atomic::{AtomicU32, Ordering};
        use std::sync::{Arc, Mutex};
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        use tokio::net::windows::named_pipe::ServerOptions;
        use tokio::sync::mpsc;

        fn test_pipe_path() -> PathBuf {
            static SEQ: AtomicU32 = AtomicU32::new(0);
            let n = SEQ.fetch_add(1, Ordering::Relaxed);
            PathBuf::from(format!(
                r"\\.\pipe\yt-browser-spawn-test-{}-{n}",
                std::process::id()
            ))
        }

        /// DB-ED-10: Windows 名前付きパイプ上で `configure_ytdl_logging` が 2 コマンドを順に送り、
        /// 片方または両方がエラー応答でもパニック・中断せず続行する（仕様決定 EA）。
        #[tokio::test]
        async fn configure_ytdl_logging_sends_both_commands_and_tolerates_errors() {
            let pipe = test_pipe_path();
            let server = ServerOptions::new().create(&pipe).unwrap();
            let received = Arc::new(Mutex::new(Vec::new()));
            let received_task = received.clone();
            let replies = ["property unavailable", "invalid parameter"];
            let handle = tokio::spawn(async move {
                server.connect().await.unwrap();
                let (r, mut w) = tokio::io::split(server);
                let mut lines = BufReader::new(r).lines();
                let mut count = 0usize;
                while let Ok(Some(line)) = lines.next_line().await {
                    let v: Value = serde_json::from_str(&line).unwrap();
                    let req_id = v["request_id"].as_u64().unwrap();
                    lock(&received_task).push(v["command"].clone());
                    let error = replies[count.min(replies.len() - 1)];
                    count += 1;
                    let reply = json!({"request_id": req_id, "error": error});
                    w.write_all((reply.to_string() + "\n").as_bytes())
                        .await
                        .unwrap();
                    if count >= replies.len() {
                        break;
                    }
                }
            });
            let (tx, _rx) = mpsc::channel(8);
            let client = IpcClient::connect(&pipe, tx).await.unwrap();
            configure_ytdl_logging(&client).await;
            handle.await.unwrap();
            let received = lock(&received).clone();
            assert_eq!(
                received,
                vec![
                    json!(["request_log_messages", "error"]),
                    json!(["change-list", "ytdl-raw-options", "append", "verbose="]),
                ]
            );
        }
    }
}
