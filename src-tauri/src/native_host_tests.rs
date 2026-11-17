use super::*;

fn framed(body: &[u8]) -> Vec<u8> {
    let mut v = (body.len() as u32).to_ne_bytes().to_vec();
    v.extend_from_slice(body);
    v
}

/// スキーム系テストで `list_playlists` 用に渡すスタブ。
/// 一覧要求でない限り呼ばれない前提の空リストを返す。
fn empty_playlists() -> io::Result<Vec<PlaylistSummary>> {
    Ok(Vec::new())
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
    let res = handle_request(
        &body,
        |u| {
            opened.push(u.to_string());
            Ok(())
        },
        empty_playlists,
    );
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
        handle_request(b"not json", never, empty_playlists),
        Response::Error(ErrorCode::InvalidMessage)
    );
    assert_eq!(
        handle_request(br#"{"href":"https://youtu.be/x"}"#, never, empty_playlists),
        Response::Error(ErrorCode::InvalidMessage)
    );
    assert_eq!(
        handle_request(br#"{"url":42}"#, never, empty_playlists),
        Response::Error(ErrorCode::InvalidMessage)
    );
    assert_eq!(
        handle_request(br#"{"url":"ftp://example.com/"}"#, never, empty_playlists),
        Response::Error(ErrorCode::InvalidUrl)
    );
    let fail = |_: &str| -> io::Result<()> { Err(io::Error::other("no handler")) };
    assert_eq!(
        handle_request(
            br#"{"url":"https://youtu.be/dQw4w9WgXcQ"}"#,
            fail,
            empty_playlists
        ),
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

/// DB-NM-11: `action` フィールド（FR-29、仕様決定 AX）。
/// 省略は `open` と同じスキームを組み立て、`queue` は
/// `yt-browser://queue?url=` を開く。受理集合外は `invalid_action` で、
/// URL 検証より先に弾く（開かない）。
#[test]
fn handle_request_action_variants() {
    let opened = std::cell::RefCell::new(Vec::<String>::new());
    // handle_request は FnOnce を受け取るため、呼び出しごとに
    // 記録用の内部クロージャを新しく作る
    let call = |body: &[u8]| -> Response {
        handle_request(
            body,
            |u| {
                opened.borrow_mut().push(u.to_string());
                Ok(())
            },
            empty_playlists,
        )
    };
    // action 省略は従来どおり open スキーム
    let body = serde_json::to_vec(&json!({ "url": "https://youtu.be/dQw4w9WgXcQ" })).unwrap();
    assert_eq!(call(&body), Response::Ok);
    assert!(opened.borrow()[0].starts_with("yt-browser://open?url="));
    // action: "open" の明示も同じ
    let body =
        serde_json::to_vec(&json!({ "url": "https://youtu.be/dQw4w9WgXcQ", "action": "open" }))
            .unwrap();
    assert_eq!(call(&body), Response::Ok);
    assert!(opened.borrow()[1].starts_with("yt-browser://open?url="));
    // action: "queue" は queue スキーム
    let body =
        serde_json::to_vec(&json!({ "url": "https://youtu.be/dQw4w9WgXcQ", "action": "queue" }))
            .unwrap();
    assert_eq!(call(&body), Response::Ok);
    assert!(opened.borrow()[2].starts_with("yt-browser://queue?url="));
    // 受理集合外は invalid_action で、URL 検証より先に弾く
    let never = |_: &str| -> io::Result<()> { panic!("開いてはいけない") };
    for action in ["play", "QUEUE", "", " open"] {
        let body =
            serde_json::to_vec(&json!({ "url": "https://youtu.be/dQw4w9WgXcQ", "action": action }))
                .unwrap();
        assert_eq!(
            handle_request(&body, never, empty_playlists),
            Response::Error(ErrorCode::InvalidAction),
            "action={action:?}"
        );
    }
    let body = serde_json::to_vec(&json!({ "url": "not a url", "action": "play" })).unwrap();
    assert_eq!(
        handle_request(&body, never, empty_playlists),
        Response::Error(ErrorCode::InvalidAction)
    );
    // action が文字列でない要求は invalid_message
    let body =
        serde_json::to_vec(&json!({ "url": "https://youtu.be/dQw4w9WgXcQ", "action": 5 })).unwrap();
    assert_eq!(
        handle_request(&body, never, empty_playlists),
        Response::Error(ErrorCode::InvalidMessage)
    );
}

/// DB-NM-12: `playlist_add` アクション（FR-50、仕様決定 CA）。
/// `yt-browser://playlist_add?url=<enc>&playlist_id=<id>` を開き、
/// `playlist_id` が欠けた・0 以下・数値でない要求は `invalid_message` にする。
#[test]
fn handle_request_playlist_add() {
    let opened = std::cell::RefCell::new(Vec::<String>::new());
    let call = |body: &[u8]| -> Response {
        handle_request(
            body,
            |u| {
                opened.borrow_mut().push(u.to_string());
                Ok(())
            },
            empty_playlists,
        )
    };
    let body = serde_json::to_vec(
        &json!({ "url": "https://youtu.be/dQw4w9WgXcQ", "action": "playlist_add",
                      "playlist_id": 7 }),
    )
    .unwrap();
    assert_eq!(call(&body), Response::Ok);
    let scheme = url::Url::parse(&opened.borrow()[0]).unwrap();
    let decoded: Vec<(String, String)> = scheme.query_pairs().into_owned().collect();
    assert_eq!(
        decoded,
        vec![
            (
                "url".to_string(),
                "https://youtu.be/dQw4w9WgXcQ".to_string()
            ),
            ("playlist_id".to_string(), "7".to_string())
        ]
    );
    // playlist_id の欠落・非整数・0・負数は invalid_message で開かない
    let never = |_: &str| -> io::Result<()> { panic!("開いてはいけない") };
    for extra in [
        json!({}),
        json!({ "playlist_id": 0 }),
        json!({ "playlist_id": -1 }),
        json!({ "playlist_id": "7" }),
        json!({ "playlist_id": 1.5 }),
    ] {
        let mut req = json!({ "url": "https://youtu.be/dQw4w9WgXcQ", "action": "playlist_add" });
        req.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        let body = serde_json::to_vec(&req).unwrap();
        assert_eq!(
            handle_request(&body, never, empty_playlists),
            Response::Error(ErrorCode::InvalidMessage),
            "extra={extra}"
        );
    }
}

/// DB-NM-13: `list_playlists` アクション（FR-50、仕様決定 CA）。
/// URL を取らず、`{"ok": true, "playlists": [{"id","name"}]}` を直接返す。
/// DB が読めないときは `db_unavailable` にする。
#[test]
fn handle_request_list_playlists() {
    let list = vec![
        PlaylistSummary {
            id: 1,
            name: "あとで見る".to_string(),
        },
        PlaylistSummary {
            id: 9,
            name: "music".to_string(),
        },
    ];
    let res = handle_request(
        br#"{"action":"list_playlists"}"#,
        |_| panic!("開いてはいけない"),
        move || Ok(list.clone()),
    );
    assert_eq!(
        res.to_json(),
        json!({
            "ok": true,
            "playlists": [
                { "id": 1, "name": "あとで見る" },
                { "id": 9, "name": "music" }
            ]
        })
    );
    // DB が開けない・読めない失敗は db_unavailable に写像する
    let res = handle_request(
        br#"{"action":"list_playlists"}"#,
        |_| panic!("開いてはいけない"),
        || Err(io::Error::other("no db")),
    );
    assert_eq!(
        res.to_json(),
        json!({ "ok": false, "error": "db_unavailable" })
    );
    // url を伴わない通常要求は invalid_message のまま（既存挙動の維持）
    let res = handle_request(
        br#"{"action":"queue"}"#,
        |_| panic!("開いてはいけない"),
        empty_playlists,
    );
    assert_eq!(res, Response::Error(ErrorCode::InvalidMessage));
}

/// DB-NM-14: 本体 DB の `playlists` 表の読み取り。
/// `playlist_list` と同じ `ORDER BY sort_order, id` で ID と名前を返す。
/// DB が無い・表が無い場合は `db_unavailable` へ写像される Err を返す。
#[test]
fn read_playlists_reads_playlists_table() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("yt-browser.db");
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute(
            "CREATE TABLE playlists (id INTEGER PRIMARY KEY AUTOINCREMENT,
                    name TEXT NOT NULL, sort_order INTEGER NOT NULL DEFAULT 0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO playlists (name, sort_order) VALUES ('b', 2), ('a', 1), ('c', 1)",
            [],
        )
        .unwrap();
    }
    let got = read_playlists(&path).unwrap();
    // sort_order→id 順： a(1), c(1 の後), b(2)
    assert_eq!(
        got.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
        vec!["a", "c", "b"]
    );
    // 存在しないパスと、表の無い DB は Err
    assert!(read_playlists(&dir.path().join("none.db")).is_err());
    let empty_db = dir.path().join("empty.db");
    rusqlite::Connection::open(&empty_db).unwrap();
    assert!(read_playlists(&empty_db).is_err());
}

/// DB-NM-10: 実際の標準入出力と同じ形の 1 往復。
/// フレーム化した要求を読み、処理し、フレーム化した応答を書く。
#[test]
fn framed_request_to_framed_response() {
    let input = framed(br#"{"url":"https://www.youtube.com/playlist?list=PL1"}"#);
    let body = read_message(&mut input.as_slice()).unwrap();
    let res = handle_request(&body, |_| Ok(()), empty_playlists);
    let mut out = Vec::new();
    write_message(&mut out, &res.to_json()).unwrap();
    let back = read_message(&mut out.as_slice()).unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&back).unwrap(),
        json!({ "ok": true })
    );
}

#[cfg(target_os = "linux")]
/// DB-NM-15: Linux の登録先一覧（FR-88、仕様決定 DM）。
/// Chrome は従来どおり無条件に置き場を返し、Edge・Chromium は
/// ブラウザの設定ディレクトリが既にあるときだけ列挙する暫定解釈を確かめる。
/// 各置き場は `<config_dir>/<設定名>/NativeMessagingHosts/<ホスト名>.json` の形。
#[test]
fn linux_manifest_paths_cover_browsers() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path();
    let manifest_name = format!("{HOST_NAME}.json");
    // ブラウザ設定ディレクトリが何も無い環境では Chrome のみ
    assert_eq!(
        linux_manifest_paths(config),
        vec![config
            .join("google-chrome")
            .join("NativeMessagingHosts")
            .join(&manifest_name)]
    );
    // Edge の設定ディレクトリがあれば Edge も対象になる
    std::fs::create_dir_all(config.join("microsoft-edge")).unwrap();
    let paths = linux_manifest_paths(config);
    assert_eq!(
        paths,
        vec![
            config
                .join("google-chrome")
                .join("NativeMessagingHosts")
                .join(&manifest_name),
            config
                .join("microsoft-edge")
                .join("NativeMessagingHosts")
                .join(&manifest_name),
        ]
    );
    // Chromium も同様に設定ディレクトリの存在で対象になる
    std::fs::create_dir_all(config.join("chromium")).unwrap();
    assert_eq!(linux_manifest_paths(config).len(), 3);
}

