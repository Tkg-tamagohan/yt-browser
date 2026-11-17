//! Chrome 拡張の Native Messaging ホスト（FR-17、仕様決定 AH、設計書 §3.4.1）。
//!
//! 本体バイナリの別モードとして動く。Chrome はホスト起動時に引数
//! `chrome-extension://<ID>/` を渡すので、`main.rs` がそれを検出したら
//! Tauri を組まずに [`run_host`] へ分岐する。
//!
//! - 通信：4 バイトのネイティブエンディアン長と UTF-8 JSON の組を 1 往復だけ行う。
//!   要求は `{"url": "<対象 URL>", "action": "open" | "queue" | "playlist_add",
//!   "playlist_id": <数値>}`（`action` 省略は `open`、仕様決定 AX。
//!   `playlist_id` は `playlist_add` で必須、FR-50、仕様決定 CA）と、
//!   URL を取らない `{"action": "list_playlists"}`（拡張側の選択肢表示用）。
//!   応答は `{"ok": true}` か `{"ok": false, "error": "<コード>"}` で、
//!   `list_playlists` だけは `{"ok": true, "playlists": [{"id","name"}]}` を返す
//! - 起動：まず起動中の本体が待つローカル IPC（Windows は名前付きパイプ
//!   `\\.\pipe\io.github.tkg-tamagohan.yt-browser-nm-ipc`、Linux は
//!   `<app_data_dir>/native-ipc.sock`）へ `yt-browser://<action>?url=<encoded>`
//!   を直接送る。未起動等で失敗したときは本体バイナリを直接起動し、
//!   それも失敗したときだけ OS の既定ハンドラ（`open::that_detached`）へ
//!   フォールバックする
//! - `list_playlists`：本体の DB を読み取り専用で開き、`playlists` 表を
//!   `playlist_list` と同じ順（`ORDER BY sort_order, id`）で読む。
//!   DB への書き込み経路は持たない（仕様決定 AX の方針を維持）
//! - 登録：本体の setup で [`register`] を呼び、ブラウザ別の置き場へ
//!   ホスト定義を冪等に書く（FR-88、仕様決定 DM）
//!
//! 標準出力はプロトコル専用なので、ホストモードではログを標準出力へ出さない。

use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
use serde::Deserialize;
use serde_json::json;

/// ホスト名。Native Messaging のホスト名は英小文字、数字、`_`、`.` に限られるため、
/// アプリ識別子のハイフンを `_` に置き換えている。
pub const HOST_NAME: &str = "io.github.tkg_tamagohan.yt_browser";

/// 拡張の固定 ID（`extension/manifest.json` の `key` から導出される値）。
pub const EXTENSION_ID: &str = "hmfhiknpeefgjdmkjoofahdpkflfnkdl";

/// 受け取るメッセージの上限。要求は URL 1 本なので小さく抑える。
const MAX_MESSAGE_LEN: u32 = 64 * 1024;

/// 本体プロセスとのローカル IPC に使うエンドポイント名（Windows 名前付きパイプ）。
#[cfg(windows)]
pub const LOCAL_IPC_PIPE: &str = r"\\.\pipe\io.github.tkg-tamagohan.yt-browser-nm-ipc";

/// 本体プロセスとのローカル IPC に使う Unix ドメインソケットのパス。
#[cfg(unix)]
pub fn local_ipc_socket_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("native-ipc.sock")
}

