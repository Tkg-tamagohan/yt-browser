//! mpv JSON IPC の送受信層（設計書 §4.1）。
//! ソケット接続、`request_id` によるコマンド/応答の対応付け、
//! 非同期イベント（property-change 等）の配送を担当する。
//!
//! 転送層は OS ごとに `imp` モジュールへ隔離する。Linux は Unix ドメイン
//! ソケット、Windows は名前付きパイプ（tokio `windows::named_pipe` で
//! 追加クレートなしに実装）。

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, oneshot, Mutex as AsyncMutex};

use crate::util::lock;

/// 転送層の OS 差異を閉じ込める。`connect` がストリームを確立して
/// 読み/書きの半端に分割して返す。以降の処理は半端の型にだけ依存する。
#[cfg(unix)]
mod imp {
    use std::io;
    use std::path::Path;
    use tokio::net::UnixStream;

    pub type ReadHalf = tokio::net::unix::OwnedReadHalf;
    pub type WriteHalf = tokio::net::unix::OwnedWriteHalf;

    pub async fn connect(path: &Path) -> io::Result<(ReadHalf, WriteHalf)> {
        Ok(UnixStream::connect(path).await?.into_split())
    }
}

#[cfg(windows)]
mod imp {
    use std::io;
    use std::path::Path;
    use std::time::Duration;
    use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeClient};

    // NamedPipeClient には Owned 型の into_split がないため
    // tokio::io::split のジェネリック半端を使う。
    pub type ReadHalf = tokio::io::ReadHalf<NamedPipeClient>;
    pub type WriteHalf = tokio::io::WriteHalf<NamedPipeClient>;

    /// ERROR_PIPE_BUSY。全パイプインスタンスが使用中＝パイプ自体は存在する。
    const ERROR_PIPE_BUSY: i32 = 231;
    /// BUSY 解消を待つ期限。wait_for_socket が存在を確認した直後の一時的な
    /// 混雑を越えられれば十分な値。
    const CONNECT_DEADLINE: Duration = Duration::from_secs(2);
    const RETRY_INTERVAL: Duration = Duration::from_millis(50);

    pub async fn connect(path: &Path) -> io::Result<(ReadHalf, WriteHalf)> {
        // 名前付きパイプの接続自体は同期 API。
        // 直前の待機プローブや別クライアントがインスタンスを占有していると
        // ERROR_PIPE_BUSY になるため、期限付きで開き直す。BUSY 以外の失敗は即返す。
        let deadline = std::time::Instant::now() + CONNECT_DEADLINE;
        loop {
            match ClientOptions::new().open(path) {
                Ok(pipe) => return Ok(tokio::io::split(pipe)),
                Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY) => {
                    if std::time::Instant::now() >= deadline {
                        return Err(e);
                    }
                    tokio::time::sleep(RETRY_INTERVAL).await;
                }
                Err(e) => return Err(e),
            }
        }
    }
}

/// mpv コマンド応答の待ち時間。loadfile 等は即時返るため十分な値。
const COMMAND_TIMEOUT: Duration = Duration::from_secs(10);

/// request_id → 応答待ち送信箱の対応表。
type PendingMap = HashMap<u64, oneshot::Sender<Result<Value, IpcError>>>;

#[derive(Debug, Error)]
pub enum IpcError {
    #[error("ipc io: {0}")]
    Io(#[from] std::io::Error),
    #[error("mpv が応答せずソケットが閉じられた")]
    Closed,
    #[error("mpv 応答タイムアウト")]
    Timeout,
    #[error("mpv エラー: {0}")]
    Mpv(String),
}

/// mpv から非同期に上がる行。プロパティ変化とその他イベントを区別する。
#[derive(Debug)]
pub enum IpcEvent {
    /// `observe_property` に対応する変化通知。
    PropertyChange {
        #[allow(dead_code)]
        id: u64,
        name: String,
        data: Value,
    },
    /// `end-file` や `shutdown` などのイベント。
    Event { name: String, data: Value },
    /// ソケットが切断された（mpv プロセス終了を含む）。
    Disconnected,
}

pub struct IpcClient {
    /// 書き込み半端は async の await 越えに保持するため tokio Mutex（Send）。
    writer: AsyncMutex<imp::WriteHalf>,
    pending: Arc<Mutex<PendingMap>>,
    next_request_id: AtomicU64,
}

impl IpcClient {
    /// ソケットへ接続し、読み取りタスクを起動する。
    /// `events` は読み取った非同期イベントの配送先。
    pub async fn connect(path: &Path, events: mpsc::Sender<IpcEvent>) -> Result<Self, IpcError> {
        let (read_half, write_half) = imp::connect(path).await?;
        let pending: Arc<Mutex<PendingMap>> = Arc::new(Mutex::new(HashMap::new()));
        spawn_reader(read_half, events, pending.clone());
        Ok(Self {
            writer: AsyncMutex::new(write_half),
            pending,
            next_request_id: AtomicU64::new(1),
        })
    }

