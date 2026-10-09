//! Chrome 拡張の Native Messaging ホスト（FR-17、仕様決定 AH、設計書 §3.4.1）。
//!
//! 本体バイナリの別モードとして動く。Chrome はホスト起動時に引数
//! `chrome-extension://<ID>/` を渡すので、`main.rs` がそれを検出したら
//! Tauri を組まずに [`run_host`] へ分岐する。
//!
//! - 通信：4 バイトのネイティブエンディアン長と UTF-8 JSON の組を 1 往復だけ行う。
//!   要求は `{"url": "<対象 URL>"}`、応答は `{"ok": true}` か
//!   `{"ok": false, "error": "<コード>"}`
//! - 起動：`yt-browser://open?url=<encoded>` を OS の既定ハンドラで開く。
//!   以降は既存の deep link 受信経路（未起動なら起動、起動中なら
//!   single-instance で転送）に乗る
//! - 登録：本体の setup で [`register`] を呼び、ホスト定義を冪等に書く
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

const SCHEME_PREFIX: &str = "yt-browser://open?url=";

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
        Ok(body) => handle_request(&body, |url| open::that_detached(url)),
        Err(e) => {
            eprintln!("yt-browser native host: 要求を読めません: {e}");
            Response::Error(ErrorCode::InvalidMessage)
        }
    };
    let code = if matches!(response, Response::Ok) {
        0
    } else {
        1
    };
    if let Err(e) = write_message(&mut stdout.lock(), &response.to_json()) {
        eprintln!("yt-browser native host: 応答を書けません: {e}");
        return 1;
    }
    code
}

/// 応答のエラーコード。拡張はコードをツールチップの理由表示に使う。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    /// 長さ付き JSON として読めない、または `url` が文字列でない
    InvalidMessage,
    /// `url` が http(s) の URL として解析できない
    InvalidUrl,
    /// OS の既定ハンドラで開けなかった
    OpenFailed,
}

impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InvalidMessage => "invalid_message",
            Self::InvalidUrl => "invalid_url",
            Self::OpenFailed => "open_failed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Response {
    Ok,
    Error(ErrorCode),
}

