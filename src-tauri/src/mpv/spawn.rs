//! mpv 起動・IPC 接続確立の下請け。エンドポイント解決・ソケット待機・
//! 失敗時の掃除・`loadfile ... replace` 送信をまとめる。
use super::{IpcClient, IpcError, MpvError};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

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
    let _ = tokio::fs::remove_file(socket_path).await;
}

#[cfg(test)]
mod tests {
    /// IPC 疑似サーバが UnixListener 前提のため Unix のみ。
    #[cfg(unix)]
    mod ipc_tests {
        use crate::mpv::spawn::{ipc_endpoint, loadfile_replace};
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
    }
}