/// ローカル IPC の残骸を終了時に掃除する。Unix はソケットファイルを消す。
/// Windows 名前付きパイプはファイルを持たず最後のハンドル解放で消えるため何もしない。
pub fn cleanup_ipc_endpoint(app_data_dir: &Path) {
    #[cfg(unix)]
    {
        let sock_path = local_ipc_socket_path(app_data_dir);
        if let Err(e) = std::fs::remove_file(&sock_path) {
            if e.kind() != io::ErrorKind::NotFound {
                tracing::warn!(
                    error = %e,
                    path = %sock_path.display(),
                    "ローカル IPC ソケットを削除できない"
                );
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = app_data_dir;
    }
}

/// Unix ドメインソケットファイルを所有者のみ（`0600`）に制限する。
#[cfg(unix)]
fn restrict_socket_permissions(sock_path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(sock_path, std::fs::Permissions::from_mode(0o600))
}

/// スキームアクション（FR-29、仕様決定 AX）。`open` は再生・取り込み、
/// `queue` は一時キューへの追加（再生しない）、`playlist_add` はプレイリスト
/// への追加（FR-50、仕様決定 CA）。`list_playlists` はスキームを組まない
/// 直接応答の要求で、一覧を返した時点で処理が終わる。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Open,
    Queue,
    PlaylistAdd,
    ListPlaylists,
}

impl Action {
    /// `action` フィールドの受理値。省略は `open`、受理集合外は `invalid_action`。
    fn parse(raw: Option<&str>) -> Result<Self, ErrorCode> {
        match raw.unwrap_or("open") {
            "open" => Ok(Self::Open),
            "queue" => Ok(Self::Queue),
            "playlist_add" => Ok(Self::PlaylistAdd),
            "list_playlists" => Ok(Self::ListPlaylists),
            _ => Err(ErrorCode::InvalidAction),
        }
    }

    /// アクションに対応するスキーム URL のプレフィックス。
    fn scheme_prefix(self) -> &'static str {
        match self {
            Self::Open => "yt-browser://open?url=",
            Self::Queue => "yt-browser://queue?url=",
            Self::PlaylistAdd => "yt-browser://playlist_add?url=",
            // `handle_request` で一覧応答へ先回りするため、ここへは来ない
            Self::ListPlaylists => unreachable!("list_playlists はスキームを組まない"),
        }
    }
}

/// 引数に Chrome のホスト起動の印（`chrome-extension://<ID>/`）が含まれるか。
/// Windows では `--parent-window=<hwnd>` も渡されるが判定には使わない。
pub fn is_host_invocation<I, S>(args: I) -> bool
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    args.into_iter()
        .any(|a| a.as_ref().starts_with("chrome-extension://"))
}

/// ホストモードの本体。標準入出力で 1 往復して終了コードを返す。
pub fn run_host() -> i32 {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let response = match read_message(&mut stdin.lock()) {
        Ok(body) => handle_request(&body, deliver_scheme_url, read_playlists_default),
        Err(e) => {
            eprintln!("yt-browser native host: 要求を読めません: {e}");
            Response::Error(ErrorCode::InvalidMessage)
        }
    };
    let code = if matches!(response, Response::Error(_)) {
        1
    } else {
        0
    };
    if let Err(e) = write_message(&mut stdout.lock(), &response.to_json()) {
        eprintln!("yt-browser native host: 応答を書けません: {e}");
        return 1;
    }
    code
}

/// スキーム URL を「1) ローカル IPC 直接送信 → 2) 本体バイナリ直接 spawn →
/// 3) OS 既定ハンドラ」の順で配送する（単体検証用に各段階を差し替え可能）。
pub fn deliver_scheme_url_with<I, S, O>(
    scheme_url: &str,
    try_ipc: I,
    try_spawn: S,
    try_open: O,
) -> io::Result<()>
where
    I: FnOnce(&str) -> io::Result<()>,
    S: FnOnce(&str) -> io::Result<()>,
    O: FnOnce(&str) -> io::Result<()>,
{
    if try_ipc(scheme_url).is_ok() {
        return Ok(());
    }
    if try_spawn(scheme_url).is_ok() {
        return Ok(());
    }
    try_open(scheme_url)
}

fn deliver_scheme_url(scheme_url: &str) -> io::Result<()> {
    deliver_scheme_url_with(scheme_url, try_send_local_ipc, try_spawn_binary, |url| {
        open::that_detached(url)
    })
}