#[cfg(windows)]
/// DB-NM-16: Windows の登録先レジストリキー一覧（FR-88、仕様決定 DM）。
/// HKCU 相対パスの `<起点>\NativeMessagingHosts\<ホスト名>` の形で、
/// Chrome・Edge・Chromium の 3 置き場を列挙する。
#[test]
fn windows_registry_subkeys_cover_browsers() {
    assert_eq!(
        windows_registry_subkeys(),
        vec![
            format!(r"Software\Google\Chrome\NativeMessagingHosts\{HOST_NAME}"),
            format!(r"Software\Microsoft\Edge\NativeMessagingHosts\{HOST_NAME}"),
            format!(r"Software\Chromium\NativeMessagingHosts\{HOST_NAME}"),
        ]
    );
}

/// DB-NM-17: スキーム URL 配送の 3 段フォールバック。
/// ローカル IPC → 本体バイナリ直接 spawn → OS 既定ハンドラの順で試行し、
/// 先行段階が成功したら後続段階を呼ばない。
#[test]
fn deliver_scheme_url_fallback_order() {
    let url = "yt-browser://open?url=https%3A%2F%2Fyoutu.be%2FdQw4w9WgXcQ";
    // 1 段目（IPC）が成功すれば spawn / open は呼ばれない
    let res = deliver_scheme_url_with(
        url,
        |u| {
            assert_eq!(u, url);
            Ok(())
        },
        |_| panic!("IPC 成功時は spawn を呼ばない"),
        |_| panic!("IPC 成功時は open を呼ばない"),
    );
    assert!(res.is_ok());

    // 1 段目失敗・2 段目（spawn）成功なら open は呼ばれない
    let res = deliver_scheme_url_with(
        url,
        |_| Err(io::Error::new(io::ErrorKind::ConnectionRefused, "no ipc")),
        |u| {
            assert_eq!(u, url);
            Ok(())
        },
        |_| panic!("spawn 成功時は open を呼ばない"),
    );
    assert!(res.is_ok());

    // 1・2 段目失敗時は 3 段目（open）へフォールバックし、その結果を返す
    let res = deliver_scheme_url_with(
        url,
        |_| Err(io::Error::other("ipc failed")),
        |_| Err(io::Error::other("spawn failed")),
        |u| {
            assert_eq!(u, url);
            Ok(())
        },
    );
    assert!(res.is_ok());

    let err = deliver_scheme_url_with(
        url,
        |_| Err(io::Error::other("ipc failed")),
        |_| Err(io::Error::other("spawn failed")),
        |_| Err(io::Error::other("open failed")),
    )
    .unwrap_err();
    assert_eq!(err.to_string(), "open failed");
}