    /// JSON コマンドを送り、`request_id` に対応する応答を待つ。
    /// 応答の `data` フィールドを返す（無い場合は Null）。
    pub async fn command(&self, args: Vec<Value>) -> Result<Value, IpcError> {
        let request_id = self.next_request_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        lock(&self.pending).insert(request_id, tx);

        let line = json!({ "command": args, "request_id": request_id }).to_string() + "\n";
        {
            let mut writer = self.writer.lock().await;
            if let Err(e) = writer.write_all(line.as_bytes()).await {
                lock(&self.pending).remove(&request_id);
                return Err(e.into());
            }
        }
        match tokio::time::timeout(COMMAND_TIMEOUT, rx).await {
            Ok(Ok(res)) => res,
            Ok(Err(_)) => Err(IpcError::Closed),
            Err(_) => {
                lock(&self.pending).remove(&request_id);
                Err(IpcError::Timeout)
            }
        }
    }
}

fn spawn_reader(
    read: imp::ReadHalf,
    events: mpsc::Sender<IpcEvent>,
    pending: Arc<Mutex<PendingMap>>,
) {
    tokio::spawn(async move {
        let mut lines = BufReader::new(read).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            dispatch_line(&line, &events, &pending);
        }
        // 応答待ちのコマンドをすべて失敗させてから切断を通知する
        for (_, tx) in lock(&pending).drain() {
            let _ = tx.send(Err(IpcError::Closed));
        }
        let _ = events.send(IpcEvent::Disconnected).await;
    });
}

fn dispatch_line(line: &str, events: &mpsc::Sender<IpcEvent>, pending: &Arc<Mutex<PendingMap>>) {
    let Ok(v) = serde_json::from_str::<Value>(line) else {
        tracing::debug!(line, "mpv IPC: 解釈不能な行をスキップ");
        return;
    };
    // コマンド応答は request_id を持つ
    if let Some(req_id) = v.get("request_id").and_then(Value::as_u64) {
        if let Some(tx) = lock(pending).remove(&req_id) {
            let result = match v.get("error").and_then(Value::as_str) {
                Some("success") | None => Ok(v.get("data").cloned().unwrap_or(Value::Null)),
                Some(err) => Err(IpcError::Mpv(err.to_string())),
            };
            let _ = tx.send(result);
        }
        return;
    }
    let ev = match v.get("event").and_then(Value::as_str) {
        Some("property-change") => IpcEvent::PropertyChange {
            id: v.get("id").and_then(Value::as_u64).unwrap_or(0),
            name: v
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            data: v.get("data").cloned().unwrap_or(Value::Null),
        },
        Some(name) => IpcEvent::Event {
            name: name.to_string(),
            data: v.clone(),
        },
        None => return,
    };
    // try_send: 受信側が溢れていても読み取りを止めない（最新値で追いつく設計）
    let _ = events.try_send(ev);
}

