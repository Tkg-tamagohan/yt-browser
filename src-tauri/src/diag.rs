//! 診断情報の一括エクスポート（FR-82、仕様決定 DG、設計書 §4.21）。
//!
//! アプリログ・設定・環境情報を 1 つの zip にまとめて書き出す。
//! サポート目的で外部へ共有される前提のため、署名 URL やトークン、
//! 絶対パス内のユーザー名などの秘匿情報は zip へ入れる前にマスクする
//! （規則は `Masker` のドキュメント）。
//!
//! mpv ログ（`mpv-*.log`）は署名入りストリーム URL が mpv 自身の出力として
//! 記録され得て内容を制御できないため、設計書 §4.9 の「外部へ共有しない」
//! 前提どおり zip の対象外とする（暫定解釈）。

use std::io::Write;
use std::path::{Path, PathBuf};

use regex::Regex;
use thiserror::Error;

use crate::db::Db;
use crate::yt::YtDlpResolver;

/// zip に収めるアプリログの最大件数（更新時刻の新しい順）。
/// ログは日次ローテーションなので直近 10 日分が目安。
const LOG_KEEP: usize = 10;
/// 1 ファイルあたりの取り込み上限。超過時は末尾側だけ残す
/// （直近の行ほど障害再現時の診断価値が高いため）。
const LOG_MAX_BYTES: u64 = 2 * 1024 * 1024;

/// アプリログのファイル名プレフィックス（lib.rs の init_tracing と同一値）。
const APP_LOG_PREFIX: &str = "yt-browser.log";

#[derive(Debug, Error)]
pub enum DiagError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("db: {0}")]
    Db(#[from] crate::db::DbError),
    #[error("zip: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

/// エクスポートに必要な外部参照。コマンド層が AppHandle から組み立てる。
/// テストでは一時ディレクトリやインメモリ DB で代替できる。
pub struct DiagContext<'a> {
    pub db: &'a Db,
    pub ytdlp: &'a YtDlpResolver,
    /// アプリログのディレクトリ（Tauri app_log_dir）。無ければログ節は空になる
    pub log_dir: Option<PathBuf>,
    /// アプリデータのディレクトリ（Tauri app_data_dir）
    pub data_dir: Option<PathBuf>,
    /// 設定ディレクトリ（Tauri config_dir）
    pub config_dir: Option<PathBuf>,
    /// ホームディレクトリ（Tauri home_dir）
    pub home_dir: Option<PathBuf>,
    /// 表示環境の分類（mpv::windowing_system() の値）
    pub windowing: &'static str,
}

/// zip に入れる 1 エントリ。
struct ZipEntry {
    name: String,
    body: Vec<u8>,
}

/// `dest` へ診断 zip を書き出す（FR-82）。
/// 既存ファイルは上書きする（保存ダイアログで選ばれたパス）。
pub async fn export_bundle(dest: &Path, ctx: &DiagContext<'_>) -> Result<(), DiagError> {
    let masker = Masker::from_context(ctx);

    let mut entries = vec![
        ZipEntry {
            name: "README.txt".to_string(),
            body: README.as_bytes().to_vec(),
        },
        ZipEntry {
            name: "environment.json".to_string(),
            body: environment_json(ctx, &masker).await?.into_bytes(),
        },
        ZipEntry {
            name: "settings.json".to_string(),
            body: settings_json(ctx, &masker)?.into_bytes(),
        },
    ];
    for (name, bytes) in collect_logs(ctx.log_dir.as_deref()) {
        entries.push(ZipEntry {
            name: format!("logs/{name}"),
            body: masker.mask_bytes(&bytes).into_bytes(),
        });
    }

    write_zip(dest, &entries)?;
    Ok(())
}