/// Windows 名前付きパイプへ 1 行のスキーム URL を送る。
/// `ERROR_PIPE_BUSY`（231）のときは最大 500ms（25ms 間隔）まで待機して再試行する。
#[cfg(windows)]
pub fn send_local_ipc_windows(pipe_name: &str, scheme_url: &str) -> io::Result<()> {
    const ERROR_PIPE_BUSY: i32 = 231;
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(500);
    let mut file = loop {
        match std::fs::OpenOptions::new().write(true).open(pipe_name) {
            Ok(f) => break f,
            Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY) => {
                if std::time::Instant::now() >= deadline {
                    return Err(e);
                }
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
            Err(e) => return Err(e),
        }
    };
    writeln!(file, "{scheme_url}")?;
    file.flush()
}

/// Unix ドメインソケットへ 1 行のスキーム URL を送る。
#[cfg(unix)]
pub fn send_local_ipc_unix(sock_path: &Path, scheme_url: &str) -> io::Result<()> {
    let mut stream = std::os::unix::net::UnixStream::connect(sock_path)?;
    let _ = stream.set_write_timeout(Some(std::time::Duration::from_secs(2)));
    writeln!(stream, "{scheme_url}")?;
    stream.flush()
}

fn try_send_local_ipc(scheme_url: &str) -> io::Result<()> {
    #[cfg(windows)]
    {
        send_local_ipc_windows(LOCAL_IPC_PIPE, scheme_url)
    }
    #[cfg(unix)]
    {
        let sock_path = app_data_dir()
            .map(|d| local_ipc_socket_path(&d))
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::NotFound, "データディレクトリを解決できない")
            })?;
        send_local_ipc_unix(&sock_path, scheme_url)
    }
    #[cfg(not(any(windows, unix)))]
    {
        let _ = scheme_url;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "ローカル IPC 未対応環境",
        ))
    }
}

fn try_spawn_binary(scheme_url: &str) -> io::Result<()> {
    let exe = std::env::current_exe()?;
    let appimage = std::env::var("APPIMAGE").ok();
    let binary = host_binary_path(appimage.as_deref(), exe);
    // Native Messaging の stdout パイプを子プロセスが継承すると Chrome 側の
    // read が閉じずハングするため stdio を明示的に /dev/null 相当へ切り離す
    std::process::Command::new(binary)
        .arg(scheme_url)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
}

type BoxFuture = std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>;
type IpcSpawner = std::sync::Arc<dyn Fn(BoxFuture) + Send + Sync>;
type IpcUrlHandler = std::sync::Arc<dyn Fn(String) + Send + Sync>;

enum IpcEndpoint {
    #[cfg(windows)]
    WindowsPipe(String),
    #[cfg(unix)]
    UnixSocket(PathBuf),
    #[cfg(not(any(windows, unix)))]
    Unsupported,
}

impl IpcEndpoint {
    fn default_for(app_data_dir: &Path) -> Self {
        #[cfg(windows)]
        {
            let _ = app_data_dir;
            Self::WindowsPipe(LOCAL_IPC_PIPE.to_owned())
        }
        #[cfg(unix)]
        {
            Self::UnixSocket(local_ipc_socket_path(app_data_dir))
        }
        #[cfg(not(any(windows, unix)))]
        {
            let _ = app_data_dir;
            Self::Unsupported
        }
    }
}

async fn handle_ipc_client<R>(reader: R, on_url: IpcUrlHandler)
where
    R: tokio::io::AsyncRead + Unpin,
{
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};

    let mut limited = BufReader::new(reader.take(u64::from(MAX_MESSAGE_LEN)));
    let mut line = String::new();
    let res = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        limited.read_line(&mut line),
    )
    .await;
    if let Ok(Ok(n)) = res {
        if n > 0 {
            let trimmed = line.trim();
            if crate::deep_link::is_app_url(trimmed) {
                on_url(trimmed.to_owned());
            }
        }
    }
}