/// 転送層の疑似サーバは OS ごとに別実装（Unix は UnixListener、Windows は
/// NamedPipeServer）なので、テストモジュールも cfg で分割する。
#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use tokio::io::AsyncWriteExt;
    use tokio::net::UnixListener;

    /// 疑似 mpv: コマンドを 1 つ受けて応答し、イベント行を 1 つ流す。
    async fn fake_mpv_server(sock: &Path, event_line: &'static str) -> tokio::task::JoinHandle<()> {
        let listener = UnixListener::bind(sock).unwrap();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (r, mut w) = stream.into_split();
            let mut lines = BufReader::new(r).lines();
            // コマンド 1 件を受けて応答し、イベント行を 1 つ流す
            if let Ok(Some(line)) = lines.next_line().await {
                let v: Value = serde_json::from_str(&line).unwrap();
                let req_id = v["request_id"].as_u64().unwrap();
                let cmd = v["command"][0].as_str().unwrap();
                let reply = if cmd == "get_property" {
                    json!({"request_id": req_id, "error": "success", "data": 42.0})
                } else {
                    json!({"request_id": req_id, "error": "success"})
                };
                w.write_all((reply.to_string() + "\n").as_bytes())
                    .await
                    .unwrap();
                if !event_line.is_empty() {
                    w.write_all(event_line.as_bytes()).await.unwrap();
                    w.write_all(b"\n").await.unwrap();
                }
            }
        })
    }

    #[tokio::test]
    async fn command_reply_matched_by_request_id() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("t.sock");
        let server = fake_mpv_server(&sock, "").await;
        let (tx, _rx) = mpsc::channel(8);
        let client = IpcClient::connect(&sock, tx).await.unwrap();
        let data = client
            .command(vec![json!("get_property"), json!("volume")])
            .await
            .unwrap();
        assert_eq!(data, json!(42.0));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn property_change_is_delivered_to_event_channel() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("t.sock");
        let server = fake_mpv_server(
            &sock,
            r#"{"event":"property-change","id":1,"name":"time-pos","data":12.5}"#,
        )
        .await;
        let (tx, mut rx) = mpsc::channel(8);
        let client = IpcClient::connect(&sock, tx).await.unwrap();
        client
            .command(vec![json!("set_property"), json!("pause"), json!(true)])
            .await
            .unwrap();
        match rx.recv().await.unwrap() {
            IpcEvent::PropertyChange { id, name, data } => {
                assert_eq!(id, 1);
                assert_eq!(name, "time-pos");
                assert_eq!(data, json!(12.5));
            }
            other => panic!("unexpected event: {other:?}"),
        }
        server.await.unwrap();
    }

    #[tokio::test]
    async fn disconnect_fails_pending_and_notifies() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("t.sock");
        // 接続を受けて応答せず保持するサーバ。コマンド行を 1 つ読んだ時点で「送信済み」を
        // got_tx で通知し、drop_tx を落とすとストリームも drop される
        let listener = UnixListener::bind(&sock).unwrap();
        let (drop_tx, drop_rx) = oneshot::channel::<()>();
        let (got_tx, got_rx) = oneshot::channel::<()>();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut lines = BufReader::new(stream).lines();
            if lines.next_line().await.unwrap().is_some() {
                let _ = got_tx.send(());
            }
            let _ = drop_rx.await;
            // lines 内の stream drop で切断
        });
        let (tx, mut rx) = mpsc::channel(8);
        let client = IpcClient::connect(&sock, tx).await.unwrap();
        // 応答の来ないコマンドを in-flight にしてから切断する
        let cmd = tokio::spawn(async move {
            client
                .command(vec![json!("get_property"), json!("pause")])
                .await
        });
        // サーバがコマンド行を読み取る＝送信完了を待ってから切断（固定待機は置かない）
        got_rx.await.unwrap();
        drop(drop_tx);
        server.await.unwrap();
        // in-flight コマンドは Closed で失敗する
        let result = tokio::time::timeout(Duration::from_secs(2), cmd)
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(result, Err(IpcError::Closed)));
        // 切断通知が届く
        let mut saw_disconnect = false;
        while let Ok(Some(ev)) = tokio::time::timeout(Duration::from_secs(1), rx.recv()).await {
            if matches!(ev, IpcEvent::Disconnected) {
                saw_disconnect = true;
                break;
            }
        }
        assert!(saw_disconnect);
    }
}