impl Response {
    pub fn to_json(self) -> serde_json::Value {
        match self {
            Self::Ok => json!({ "ok": true }),
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
    url: String,
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
pub fn scheme_url(target: &str) -> String {
    format!(
        "{SCHEME_PREFIX}{}",
        utf8_percent_encode(target, NON_ALPHANUMERIC)
    )
}

/// 要求本文を解釈し、検証を通れば `opener` でスキーム URL を開く。
pub fn handle_request<F>(body: &[u8], opener: F) -> Response
where
    F: FnOnce(&str) -> io::Result<()>,
{
    let req: Request = match serde_json::from_slice(body) {
        Ok(r) => r,
        Err(_) => return Response::Error(ErrorCode::InvalidMessage),
    };
    if !validate_url(&req.url) {
        return Response::Error(ErrorCode::InvalidUrl);
    }
    match opener(&scheme_url(&req.url)) {
        Ok(()) => Response::Ok,
        Err(e) => {
            eprintln!("yt-browser native host: スキーム URL を開けません: {e}");
            Response::Error(ErrorCode::OpenFailed)
        }
    }
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

/// 本体の setup から呼ぶ自己登録。失敗は警告ログに留め、起動は止めない。
///
/// - Linux：`<config_dir>/google-chrome/NativeMessagingHosts/<ホスト名>.json`
/// - Windows：JSON を `<app_data>/native-messaging/` に置き、
///   `HKCU\Software\Google\Chrome\NativeMessagingHosts\<ホスト名>` の既定値にそのパスを書く
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
        let path = config_dir
            .join("google-chrome")
            .join("NativeMessagingHosts")
            .join(format!("{HOST_NAME}.json"));
        report_write(&path, write_if_changed(&path, &manifest));
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
            if let Err(e) = register_windows_key(&path) {
                tracing::warn!(error = %e, "Native Messaging ホスト登録: レジストリを書けない");
            }
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

#[cfg(windows)]
fn register_windows_key(manifest_path: &Path) -> windows_registry::Result<()> {
    let subkey = format!(r"Software\Google\Chrome\NativeMessagingHosts\{HOST_NAME}");
    let value = manifest_path.to_string_lossy().into_owned();
    let key = windows_registry::CURRENT_USER.create(&subkey)?;
    if key.get_string("").is_ok_and(|cur| cur == value) {
        return Ok(());
    }
    key.set_string("", &value)?;
    tracing::info!(key = %subkey, "Native Messaging ホストのレジストリを更新");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn framed(body: &[u8]) -> Vec<u8> {
        let mut v = (body.len() as u32).to_ne_bytes().to_vec();
        v.extend_from_slice(body);
        v
    }

    /// DB-NM-01: 長さ付き JSON の読み書き（設計書 §3.4.1 の通信形式）。
    /// 書いた応答は 4 バイトのネイティブエンディアン長と本文の組で、同じ形で読み戻せる。
    #[test]
    fn message_round_trip() {
        let mut buf = Vec::new();
        write_message(&mut buf, &Response::Ok.to_json()).unwrap();
        let len = u32::from_ne_bytes(buf[..4].try_into().unwrap()) as usize;
        assert_eq!(len, buf.len() - 4);
        let body = read_message(&mut buf.as_slice()).unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v, json!({ "ok": true }));

        let mut buf = Vec::new();
        write_message(&mut buf, &Response::Error(ErrorCode::InvalidUrl).to_json()).unwrap();
        let body = read_message(&mut buf.as_slice()).unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v, json!({ "ok": false, "error": "invalid_url" }));
    }

    /// DB-NM-02: 読み取りの異常系。
    /// 長さが本文より長い途中切れと、上限を超える長さはエラーにする。
    #[test]
    fn read_message_rejects_truncated_and_oversized() {
        let mut short = 10u32.to_ne_bytes().to_vec();
        short.extend_from_slice(b"{}");
        assert!(read_message(&mut short.as_slice()).is_err());

        let huge = (MAX_MESSAGE_LEN + 1).to_ne_bytes().to_vec();
        assert!(read_message(&mut huge.as_slice()).is_err());

        let empty: &[u8] = &[];
        assert!(read_message(&mut &*empty).is_err());
    }

    /// DB-NM-03: `url` の検証。http(s) の URL として解析できるものだけを受理する。
    /// 動画とプレイリストの振り分けはホストでは行わない。
    #[test]
    fn validate_url_accepts_only_http_urls() {
        assert!(validate_url("https://www.youtube.com/watch?v=dQw4w9WgXcQ"));
        assert!(validate_url("http://youtu.be/dQw4w9WgXcQ"));
        assert!(validate_url("https://example.com/"));
        assert!(!validate_url("yt-browser://open?url=x"));
        assert!(!validate_url("javascript:alert(1)"));
        assert!(!validate_url("file:///etc/passwd"));
        assert!(!validate_url("not a url"));
        assert!(!validate_url(""));
    }

    /// DB-NM-04: 要求の処理。正常系ではスキーム URL を 1 回だけ開き、`ok: true` を返す。
    /// `url` は `yt-browser://open?url=` へ符号化して渡す。
    #[test]
    fn handle_request_opens_scheme_url() {
        let target = "https://www.youtube.com/watch?v=dQw4w9WgXcQ&list=PL1";
        let body = serde_json::to_vec(&json!({ "url": target })).unwrap();
        let mut opened = Vec::new();
        let res = handle_request(&body, |u| {
            opened.push(u.to_string());
            Ok(())
        });
        assert_eq!(res, Response::Ok);
        assert_eq!(opened.len(), 1);
        let scheme = url::Url::parse(&opened[0]).unwrap();
        assert_eq!(scheme.scheme(), "yt-browser");
        let decoded: Vec<(String, String)> = scheme.query_pairs().into_owned().collect();
        assert_eq!(decoded, vec![("url".to_string(), target.to_string())]);
    }

    /// DB-NM-05: 要求の異常系。壊れた JSON や `url` 欠落は `invalid_message`、
    /// http(s) でない URL は `invalid_url`、開けなければ `open_failed` を返す。
    /// 検証で弾いた要求ではスキーム URL を開かない。
    #[test]
    fn handle_request_error_codes() {
        let never = |_: &str| -> io::Result<()> { panic!("開いてはいけない") };
        assert_eq!(
            handle_request(b"not json", never),
            Response::Error(ErrorCode::InvalidMessage)
        );
        assert_eq!(
            handle_request(br#"{"href":"https://youtu.be/x"}"#, never),
            Response::Error(ErrorCode::InvalidMessage)
        );
        assert_eq!(
            handle_request(br#"{"url":42}"#, never),
            Response::Error(ErrorCode::InvalidMessage)
        );
        assert_eq!(
            handle_request(br#"{"url":"ftp://example.com/"}"#, never),
            Response::Error(ErrorCode::InvalidUrl)
        );
        let fail = |_: &str| -> io::Result<()> { Err(io::Error::other("no handler")) };
        assert_eq!(
            handle_request(br#"{"url":"https://youtu.be/dQw4w9WgXcQ"}"#, fail),
            Response::Error(ErrorCode::OpenFailed)
        );
    }

    /// DB-NM-06: ホストモードの判定。`chrome-extension://<ID>/` 引数があるときだけ真。
    /// deep link の `yt-browser:` 引数や通常起動では偽。
    #[test]
    fn detects_host_invocation() {
        let ext = format!("chrome-extension://{EXTENSION_ID}/");
        assert!(is_host_invocation(["/opt/yt-browser", ext.as_str()]));
        assert!(is_host_invocation([
            r"C:\yt-browser.exe",
            ext.as_str(),
            "--parent-window=0"
        ]));
        assert!(!is_host_invocation(["/opt/yt-browser"]));
        assert!(!is_host_invocation([
            "/opt/yt-browser",
            "yt-browser://open?url=https%3A%2F%2Fyoutu.be%2Fx"
        ]));
    }

    /// DB-NM-07: ホスト定義 JSON の生成。
    /// ホスト名、stdio 型、バイナリの絶対パス、固定 ID の `allowed_origins` を持つ。
    #[test]
    fn host_manifest_fields() {
        let path = Path::new("/opt/yt-browser/yt-browser");
        let v: serde_json::Value = serde_json::from_str(&host_manifest(path)).unwrap();
        assert_eq!(v["name"], HOST_NAME);
        assert_eq!(v["type"], "stdio");
        assert_eq!(v["path"], "/opt/yt-browser/yt-browser");
        assert_eq!(
            v["allowed_origins"],
            json!([format!("chrome-extension://{EXTENSION_ID}/")])
        );
        assert!(HOST_NAME
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '.'));
    }

    /// DB-NM-08: AppImage のパス選択。`$APPIMAGE` があればそれを、空か未設定なら実行パスを使う。
    #[test]
    fn host_binary_prefers_appimage() {
        let exe = PathBuf::from("/tmp/.mount_xyz/usr/bin/yt-browser");
        assert_eq!(
            host_binary_path(Some("/home/u/yt-browser.AppImage"), exe.clone()),
            PathBuf::from("/home/u/yt-browser.AppImage")
        );
        assert_eq!(host_binary_path(Some(""), exe.clone()), exe);
        assert_eq!(host_binary_path(None, exe.clone()), exe);
    }

    /// DB-NM-09: 冪等な書き込み。初回と内容変更時だけ書き、同じ内容なら書き換えない。
    #[test]
    fn write_if_changed_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("NativeMessagingHosts").join("h.json");
        assert!(write_if_changed(&path, "a").unwrap());
        assert!(!write_if_changed(&path, "a").unwrap());
        assert!(write_if_changed(&path, "b").unwrap());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "b");
    }

    /// DB-NM-10: 実際の標準入出力と同じ形の 1 往復。
    /// フレーム化した要求を読み、処理し、フレーム化した応答を書く。
    #[test]
    fn framed_request_to_framed_response() {
        let input = framed(br#"{"url":"https://www.youtube.com/playlist?list=PL1"}"#);
        let body = read_message(&mut input.as_slice()).unwrap();
        let res = handle_request(&body, |_| Ok(()));
        let mut out = Vec::new();
        write_message(&mut out, &res.to_json()).unwrap();
        let back = read_message(&mut out.as_slice()).unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&back).unwrap(),
            json!({ "ok": true })
        );
    }
}