async fn run_ipc_listener(
    endpoint: IpcEndpoint,
    spawn: IpcSpawner,
    on_url: IpcUrlHandler,
    ready: Option<tokio::sync::oneshot::Sender<io::Result<()>>>,
) {
    match endpoint {
        #[cfg(windows)]
        IpcEndpoint::WindowsPipe(pipe_name) => {
            use tokio::net::windows::named_pipe::ServerOptions;
            let mut server = match ServerOptions::new()
                .reject_remote_clients(true)
                .create(&pipe_name)
            {
                Ok(s) => {
                    if let Some(tx) = ready {
                        let _ = tx.send(Ok(()));
                    }
                    s
                }
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        pipe = %pipe_name,
                        "ローカル IPC パイプを作成できない"
                    );
                    if let Some(tx) = ready {
                        let _ = tx.send(Err(e));
                    }
                    return;
                }
            };
            loop {
                if let Err(e) = server.connect().await {
                    tracing::warn!(error = %e, "ローカル IPC パイプの接続待機に失敗");
                    break;
                }
                let connected = server;
                let client_url = on_url.clone();
                server = match ServerOptions::new()
                    .reject_remote_clients(true)
                    .create(&pipe_name)
                {
                    Ok(s) => s,
                    Err(e) => {
                        spawn(Box::pin(handle_ipc_client(connected, client_url)));
                        tracing::warn!(error = %e, "ローカル IPC 次インスタンスを作成できない");
                        break;
                    }
                };
                spawn(Box::pin(handle_ipc_client(connected, client_url)));
            }
        }
        #[cfg(unix)]
        IpcEndpoint::UnixSocket(sock_path) => {
            let _ = std::fs::remove_file(&sock_path);
            let listener = match tokio::net::UnixListener::bind(&sock_path) {
                Ok(l) => {
                    // ソケットファイルは既定で他ユーザーからも書き込めるため
                    // 所有者のみ（0600）へ制限する。失敗しても待ち受けは継続する
                    if let Err(e) = restrict_socket_permissions(&sock_path) {
                        tracing::warn!(
                            error = %e,
                            path = %sock_path.display(),
                            "ローカル IPC ソケットの権限を 0600 に設定できない"
                        );
                    }
                    if let Some(tx) = ready {
                        let _ = tx.send(Ok(()));
                    }
                    l
                }
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        path = %sock_path.display(),
                        "ローカル IPC ソケットを作成できない"
                    );
                    if let Some(tx) = ready {
                        let _ = tx.send(Err(e));
                    }
                    return;
                }
            };
            loop {
                match listener.accept().await {
                    Ok((stream, _addr)) => {
                        let client_url = on_url.clone();
                        spawn(Box::pin(handle_ipc_client(stream, client_url)));
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "ローカル IPC ソケットの接続待機に失敗");
                        break;
                    }
                }
            }
        }
        #[cfg(not(any(windows, unix)))]
        IpcEndpoint::Unsupported => {
            let _ = (spawn, on_url);
            if let Some(tx) = ready {
                let _ = tx.send(Ok(()));
            }
        }
    }
}

fn start_ipc_listener_with(
    endpoint: IpcEndpoint,
    spawn: IpcSpawner,
    on_url: IpcUrlHandler,
    ready: Option<tokio::sync::oneshot::Sender<io::Result<()>>>,
) {
    let listener_spawn = spawn.clone();
    spawn(Box::pin(run_ipc_listener(
        endpoint,
        listener_spawn,
        on_url,
        ready,
    )));
}

/// 本体起動時にローカル IPC（名前付きパイプ / Unix ソケット）を待ち受け、
/// Native Messaging ホストから直接届いた `yt-browser://` URL を
/// `deep_link::handle_open_url` へ渡す。作成失敗は警告に留め起動は止めない。
///
/// 注意: Tauri の `setup` フックは Tokio ランタイム外で同期実行されるため、
/// `ServerOptions::create` / `UnixListener::bind` も `spawn` した非同期タスク内で実行する。
pub fn start_ipc_listener(app: tauri::AppHandle, app_data_dir: &Path) {
    let spawner: IpcSpawner = std::sync::Arc::new(|fut| {
        tauri::async_runtime::spawn(fut);
    });
    let on_url: IpcUrlHandler = std::sync::Arc::new(move |url| {
        crate::deep_link::handle_open_url(&app, &url);
    });
    start_ipc_listener_with(
        IpcEndpoint::default_for(app_data_dir),
        spawner,
        on_url,
        None,
    );
}