/// Unix 版 tests と同じ 3 ケースを NamedPipeServer で検証する。
/// パイプはファイルシステム上のパスではなく `\\.\pipe\` 名前空間の名前なので、
/// テストごとに一意の名前を生成する。
#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tokio::io::AsyncWriteExt;
    use tokio::net::windows::named_pipe::ServerOptions;

    /// 並行実行されるテスト間でパイプ名が衝突しないよう連番で一意化する。
    static PIPE_SEQ: AtomicU64 = AtomicU64::new(0);

    fn pipe_path() -> PathBuf {
        let seq = PIPE_SEQ.fetch_add(1, Ordering::Relaxed);
        PathBuf::from(format!(
            r"\\.\pipe\yt-browser-ipc-test-{}-{}",
            std::process::id(),
            seq
        ))
    }

    /// 疑似 mpv: コマンドを 1 つ受けて応答し、イベント行を 1 つ流す。
    /// ServerOptions::create は同期 API なので spawn 前に作成済みにしておく
    ///（存在しないパイプへの open は BUSY ではなく即時エラーになる）。
    async fn fake_mpv_server(pipe: &Path, event_line: &'static str) -> tokio::task::JoinHandle<()> {
        let server = ServerOptions::new().create(pipe).unwrap();
        tokio::spawn(async move {
            server.connect().await.unwrap();
            let (r, mut w) = tokio::io::split(server);
            let mut lines = BufReader::new(r).lines();
            if let Ok(Some(line)) = lines.next_line().await {
                let v: Value = serde_json::from_str(&line).unwrap();
                let req_id = v["request_id"].as_u64().unwrap();
                let cmd = v["command"][0].as_str().unwrap();
                let reply = if cmd == "get_property" {
                    json!({"request_id": req_id, "error": "success", "data": 42.0})
                } else {
                    json!({"request_id": req_id, "error": "success"})
                };
                w.write_all((reply.to_string() + "\n").as_bytes())
                    .await
                    .unwrap();
                if !event_line.is_empty() {
                    w.write_all(event_line.as_bytes()).await.unwrap();
                    w.write_all(b"\n").await.unwrap();
                }
            }
        })
    }

    #[tokio::test]
    async fn command_reply_matched_by_request_id() {
        let pipe = pipe_path();
        let server = fake_mpv_server(&pipe, "").await;
        let (tx, _rx) = mpsc::channel(8);
        let client = IpcClient::connect(&pipe, tx).await.unwrap();
        let data = client
            .command(vec![json!("get_property"), json!("volume")])
            .await
            .unwrap();
        assert_eq!(data, json!(42.0));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn property_change_is_delivered_to_event_channel() {
        let pipe = pipe_path();
        let server = fake_mpv_server(
            &pipe,
            r#"{"event":"property-change","id":1,"name":"time-pos","data":12.5}"#,
        )
        .await;
        let (tx, mut rx) = mpsc::channel(8);
        let client = IpcClient::connect(&pipe, tx).await.unwrap();
        client
            .command(vec![json!("set_property"), json!("pause"), json!(true)])
            .await
            .unwrap();
        match rx.recv().await.unwrap() {
            IpcEvent::PropertyChange { id, name, data } => {
                assert_eq!(id, 1);
                assert_eq!(name, "time-pos");
                assert_eq!(data, json!(12.5));
            }
            other => panic!("unexpected event: {other:?}"),
        }
        server.await.unwrap();
    }

    #[tokio::test]
    async fn disconnect_fails_pending_and_notifies() {
        let pipe = pipe_path();
        // 接続を受けて応答せず保持するサーバ。コマンド行を 1 つ読んだ時点で「送信済み」を
        // got_tx で通知し、drop_tx を落とすとサーバも drop される
        let server = ServerOptions::new().create(&pipe).unwrap();
        let (drop_tx, drop_rx) = oneshot::channel::<()>();
        let (got_tx, got_rx) = oneshot::channel::<()>();
        let server_task = tokio::spawn(async move {
            server.connect().await.unwrap();
            let mut lines = BufReader::new(server).lines();
            if lines.next_line().await.unwrap().is_some() {
                let _ = got_tx.send(());
            }
            let _ = drop_rx.await;
            // lines 内の server drop で切断
        });
        let (tx, mut rx) = mpsc::channel(8);
        let client = IpcClient::connect(&pipe, tx).await.unwrap();
        // 応答の来ないコマンドを in-flight にしてから切断する
        let cmd = tokio::spawn(async move {
            client
                .command(vec![json!("get_property"), json!("pause")])
                .await
        });
        // サーバがコマンド行を読み取る＝送信完了を待ってから切断（固定待機は置かない）
        got_rx.await.unwrap();
        drop(drop_tx);
        server_task.await.unwrap();
        // in-flight コマンドは Closed で失敗する
        let result = tokio::time::timeout(Duration::from_secs(2), cmd)
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(result, Err(IpcError::Closed)));
        // 切断通知が届く
        let mut saw_disconnect = false;
        while let Ok(Some(ev)) = tokio::time::timeout(Duration::from_secs(1), rx.recv()).await {
            if matches!(ev, IpcEvent::Disconnected) {
                saw_disconnect = true;
                break;
            }
        }
        assert!(saw_disconnect);
    }
}