/// DB-NM-18: ローカル IPC 転送の往復。
/// Windows 名前付きパイプ / Unix ソケット経由で送信した 1 行のスキーム URL が受信できる。
#[tokio::test]
async fn local_ipc_transfers_scheme_url() {
    use tokio::io::{AsyncBufReadExt, BufReader};

    let target = "yt-browser://queue?url=https%3A%2F%2Fyoutu.be%2FdQw4w9WgXcQ";
    #[cfg(windows)]
    {
        use tokio::net::windows::named_pipe::ServerOptions;
        let pipe = format!(r"\\.\pipe\yt-browser-test-nm-ipc-{}", std::process::id());
        let server = ServerOptions::new().create(&pipe).unwrap();
        let pipe_client = pipe.clone();
        let client =
            tokio::task::spawn_blocking(move || send_local_ipc_windows(&pipe_client, target));
        server.connect().await.unwrap();
        let mut reader = BufReader::new(server);
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        client.await.unwrap().unwrap();
        assert_eq!(line.trim(), target);
    }
    #[cfg(unix)]
    {
        let dir = tempfile::tempdir().unwrap();
        let sock = local_ipc_socket_path(dir.path());
        let listener = tokio::net::UnixListener::bind(&sock).unwrap();
        let sock_client = sock.clone();
        let client = tokio::task::spawn_blocking(move || send_local_ipc_unix(&sock_client, target));
        let (stream, _) = listener.accept().await.unwrap();
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        client.await.unwrap().unwrap();
        assert_eq!(line.trim(), target);
    }
}