/// 応答のエラーコード。拡張はコードをツールチップの理由表示に使う。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    /// 長さ付き JSON として読めない、`url` が文字列でない、
    /// 必須フィールド（`url` / `playlist_id`）が欠けた・形が不正
    InvalidMessage,
    /// `url` が http(s) の URL として解析できない
    InvalidUrl,
    /// `action` が受理集合（`open` / `queue` / `playlist_add` / `list_playlists`）外
    InvalidAction,
    /// OS の既定ハンドラで開けなかった
    OpenFailed,
    /// `list_playlists` で本体の DB を開けない・読めない
    DbUnavailable,
}

impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InvalidMessage => "invalid_message",
            Self::InvalidUrl => "invalid_url",
            Self::InvalidAction => "invalid_action",
            Self::OpenFailed => "open_failed",
            Self::DbUnavailable => "db_unavailable",
        }
    }
}

/// `list_playlists` が応答へ載せるプレイリストの要約。
/// 拡張の選択肢表示に必要な ID と名前だけを持つ（FR-50、仕様決定 CA）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistSummary {
    pub id: i64,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Response {
    Ok,
    Playlists(Vec<PlaylistSummary>),
    Error(ErrorCode),
}

impl Response {
    pub fn to_json(self) -> serde_json::Value {
        match self {
            Self::Ok => json!({ "ok": true }),
            Self::Playlists(list) => json!({
                "ok": true,
                "playlists": list
                    .iter()
                    .map(|p| json!({ "id": p.id, "name": p.name }))
                    .collect::<Vec<_>>()
            }),
            Self::Error(code) => json!({ "ok": false, "error": code.as_str() }),
        }
    }
}

/// 4 バイトのネイティブエンディアン長に続く本文を 1 件読む。
pub fn read_message<R: Read>(reader: &mut R) -> io::Result<Vec<u8>> {
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf)?;
    let len = u32::from_ne_bytes(len_buf);
    if len > MAX_MESSAGE_LEN {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("メッセージ長 {len} が上限 {MAX_MESSAGE_LEN} を超える"),
        ));
    }
    let mut body = vec![0u8; len as usize];
    reader.read_exact(&mut body)?;
    Ok(body)
}

/// JSON 値を 4 バイトのネイティブエンディアン長付きで書く。
pub fn write_message<W: Write>(writer: &mut W, value: &serde_json::Value) -> io::Result<()> {
    let body = serde_json::to_vec(value)?;
    let len = u32::try_from(body.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "応答が大きすぎる"))?;
    writer.write_all(&len.to_ne_bytes())?;
    writer.write_all(&body)?;
    writer.flush()
}

#[derive(Deserialize)]
struct Request {
    /// 対象 URL。`list_playlists` だけは URL を取らず省略される
    url: Option<String>,
    action: Option<String>,
    /// `playlist_add` で必須のプレイリスト ID（`playlists.id`）
    playlist_id: Option<i64>,
}

/// `url` が http(s) の URL として解析できるかだけを確かめる。
/// 動画とプレイリストの振り分けは本体の `deeplink.svelte.ts` が行う。
pub fn validate_url(raw: &str) -> bool {
    match url::Url::parse(raw) {
        Ok(u) => matches!(u.scheme(), "http" | "https") && u.host_str().is_some(),
        Err(_) => false,
    }
}

/// 対象 URL をアプリのスキーム URL へ包む。
/// `playlist_add` だけ `playlist_id` をクエリへ追加する（FR-50、仕様決定 CA）。
pub fn scheme_url(target: &str, action: Action, playlist_id: Option<i64>) -> String {
    let mut url = format!(
        "{}{}",
        action.scheme_prefix(),
        utf8_percent_encode(target, NON_ALPHANUMERIC)
    );
    if let Some(id) = playlist_id {
        url.push_str(&format!("&playlist_id={id}"));
    }
    url
}