/// zip 同梱の説明文。受け取った側が中身とマスクの有無を判別できるようにする。
const README: &str = "yt-browser 診断情報（FR-82）\n\
\n\
environment.json … OS・バージョン・依存コマンドの検出状況\n\
settings.json … アプリ設定（秘匿系キーの値はマスク済み）\n\
logs/ … アプリログ（最新 10 件・各 2MiB まで・秘匿情報マスク済み）\n\
\n\
署名 URL の署名・トークン類、絶対パス内のユーザー名は *** に置き換えています。\n\
mpv ログは署名入り URL を含み得るため同梱していません。\n";

/// マスク対象とする名前の一覧（URL クエリ名・JSON キー名で共用、大小文字無視）。
/// 署名 URL の署名系（sig/lsig/signature/pot）、認証系（token/auth/key/secret/
/// password 系）、セッション系（session/sid/visitor_data/continuation）、
/// および発信元の秘匿（ip/ipbits）を対象とする。動画 ID・itag・再生系の
/// 値は診断に要るため残す（暫定解釈: 対象名の列挙式）。
const SECRET_NAMES: &[&str] = &[
    "sig",
    "lsig",
    "signature",
    "pot",
    "po_token",
    "token",
    "access_token",
    "refresh_token",
    "id_token",
    "auth",
    "authorization",
    "key",
    "api_key",
    "apikey",
    "secret",
    "client_secret",
    "password",
    "passwd",
    "session",
    "session_id",
    "sid",
    "visitor_data",
    "continuation",
    "bearer",
    "ip",
    "ipbits",
    "cookie",
    "cookies",
];

/// 設定キー名がこれらの断片を含むとき、その値全体を秘匿として "***" に
/// 置き換える（値の中身を解析しない。`ytdlp.po_token` など）。
const SECRET_SETTING_FRAGMENTS: &[&str] = &[
    "token",
    "secret",
    "password",
    "passwd",
    "credential",
    "cookie",
];

/// 秘匿情報のマスク処理。置換規則をコンストラクタで確定し、テキストへ
/// 一括適用する。
///
/// 規則（設計書 §4.21）:
/// 1. 既知の絶対ディレクトリ（ログ・データ・設定・ホーム）を
///    `<LOG_DIR>` などのプレースホルダへ置き換える
/// 2. URL クエリで `SECRET_NAMES` に列挙する名前の値を `***` に置き換える
///    （`sig=ABC&x=1` → `sig=***&x=1`）
/// 3. JSON 文字列値で `SECRET_NAMES` に列挙するキーの値を `"***"` に置き換える
/// 4. `Authorization: Bearer/Basic <値>`、`Cookie: <値>` の値を `***` に置き換える
/// 5. 上記に残った汎用のユーザーディレクトリ（`C:\Users\<名>`・`/home/<名>`・
///    `/Users/<名>`）のユーザー名を `***` に置き換える
pub(crate) struct Masker {
    /// (検索文字列, 置換後)。長い順に適用し、短い前方一致で先に潰れないようにする
    known_paths: Vec<(String, String)>,
    re_query: Regex,
    re_json: Regex,
    re_header: Regex,
    re_home_win: Regex,
    re_home_unix: Regex,
    re_home_mac: Regex,
}