#[cfg(any(windows, unix))]
#[test]
fn ipc_listener_starts_from_non_tokio_thread() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let handle = rt.handle().clone();

    #[cfg(windows)]
    let (endpoint, pipe_client, _dir) = {
        let pipe = format!(
            r"\\.\pipe\yt-browser-test-nm-listener-{}",
            std::process::id()
        );
        (IpcEndpoint::WindowsPipe(pipe.clone()), pipe, ())
    };
    #[cfg(unix)]
    let (endpoint, sock_client, _dir) = {
        let dir = tempfile::tempdir().unwrap();
        let sock = local_ipc_socket_path(dir.path());
        (IpcEndpoint::UnixSocket(sock.clone()), sock, dir)
    };

    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let (got_tx, got_rx) = std::sync::mpsc::channel();
    let spawner: IpcSpawner = std::sync::Arc::new(move |fut| {
        handle.spawn(fut);
    });
    let on_url: IpcUrlHandler = std::sync::Arc::new(move |url| {
        let _ = got_tx.send(url);
    });

    // Tokio ランタイム外の純粋な別スレッドから同期起動してもリアクタ不在でパニックしないこと
    let start_res = std::thread::spawn(move || {
        start_ipc_listener_with(endpoint, spawner, on_url, Some(ready_tx));
    })
    .join();
    assert!(
        start_res.is_ok(),
        "Tokio 外スレッドからの起動でパニックしない"
    );

    rt.block_on(async {
        ready_rx.await.unwrap().unwrap();
    });

    let target = "yt-browser://queue?url=https%3A%2F%2Fyoutu.be%2FdQw4w9WgXcQ";
    #[cfg(windows)]
    send_local_ipc_windows(&pipe_client, target).unwrap();
    #[cfg(unix)]
    send_local_ipc_unix(&sock_client, target).unwrap();

    let received = got_rx
        .recv_timeout(std::time::Duration::from_secs(2))
        .expect("ローカル IPC リスナー経由で URL を受信できる");
    assert_eq!(received, target);

    rt.shutdown_timeout(std::time::Duration::from_millis(200));
}

#[cfg(unix)]
/// DB-NM-19: Unix ソケットの権限を所有者のみ（`0600`）に制限する。
/// 既定のソケット権限は他ユーザーの接続も許すため、bind 直後に狭める。
#[test]
fn unix_socket_permissions_are_owner_only() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let sock = local_ipc_socket_path(dir.path());
    std::os::unix::net::UnixListener::bind(&sock).unwrap();
    restrict_socket_permissions(&sock).unwrap();
    let mode = std::fs::metadata(&sock).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
}

#[cfg(unix)]
/// DB-NM-20: 終了時クリーンアップが Unix ソケットの残骸を消し、
/// ファイルが無い状態でも正常終了する。
#[test]
fn cleanup_ipc_endpoint_removes_socket() {
    let dir = tempfile::tempdir().unwrap();
    let sock = local_ipc_socket_path(dir.path());
    // 残骸が無い状態でも正常終了する
    cleanup_ipc_endpoint(dir.path());
    std::fs::write(&sock, "x").unwrap();
    cleanup_ipc_endpoint(dir.path());
    assert!(!sock.exists());
}