/// 要求本文を解釈する。`list_playlists` は `lister` の結果を直接応答へ、
/// それ以外は検証を通れば `opener` でスキーム URL を開く。
pub fn handle_request<F, L>(body: &[u8], opener: F, lister: L) -> Response
where
    F: FnOnce(&str) -> io::Result<()>,
    L: FnOnce() -> io::Result<Vec<PlaylistSummary>>,
{
    let req: Request = match serde_json::from_slice(body) {
        Ok(r) => r,
        Err(_) => return Response::Error(ErrorCode::InvalidMessage),
    };
    let action = match Action::parse(req.action.as_deref()) {
        Ok(a) => a,
        Err(code) => return Response::Error(code),
    };
    // list_playlists は URL を取らない一覧要求で、DB から読んだ結果をそのまま返す。
    // DB が開けない・読めないときは db_unavailable で失敗を知らせる
    if action == Action::ListPlaylists {
        return match lister() {
            Ok(list) => Response::Playlists(list),
            Err(e) => {
                eprintln!("yt-browser native host: プレイリスト一覧を読めません: {e}");
                Response::Error(ErrorCode::DbUnavailable)
            }
        };
    }
    let url = match req.url {
        Some(u) => u,
        // open / queue / playlist_add では url が必須。欠落は
        // 「url が文字列でない」と同じ invalid_message に揃える
        None => return Response::Error(ErrorCode::InvalidMessage),
    };
    if !validate_url(&url) {
        return Response::Error(ErrorCode::InvalidUrl);
    }
    // playlist_add は playlist_id が必須。0 以下や欠落は本体の
    // `deeplink.svelte.ts` が弾く値と同じく、ここで invalid_message にする
    let playlist_id = match action {
        Action::PlaylistAdd => match req.playlist_id {
            Some(id) if id > 0 => Some(id),
            _ => return Response::Error(ErrorCode::InvalidMessage),
        },
        _ => None,
    };
    match opener(&scheme_url(&url, action, playlist_id)) {
        Ok(()) => Response::Ok,
        Err(e) => {
            eprintln!("yt-browser native host: スキーム URL を開けません: {e}");
            Response::Error(ErrorCode::OpenFailed)
        }
    }
}

/// アプリのデータディレクトリ。Tauri の `app_data_dir` と同じ規則で解決する
/// （Windows は `%APPDATA%\<identifier>`、Linux は
/// `$XDG_DATA_HOME` または `~/.local/share` 配下）。
/// ホストモードでは PathResolver を組めないため OS ごとに自前で解決する。
/// macOS も Linux と同じ解決になるが、対象環境は Windows / Linux のみなので実害はない。
fn app_data_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .map(|d| d.join("io.github.tkg-tamagohan.yt-browser"))
    }
    #[cfg(not(windows))]
    {
        let base = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")));
        base.map(|d| d.join("io.github.tkg-tamagohan.yt-browser"))
    }
}

/// 本体 DB（`<app_data_dir>/yt-browser.db`）の `playlists` 表を読み取り専用で開き、
/// `playlist_list` と同じ `ORDER BY sort_order, id` で ID と名前を返す。
/// 本体未インストール・DB 不在・読めない場合は Err で、`db_unavailable` に写像する。
fn read_playlists_default() -> io::Result<Vec<PlaylistSummary>> {
    let db = app_data_dir()
        .map(|d| d.join("yt-browser.db"))
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, "データディレクトリを解決できない")
        })?;
    read_playlists(&db)
}