impl Masker {
    /// `known` は `(ディレクトリ, プレースホルダ)` の並び。
    /// パスの区切りは `\` `/` 双方の表記を登録する（ログ内の表記ゆれに備える）。
    pub(crate) fn new(known: &[(PathBuf, &str)]) -> Self {
        let names = SECRET_NAMES
            .iter()
            .map(|n| regex::escape(n))
            .collect::<Vec<_>>()
            .join("|");
        let mut known_paths: Vec<(String, String)> = Vec::new();
        for (dir, placeholder) in known {
            let disp = dir.display().to_string();
            if disp.is_empty() {
                continue;
            }
            known_paths.push((disp.clone(), (*placeholder).to_string()));
            // ログや設定値では区切りが正反対に書かれうるため両方登録する
            let swapped: String = disp
                .chars()
                .map(|c| match c {
                    '\\' => '/',
                    '/' => '\\',
                    c => c,
                })
                .collect();
            if swapped != disp {
                known_paths.push((swapped, (*placeholder).to_string()));
            }
        }
        known_paths.sort_by_key(|a| std::cmp::Reverse(a.0.len()));
        Self {
            known_paths,
            // `?name=value` / `&name=value` の value 部（区切り・引用・空白まで）
            re_query: Regex::new(&format!(
                r#"(?i)([?&](?:{names})=)[^&\s"')>\]]+"#
            ))
            .expect("re_query"),
            // `"name": "value"` / `"name": value` の value 部（JSON 片想定。
            // エスケープ込みの文字列は `"..."` の最短一致で粗く潰す）
            re_json: Regex::new(&format!(
                r#"(?i)("(?:{names})"\s*:\s*)("([^"\\]|\\.)*"|[^,\}}\]\s]+)"#
            ))
            .expect("re_json"),
            // `Authorization: Bearer xxx` / `Authorization: Basic xxx`、
            // `Cookie:` / `Set-Cookie:` ヘッダ値（行末まで）
            re_header: Regex::new(
                r"(?im)^(\s*(?:authorization|cookie|set-cookie)\s*[:=]\s*(?:bearer|basic)?\s*)\S[^\r\n]*",
            )
            .expect("re_header"),
            // Windows ユーザーディレクトリのユーザー名
            re_home_win: Regex::new(r#"(?i)([a-z]:[\\/]users[\\/])[^\\/:*?"<>|\s]+"#)
                .expect("re_home_win"),
            // Linux の /home/<user>
            re_home_unix: Regex::new(r#"(/home/)[^/\s"':]+"#).expect("re_home_unix"),
            // macOS の /Users/<user>
            re_home_mac: Regex::new(r#"(/Users/)[^/\s"':]+"#).expect("re_home_mac"),
        }
    }

    /// `DiagContext` の既知ディレクトリから Masker を組み立てる。
    fn from_context(ctx: &DiagContext<'_>) -> Self {
        let mut known: Vec<(PathBuf, &str)> = Vec::new();
        if let Some(d) = &ctx.log_dir {
            known.push((d.clone(), "<LOG_DIR>"));
        }
        if let Some(d) = &ctx.data_dir {
            known.push((d.clone(), "<DATA_DIR>"));
        }
        if let Some(d) = &ctx.config_dir {
            known.push((d.clone(), "<CONFIG_DIR>"));
        }
        if let Some(d) = &ctx.home_dir {
            known.push((d.clone(), "<HOME>"));
        }
        Self::new(&known)
    }

    /// UTF-8 でない可能性のあるバイト列を損失変換してからマスクする。
    pub(crate) fn mask_bytes(&self, bytes: &[u8]) -> String {
        self.mask(&String::from_utf8_lossy(bytes))
    }

    /// マスク規則をテキストへ適用する。
    pub(crate) fn mask(&self, text: &str) -> String {
        let mut out = text.to_string();
        // 1. 既知ディレクトリ（先に潰すと汎用ユーザーディレクトリ規則より優先される）
        for (from, to) in &self.known_paths {
            out = out.replace(from.as_str(), to);
        }
        // 2. URL クエリの秘匿名
        out = self.re_query.replace_all(&out, "${1}***").into_owned();
        // 3. JSON 値の秘匿名
        out = self.re_json.replace_all(&out, "${1}\"***\"").into_owned();
        // 4. Authorization / Cookie ヘッダ
        out = self.re_header.replace_all(&out, "${1}***").into_owned();
        // 5. 汎用ユーザーディレクトリ
        out = self.re_home_win.replace_all(&out, "${1}***").into_owned();
        out = self.re_home_unix.replace_all(&out, "${1}***").into_owned();
        out = self.re_home_mac.replace_all(&out, "${1}***").into_owned();
        out
    }
}

/// 設定キーが秘匿系（token/secret/password 等の断片を含む）か。
fn is_secret_setting(key: &str) -> bool {
    let k = key.to_ascii_lowercase();
    SECRET_SETTING_FRAGMENTS.iter().any(|f| k.contains(f))
}

/// `settings.json` の中身。`{キー: 値}` の JSON オブジェクトで、
/// 秘匿系キーの値は `"***"`、それ以外はテキストマスクを通した値。
fn settings_json(ctx: &DiagContext<'_>, masker: &Masker) -> Result<String, DiagError> {
    let settings = ctx.db.settings_list()?;
    let map: serde_json::Map<String, serde_json::Value> = settings
        .into_iter()
        .map(|(key, value)| {
            let v = if is_secret_setting(&key) {
                "***".to_string()
            } else {
                masker.mask(&value)
            };
            (key, serde_json::Value::String(v))
        })
        .collect();
    Ok(serde_json::to_string_pretty(&serde_json::Value::Object(
        map,
    ))?)
}

/// `environment.json` の中身。OS・バージョン・依存コマンドの検出状況。
/// パス類はすべてマスク済みで返す。
async fn environment_json(ctx: &DiagContext<'_>, masker: &Masker) -> Result<String, DiagError> {
    // 依存コマンドの検出は FR-74 の deps_status と同じ経路を使い、
    // 解決パスだけ診断出力用にマスクする（検出本体は再実装しない）
    let deps: Vec<crate::model::DepStatus> = crate::deps::collect_deps_status(ctx.db, ctx.ytdlp)
        .await
        .into_iter()
        .map(|mut d| {
            d.path = d.path.map(|p| masker.mask(&p));
            d
        })
        .collect();

    let generated_at = ctx.db.local_timestamp()?;
    let schema_version = ctx.db.schema_version()?;
    let env = serde_json::json!({
        "app": {
            "name": "yt-browser",
            "version": env!("CARGO_PKG_VERSION"),
            "debug": cfg!(debug_assertions),
            "tauriVersion": tauri::VERSION,
        },
        "os": {
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            "family": std::env::consts::FAMILY,
        },
        "windowingSystem": ctx.windowing,
        "db": { "schemaVersion": schema_version },
        "dependencies": deps,
        "paths": {
            "logDir": ctx.log_dir.as_ref().map(|p| masker.mask(&p.display().to_string())),
            "dataDir": ctx.data_dir.as_ref().map(|p| masker.mask(&p.display().to_string())),
            "configDir": ctx.config_dir.as_ref().map(|p| masker.mask(&p.display().to_string())),
        },
        "generatedAt": generated_at,
    });
    Ok(serde_json::to_string_pretty(&env)?)
}

/// `log_dir` 内のアプリログを新しい順に最大 `LOG_KEEP` 件集める。
/// 戻り値は `(ファイル名, 内容)` の並び。内容は末尾側 `LOG_MAX_BYTES` に
/// 収め、切り詰めた場合は先頭に省略マーカー行を付ける。
/// ディレクトリが無い・読めない場合は空を返す（ログ無しの zip も正常）。
fn collect_logs(log_dir: Option<&Path>) -> Vec<(String, Vec<u8>)> {
    let Some(dir) = log_dir else {
        return Vec::new();
    };
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<(String, std::time::SystemTime)> = rd
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .filter_map(|e| {
            let name = e.file_name().to_str()?.to_string();
            if !name.starts_with(APP_LOG_PREFIX) {
                return None;
            }
            let mtime = e.metadata().and_then(|m| m.modified()).ok()?;
            Some((name, mtime))
        })
        .collect();
    files.sort_by_key(|f| std::cmp::Reverse(f.1));
    files.truncate(LOG_KEEP);
    files
        .into_iter()
        .filter_map(|(name, _)| {
            let path = dir.join(&name);
            let bytes = read_tail(&path, LOG_MAX_BYTES).ok()?;
            Some((name, bytes))
        })
        .collect()
}

/// ファイルの末尾側を最大 `limit` バイト読む。超過時は省略マーカー行を
/// 先頭に付けて返す。
fn read_tail(path: &Path, limit: u64) -> std::io::Result<Vec<u8>> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path)?;
    let len = f.metadata()?.len();
    if len <= limit {
        let mut buf = Vec::with_capacity(len as usize);
        f.read_to_end(&mut buf)?;
        return Ok(buf);
    }
    f.seek(SeekFrom::End(-(limit as i64)))?;
    let mut buf = Vec::with_capacity(limit as usize);
    f.read_to_end(&mut buf)?;
    let marker = format!("# ...（先頭 {} バイトは省略）...\n", len - limit);
    let mut out = marker.into_bytes();
    out.append(&mut buf);
    Ok(out)
}

/// エントリ列を zip ファイルとして `dest` へ書き出す。
fn write_zip(dest: &Path, entries: &[ZipEntry]) -> Result<(), DiagError> {
    let file = std::fs::File::create(dest)?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for e in entries {
        zip.start_file(&e.name, options)?;
        zip.write_all(&e.body)?;
    }
    zip.finish()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use zip::read::ZipArchive;

    fn masker_with(paths: &[(&str, &str)]) -> Masker {
        let known: Vec<(PathBuf, &str)> =
            paths.iter().map(|(p, h)| (PathBuf::from(p), *h)).collect();
        Masker::new(&known)
    }

    /// DB-DG-01: URL クエリの署名系・セッション系パラメータが `***` に
    /// 置き換わり、対象外のパラメータやホスト部分は残る（FR-82）。
    #[test]
    fn mask_query_params() {
        let m = masker_with(&[]);
        let input = "https://r1---sn-abc.googlevideo.com/videoplayback?expire=1&ei=x&ip=1.2.3.4&id=o-abc&itag=18&sig=SECRET&lsig=S2&pot=PT&source=youtube&visitor_data=V&continuation=C";
        let out = m.mask(input);
        for k in ["ip", "sig", "lsig", "pot", "visitor_data", "continuation"] {
            assert!(
                out.contains(&format!("{k}=***")),
                "{k} がマスクされる: {out}"
            );
        }
        for keep in ["expire=1", "ei=x", "id=o-abc", "itag=18", "source=youtube"] {
            assert!(out.contains(keep), "{keep} は残る: {out}");
        }
        assert!(!out.contains("SECRET"));
    }

    /// DB-DG-02: JSON 片の秘匿キー値がマスクされる（大小文字も対象）。
    #[test]
    fn mask_json_values() {
        let m = masker_with(&[]);
        let input = r#"{"token": "abc123", "PO_TOKEN": "xyz", "title": "動画", "nested": {"password": "p"}}"#;
        let out = m.mask(input);
        assert!(out.contains(r#""token": "***""#), "{out}");
        assert!(out.contains(r#""PO_TOKEN": "***""#), "{out}");
        assert!(out.contains(r#""password": "***""#), "{out}");
        assert!(out.contains(r#""title": "動画""#), "{out}");
        assert!(!out.contains("abc123"));
        assert!(!out.contains("xyz"));
    }

    /// DB-DG-03: Authorization / Cookie ヘッダ値が行末までマスクされる。
    #[test]
    fn mask_auth_headers() {
        let m = masker_with(&[]);
        let input = "GET /x\nAuthorization: Bearer s3cr3ttoken\nCookie: SID=abc; other=1\nuser-agent: yt-browser";
        let out = m.mask(input);
        assert!(out.contains("Authorization: Bearer ***"), "{out}");
        assert!(out.contains("Cookie: ***"), "{out}");
        assert!(out.contains("user-agent: yt-browser"), "{out}");
        assert!(!out.contains("s3cr3ttoken"));
        assert!(!out.contains("SID=abc"));
    }

    /// DB-DG-04: 既知ディレクトリはプレースホルダへ、それ以外の
    /// ユーザーディレクトリはユーザー名のみ `***` へ置き換わる。
    #[test]
    fn mask_user_paths() {
        let m = masker_with(&[
            ("/home/alice/.local/share/app/logs", "<LOG_DIR>"),
            ("/home/alice", "<HOME>"),
        ]);
        // 既知ディレクトリは長い方（logs）が優先される
        let out = m.mask("log at /home/alice/.local/share/app/logs/yt-browser.log end");
        assert!(out.contains("<LOG_DIR>/yt-browser.log"), "{out}");
        // 既知ディレクトリ外の同一ホームは <HOME> へ
        let out = m.mask("cfg=/home/alice/.config/x");
        assert!(out.contains("<HOME>/.config/x"), "{out}");
        // 無関係なユーザー名は汎用規則でユーザー名だけ潰れる
        let out = m.mask("p=/home/bob/bin/yt-dlp");
        assert!(out.contains("/home/***/bin/yt-dlp"), "{out}");
        let out = m.mask(r"p=C:\Users\carol\bin\mpv.exe");
        assert!(out.contains(r"C:\Users\***\bin\mpv.exe"), "{out}");
        let out = m.mask("p=/Users/dave/tool");
        assert!(out.contains("/Users/***/tool"), "{out}");
    }

    /// DB-DG-05: 設定キー名の秘匿判定。token/secret/password 等の断片を
    /// 含むキーは値全体を潰す対象。
    #[test]
    fn secret_setting_key_detection() {
        for k in [
            "ytdlp.po_token",
            "api.token",
            "x.secret",
            "x.password",
            "x.passwd",
            "x.credential",
            "x.cookie",
        ] {
            assert!(is_secret_setting(k), "{k}");
        }
        for k in [
            "quality.format",
            "feed.notify_new",
            "window.screen",
            "ytdlp.path",
            "ui.language",
        ] {
            assert!(!is_secret_setting(k), "{k}");
        }
    }

    /// DB-DG-06: settings.json は秘匿系キーの値を "***" に、通常キーの
    /// 値中のユーザーパスをマスクして出力する。
    #[test]
    fn settings_json_masks() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::connect(&dir.path().join("t.db")).unwrap();
        db.setting_set("ytdlp.po_token", "SECRET_TOKEN").unwrap();
        db.setting_set("ytdlp.path", "/home/alice/bin/yt-dlp")
            .unwrap();
        db.setting_set("quality.format", "best").unwrap();
        let resolver = YtDlpResolver::new(None);
        let ctx = DiagContext {
            db: &db,
            ytdlp: &resolver,
            log_dir: None,
            data_dir: None,
            config_dir: None,
            home_dir: Some(PathBuf::from("/home/alice")),
            windowing: "other",
        };
        let m = Masker::from_context(&ctx);
        let json = settings_json(&ctx, &m).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["ytdlp.po_token"], "***");
        assert_eq!(v["ytdlp.path"], "<HOME>/bin/yt-dlp");
        assert_eq!(v["quality.format"], "best");
    }

    /// DB-DG-07: ログ収集は `yt-browser.log` 前置のファイルだけを新しい順で
    /// 最大件数まで拾い、mpv ログなどは除く。
    #[test]
    fn collect_logs_filters_and_orders() {
        let dir = tempfile::tempdir().unwrap();
        // 古い順に 3 件 + 対象外 2 件を作る
        for (i, name) in [
            "yt-browser.log.2026-01-01",
            "yt-browser.log.2026-01-02",
            "yt-browser.log",
        ]
        .iter()
        .enumerate()
        {
            let p = dir.path().join(name);
            std::fs::write(&p, format!("log{i}")).unwrap();
            // 更新時刻を順にずらす
            let f = std::fs::File::options().write(true).open(&p).unwrap();
            f.set_modified(std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(100 + i as u64))
                .unwrap();
        }
        std::fs::write(dir.path().join("mpv-1-1-1.log"), "mpv").unwrap();
        std::fs::write(dir.path().join("other.txt"), "x").unwrap();

        let logs = collect_logs(Some(dir.path()));
        assert_eq!(logs.len(), 3);
        assert_eq!(logs[0].0, "yt-browser.log");
        assert_eq!(logs[1].0, "yt-browser.log.2026-01-02");
        assert_eq!(logs[2].0, "yt-browser.log.2026-01-01");
        assert!(logs.iter().all(|(_, b)| b.starts_with(b"log")));
        // 対象外は拾わない
        assert!(!logs.iter().any(|(n, _)| n.starts_with("mpv-")));
        // ログディレクトリなしは空
        assert!(collect_logs(None).is_empty());
    }

    /// DB-DG-08: 上限超過のログは末尾側だけ残り省略マーカーが付く。
    #[test]
    fn read_tail_truncates() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("big.log");
        let body = "0123456789".repeat(1000);
        std::fs::write(&p, &body).unwrap();
        let out = read_tail(&p, 100).unwrap();
        let text = String::from_utf8_lossy(&out);
        assert!(text.contains("省略"), "{text}");
        assert!(text.ends_with(&"0123456789".repeat(10)));
        let out = read_tail(&p, u64::MAX).unwrap();
        assert_eq!(out, body.as_bytes());
    }

    /// DB-DG-09: export_bundle が zip を生成し、ログ・設定・環境情報と
    /// マスク済みの内容が入る（FR-82 受け入れ条件）。
    #[tokio::test]
    async fn export_bundle_writes_masked_zip() {
        let dir = tempfile::tempdir().unwrap();
        let log_dir = dir.path().join("logs");
        std::fs::create_dir_all(&log_dir).unwrap();
        std::fs::write(
            log_dir.join("yt-browser.log"),
            "open https://x/v?sig=SECRET&itag=18 at /home/alice/x\n",
        )
        .unwrap();
        // mpv ログは同梱対象外（設計書 §4.9 の非共有前提）
        std::fs::write(log_dir.join("mpv-1-1-1.log"), "mpv sig=KEEP").unwrap();

        let db = Db::connect(&dir.path().join("t.db")).unwrap();
        db.setting_set("ytdlp.po_token", "SECRET_TOKEN").unwrap();

        let resolver = YtDlpResolver::new(None);
        let ctx = DiagContext {
            db: &db,
            ytdlp: &resolver,
            log_dir: Some(log_dir),
            data_dir: Some(dir.path().join("data")),
            config_dir: Some(dir.path().join("config")),
            home_dir: Some(PathBuf::from("/home/alice")),
            windowing: "x11",
        };
        let dest = dir.path().join("diag.zip");
        export_bundle(&dest, &ctx).await.unwrap();

        let file = std::fs::File::open(&dest).unwrap();
        let mut zip = ZipArchive::new(file).unwrap();
        let names: Vec<String> = (0..zip.len())
            .map(|i| zip.by_index(i).unwrap().name().to_string())
            .collect();
        assert!(names.contains(&"environment.json".to_string()), "{names:?}");
        assert!(names.contains(&"settings.json".to_string()), "{names:?}");
        assert!(
            names.contains(&"logs/yt-browser.log".to_string()),
            "{names:?}"
        );
        // mpv ログは同梱しない
        assert!(!names.iter().any(|n| n.contains("mpv-")), "{names:?}");

        use std::io::Read;
        let mut log = String::new();
        zip.by_name("logs/yt-browser.log")
            .unwrap()
            .read_to_string(&mut log)
            .unwrap();
        assert!(log.contains("sig=***"), "{log}");
        assert!(!log.contains("SECRET"));
        assert!(log.contains("<HOME>/x"), "{log}");

        let mut env = String::new();
        zip.by_name("environment.json")
            .unwrap()
            .read_to_string(&mut env)
            .unwrap();
        let env: serde_json::Value = serde_json::from_str(&env).unwrap();
        assert_eq!(env["os"]["os"], std::env::consts::OS);
        assert_eq!(env["windowingSystem"], "x11");
        assert!(env["db"]["schemaVersion"].as_u64().unwrap() >= 21);
        assert!(env["dependencies"].as_array().unwrap().len() == 3);

        let mut settings = String::new();
        zip.by_name("settings.json")
            .unwrap()
            .read_to_string(&mut settings)
            .unwrap();
        let s: serde_json::Value = serde_json::from_str(&settings).unwrap();
        assert_eq!(s["ytdlp.po_token"], "***");
    }
}