/// `path` の DB を読み取り専用で開いて `playlists` 表を読む。
/// 本体が起動中でも WAL モードなら読み取りは並行して成功する。
fn read_playlists(path: &Path) -> io::Result<Vec<PlaylistSummary>> {
    let conn =
        rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(io::Error::other)?;
    // WAL チェックポイントや書込トランザクションと重なっても即座に BUSY で失敗しないよう待機する
    let _ = conn.busy_timeout(std::time::Duration::from_millis(5000));
    let mut stmt = conn
        .prepare("SELECT id, name FROM playlists ORDER BY sort_order, id")
        .map_err(io::Error::other)?;
    let rows = stmt
        .query_map([], |r| {
            Ok(PlaylistSummary {
                id: r.get(0)?,
                name: r.get(1)?,
            })
        })
        .map_err(io::Error::other)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(io::Error::other)?);
    }
    Ok(out)
}

/// ホスト定義 JSON の本文。`path` はホストとして起動されるバイナリの絶対パス。
pub fn host_manifest(binary: &Path) -> String {
    let value = json!({
        "name": HOST_NAME,
        "description": "yt-browser native messaging host",
        "path": binary.to_string_lossy(),
        "type": "stdio",
        "allowed_origins": [format!("chrome-extension://{EXTENSION_ID}/")],
    });
    // to_string_pretty は Value に対して失敗しない
    serde_json::to_string_pretty(&value).unwrap_or_default() + "\n"
}

/// ホスト定義に書くバイナリのパス。AppImage ではマウント先の一時パスではなく
/// `$APPIMAGE`（AppImage ファイル自体のパス）を使う。
pub fn host_binary_path(appimage: Option<&str>, current_exe: PathBuf) -> PathBuf {
    match appimage {
        Some(p) if !p.is_empty() => PathBuf::from(p),
        _ => current_exe,
    }
}

/// 内容が同じなら書き換えずに `false` を返す。書いたら `true`。
pub fn write_if_changed(path: &Path, content: &str) -> io::Result<bool> {
    if std::fs::read_to_string(path).is_ok_and(|cur| cur == content) {
        return Ok(false);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, content)?;
    Ok(true)
}

/// Linux の登録対象（FR-88、仕様決定 DM）。`<config_dir>/<dir>` がブラウザの
/// 設定ディレクトリ名で、その下の `NativeMessagingHosts/<ホスト名>.json` に置く。
/// `installed_only` が真の対象は、設定ディレクトリが既にあるとき
/// （ブラウザの初回起動済みとみなせるとき）だけ書き込む暫定解釈。
/// 未導入ブラウザ向けに空の設定ディレクトリを新規作成しないためで、
/// 後から導入した場合はブラウザの初回起動後の本体起動で登録される。
/// Chrome は従来どおり無条件に書く（仕様決定 AH の既存挙動を維持）。
#[cfg(target_os = "linux")]
const LINUX_TARGETS: &[(&str, bool)] = &[
    ("google-chrome", false),
    ("microsoft-edge", true),
    ("chromium", true),
];

/// Linux の登録先パス一覧。`installed_only` の対象は設定ディレクトリが
/// 存在するものだけを返す（見つからないものはブラウザ未導入として飛ばす）。
#[cfg(target_os = "linux")]
fn linux_manifest_paths(config_dir: &Path) -> Vec<PathBuf> {
    LINUX_TARGETS
        .iter()
        .filter(|(dir, installed_only)| !installed_only || config_dir.join(dir).is_dir())
        .map(|(dir, _)| {
            config_dir
                .join(dir)
                .join("NativeMessagingHosts")
                .join(format!("{HOST_NAME}.json"))
        })
        .collect()
}

/// Windows の登録先レジストリキーの起点（HKCU からの相対パス）。
/// `<起点>\NativeMessagingHosts\<ホスト名>` の既定値へマニフェストのパスを書く。
/// ブラウザ導入の有無は判定せず全対象へ書く暫定解釈（Chrome の既存挙動を踏襲。
/// 未導入ブラウザの残存キーは無害と判断）。`Software\Chromium` は
/// 仕様決定 DM の「Edge と Chrome」に対し FR-88 の「Chromium 系」を
/// Windows の Chromium 系ブラウザにも広げた暫定解釈の追加対象。
#[cfg(windows)]
const WINDOWS_REGISTRIES: &[&str] = &[
    r"Software\Google\Chrome",
    r"Software\Microsoft\Edge",
    r"Software\Chromium",
];

/// Windows の登録先レジストリキー（HKCU からの相対パス）の一覧。
#[cfg(windows)]
fn windows_registry_subkeys() -> Vec<String> {
    WINDOWS_REGISTRIES
        .iter()
        .map(|root| format!(r"{root}\NativeMessagingHosts\{HOST_NAME}"))
        .collect()
}

/// 本体の setup から呼ぶ自己登録。失敗は警告ログに留め、起動は止めない。
/// 対象ブラウザの置き場は [`linux_manifest_paths`]（Linux）と
/// [`windows_registry_subkeys`]（Windows）が列挙する（FR-88、仕様決定 DM）。
///
/// - Linux：`<config_dir>/<ブラウザ設定名>/NativeMessagingHosts/<ホスト名>.json`
/// - Windows：JSON を `<app_data>/native-messaging/` に置き、
///   各ブラウザの `HKCU\<起点>\NativeMessagingHosts\<ホスト名>` の既定値にそのパスを書く
#[allow(unused_variables)]
pub fn register(config_dir: Option<PathBuf>, app_data_dir: &Path) {
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(error = %e, "Native Messaging ホスト登録: 実行パスを取得できない");
            return;
        }
    };
    let appimage = std::env::var("APPIMAGE").ok();
    let binary = host_binary_path(appimage.as_deref(), exe);
    let manifest = host_manifest(&binary);

    #[cfg(target_os = "linux")]
    {
        let Some(config_dir) = config_dir else {
            tracing::warn!("Native Messaging ホスト登録: 設定ディレクトリを取得できない");
            return;
        };
        for path in linux_manifest_paths(&config_dir) {
            report_write(&path, write_if_changed(&path, &manifest));
        }
    }

    #[cfg(windows)]
    {
        let path = app_data_dir
            .join("native-messaging")
            .join(format!("{HOST_NAME}.json"));
        let written = write_if_changed(&path, &manifest);
        let ok = written.is_ok();
        report_write(&path, written);
        if ok {
            register_windows_keys(&path);
        }
    }
}

#[cfg(any(target_os = "linux", windows))]
fn report_write(path: &Path, result: io::Result<bool>) {
    match result {
        Ok(true) => tracing::info!(path = %path.display(), "Native Messaging ホスト定義を更新"),
        Ok(false) => tracing::debug!(path = %path.display(), "Native Messaging ホスト定義は最新"),
        Err(e) => tracing::warn!(
            path = %path.display(),
            error = %e,
            "Native Messaging ホスト定義を書けない"
        ),
    }
}

/// Windows 側の登録。対象ブラウザの `NativeMessagingHosts` キーそれぞれの
/// 既定値へマニフェストのパスを書く。1 キーの失敗で他への登録は止めない。
#[cfg(windows)]
fn register_windows_keys(manifest_path: &Path) {
    let value = manifest_path.to_string_lossy().into_owned();
    for subkey in windows_registry_subkeys() {
        if let Err(e) = write_windows_key(&subkey, &value) {
            tracing::warn!(key = %subkey, error = %e, "Native Messaging ホスト登録: レジストリを書けない");
        }
    }
}

/// `subkey` の既定値へ `value` を冪等に書く。内容が同じなら書き換えない。
#[cfg(windows)]
fn write_windows_key(subkey: &str, value: &str) -> windows_registry::Result<()> {
    let key = windows_registry::CURRENT_USER.create(subkey)?;
    if key.get_string("").is_ok_and(|cur| cur == value) {
        return Ok(());
    }
    key.set_string("", value)?;
    tracing::info!(key = %subkey, "Native Messaging ホストのレジストリを更新");
    Ok(())
}

#[cfg(test)]
#[path = "native_host_tests.rs"]
mod tests;
