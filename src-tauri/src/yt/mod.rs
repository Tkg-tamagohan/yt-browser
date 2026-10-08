//! yt-dlp 子プロセスの呼び出し（仕様決定 A / 技術方針 P、設計書 §5）。
//! Phase 1 ではパス解決・バージョン確認・`yt-dlp -U` 更新のみを提供する。
//! Phase 5 で検索（ytsearch + flat-playlist）を追加した。
//! バックエンドの差し替え面はこのモジュール内の関数群が担う
//! （トレイト抽象化は導入を遅延。decision-records「Phase 5 で確定した事項」参照）。

use std::time::Duration;

use thiserror::Error;
use tokio::process::Command;

use crate::db::Db;

/// yt-dlp 呼び出しのタイムアウト。`-U` や `-J` はネットワーク依存なので長めに取る。
const PROCESS_TIMEOUT: Duration = Duration::from_secs(60);

/// 設定キー: yt-dlp のパスをユーザーが明示する場合の上書き設定。
pub const SETTING_YTDLP_PATH: &str = "ytdlp.path";
/// 設定キー: 画質式（`ytdl-format`）のユーザー上書き。
pub const SETTING_QUALITY_FORMAT: &str = "quality.format";

#[derive(Debug, Error)]
pub enum YtError {
    #[error("yt-dlp の起動に失敗: {0}")]
    Spawn(#[source] std::io::Error),
    #[error("yt-dlp が終了コード {code} で失敗: {stderr}")]
    Exit { code: i32, stderr: String },
    #[error("yt-dlp が時間内に応答しなかった")]
    Timeout,
    #[error("yt-dlp が見つからない。インストールするか設定 `ytdlp.path` でパスを指定してください")]
    NotFound,
    #[error("yt-dlp の出力の JSON 解析に失敗: {0}")]
    Json(#[from] serde_json::Error),
    #[error("yt-dlp の出力に channel_id が含まれていない")]
    NoChannelId,
}

/// yt-dlp のバイナリパス解決器。
///
/// 解決順（Phase 1 で確定。決定記録「Phase 1 で確定した事項」参照）:
/// 1. `settings.ytdlp_path`（`ytdlp.path` のユーザー指定）
/// 2. 同梱リソースの `yt-dlp`（配布フェーズで bundle に同梱する想定）
/// 3. PATH 上の `yt-dlp`（システムインストールのフォールバック）
#[derive(Clone)]
pub struct YtDlpResolver {
    /// 同梱リソース探索用のディレクトリ。Tauri の resource_dir を起動時に確定して保持する。
    resource_dir: Option<std::path::PathBuf>,
}

impl YtDlpResolver {
    pub fn new(resource_dir: Option<std::path::PathBuf>) -> Self {
        Self { resource_dir }
    }

    /// 利用する yt-dlp を解決する。見つからなければ None。
    /// 戻り値は実行可能なパスまたは `yt-dlp`（PATH 解決名）。
    pub async fn resolve(&self, db: &Db) -> Option<String> {
        // 1. 設定による明示パス
        if let Ok(Some(path)) = db.setting_get(SETTING_YTDLP_PATH) {
            if !path.is_empty() {
                return Some(path);
            }
        }
        // 2. 同梱リソース
        if let Some(dir) = &self.resource_dir {
            let name = if cfg!(windows) {
                "yt-dlp.exe"
            } else {
                "yt-dlp"
            };
            let bundled = dir.join(name);
            if bundled.exists() {
                return Some(bundled.to_string_lossy().into_owned());
            }
        }
        // 3. PATH
        if which_exists("yt-dlp").await {
            return Some("yt-dlp".to_string());
        }
        None
    }

    /// `ytdlp_status` 用に解決パスとバージョンを得る。
    pub async fn status(&self, db: &Db) -> (Option<String>, Option<String>) {
        let path = self.resolve(db).await;
        let version = match &path {
            Some(p) => version(p).await.ok(),
            None => None,
        };
        (path, version)
    }
}

/// `yt-dlp --version` を実行してバージョン文字列を返す。
pub async fn version(path: &str) -> Result<String, YtError> {
    let out = run_with_timeout(Command::new(path).arg("--version")).await?;
    if !out.status.success() {
        return Err(YtError::Exit {
            code: out.status.code().unwrap_or(-1),
            stderr: combined_output(&out),
        });
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// `yt-dlp -U`（セルフアップデート）を実行して出力末尾を返す。
/// システム管理のパスでは権限不足で失敗し得る。決定記録どおり stdout/stderr の
/// 両方の出力を返し、失敗時もその出力をエラーメッセージに含める。
pub async fn update(path: &str) -> Result<String, YtError> {
    let out = run_with_timeout(Command::new(path).arg("-U")).await?;
    if !out.status.success() {
        return Err(YtError::Exit {
            code: out.status.code().unwrap_or(-1),
            stderr: combined_output(&out),
        });
    }
    Ok(combined_output(&out))
}

/// stdout + stderr をまとめて返す。`-U` は進捗を stdout に出すので失敗時も
/// stdout を捨てない（決定記録「出力をそのまま UI に返す」）。
/// 各ストリームを別々に 500 文字へ切り詰め、長い stdout に stderr の診断が
/// 潰されないようにする。
fn combined_output(out: &std::process::Output) -> String {
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let mut s: String = stdout.trim().chars().take(500).collect();
    let err: String = stderr.trim().chars().take(500).collect();
    if !err.is_empty() {
        if !s.is_empty() {
            s.push('\n');
        }
        s.push_str(&err);
    }
    s
}

/// チャンネル URL（@handle 等）からチャンネル ID を解決する。
/// `--flat-playlist --playlist-end 1 --dump-single-json` で最小限の取得に留める。
/// 返される JSON の `channel_id` を読む（プレイリスト型応答を想定）。
pub async fn channel_id(path: &str, url: &str) -> Result<String, YtError> {
    let out = run_with_timeout(
        Command::new(path)
            .arg(url)
            .arg("--flat-playlist")
            .arg("--playlist-end")
            .arg("1")
            .arg("--dump-single-json"),
    )
    .await?;
    if !out.status.success() {
        return Err(YtError::Exit {
            code: out.status.code().unwrap_or(-1),
            stderr: combined_output(&out),
        });
    }
    let v: serde_json::Value = serde_json::from_slice(&out.stdout)?;
    v.get("channel_id")
        .and_then(|c| c.as_str())
        .map(|s| s.to_string())
        .ok_or(YtError::NoChannelId)
}

/// 再生中動画のチャンネル参照候補をメタから取る（FR-12、仕様決定 U）。
/// `channel_id` → `uploader_id` → `channel_url` の順に非空の値を
/// `subscribe_channel` が受理できる形に正規化して返す。
/// 動画視聴ページは単一項目のプレイリストとして `--flat-playlist` で軽く取る。
pub async fn video_channel_ref(path: &str, video_id: &str) -> Result<Vec<String>, YtError> {
    let url = format!("https://www.youtube.com/watch?v={video_id}");
    let out = run_with_timeout(
        Command::new(path)
            .arg(&url)
            .arg("--flat-playlist")
            .arg("--dump-single-json"),
    )
    .await?;
    if !out.status.success() {
        return Err(YtError::Exit {
            code: out.status.code().unwrap_or(-1),
            stderr: combined_output(&out),
        });
    }
    let v: serde_json::Value = serde_json::from_slice(&out.stdout)?;
    // 単一動画ではトップレベルにメタが来る。プレイリスト形で返る場合は
    // entries[0] を同じ優先順で見る
    let cands = channel_ref_candidates(&v);
    if !cands.is_empty() {
        return Ok(cands);
    }
    if let Some(first) = v.get("entries").and_then(|e| e.get(0)) {
        let cands = channel_ref_candidates(first);
        if !cands.is_empty() {
            return Ok(cands);
        }
    }
    Err(YtError::NoChannelId)
}

/// メタ JSON から `channel_id` → `uploader_id` → `channel_url` の順に
/// 購読解決へ渡せる候補を拾う。`uploader_id` が `@` なしで来た場合は
/// `@` を補う（暫定: ハンドルとみなす）。yt-dlp が文字列 "None" を
/// 出力することがあるため空扱いする（実測確認済み、ytsearch と同じ罠）。
fn channel_ref_candidates(v: &serde_json::Value) -> Vec<String> {
    let get = |key: &str| -> Option<String> {
        v.get(key)
            .and_then(|x| x.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty() && *s != "None")
            .map(str::to_string)
    };
    let mut out = Vec::new();
    if let Some(cid) = get("channel_id") {
        out.push(cid);
    }
    if let Some(up) = get("uploader_id") {
        // UC 形の uploader_id はチャンネル ID として扱う（yt-dlp が
        // uploader_id に UC ID を出すことがある。`@UC...` と解釈すると
        // 存在しないハンドルへ解決してしまう）
        out.push(if is_uc_channel_id(&up) || up.starts_with('@') {
            up
        } else {
            format!("@{up}")
        });
    }
    if let Some(url) = get("channel_url") {
        out.push(url);
    }
    out
}

/// PATH 解決可否。`--version` が起動できれば存在するとみなす。
/// 応答しない実行ファイルに引きずられないよう 10 秒で打ち切る。
async fn which_exists(name: &str) -> bool {
    let mut cmd = Command::new(name);
    cmd.arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(crate::CREATE_NO_WINDOW);
    match tokio::time::timeout(Duration::from_secs(10), cmd.status()).await {
        Ok(res) => res.map(|s| s.success()).unwrap_or(false),
        Err(_) => false,
    }
}

/// `cmd.output()` をタイムアウト付きで実行する。
/// `kill_on_drop(true)` によりタイムアウト時に子プロセスも確実に終了する
/// （ドロップだけではプロセスが残る）。
async fn run_with_timeout(cmd: &mut Command) -> Result<std::process::Output, YtError> {
    cmd.kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(crate::CREATE_NO_WINDOW);
    match tokio::time::timeout(PROCESS_TIMEOUT, cmd.output()).await {
        Ok(res) => res.map_err(YtError::Spawn),
        Err(_) => Err(YtError::Timeout),
    }
}

/// `yt-dlp "ytsearch<N>:<query>" --dump-json --flat-playlist` による検索（設計書 §5）。
/// flat エントリはチャンネル一覧表示と同等の軽量メタだけを持つ。
/// `limit` は取得する最大件数。
pub async fn search(
    path: &str,
    query: &str,
    limit: u32,
) -> Result<Vec<crate::model::SearchResult>, YtError> {
    let out = run_with_timeout(
        Command::new(path)
            .arg(format!("ytsearch{limit}:{query}"))
            .arg("--dump-json")
            .arg("--flat-playlist")
            .arg("--no-warnings"),
    )
    .await?;
    if !out.status.success() {
        return Err(YtError::Exit {
            code: out.status.code().unwrap_or(-1),
            stderr: combined_output(&out),
        });
    }
    Ok(parse_search_jsonl(&out.stdout))
}

/// 動画 ID からサムネイル URL を組み立てる。
/// flat エントリが `thumbnails` を持たない場合のフォールバックとして使う。
/// `i.ytimg.com` の静的 URL パターンで、CSP img-src の許可範囲内。
fn video_thumbnail(video_id: &str) -> String {
    format!("https://i.ytimg.com/vi/{video_id}/hqdefault.jpg")
}

/// `thumbnails` 配列から最後尾（高解像度側）の URL を取る。無ければフォールバック。
fn pick_thumbnail(entry: &serde_json::Value, video_id: &str) -> Option<String> {
    entry
        .get("thumbnails")
        .and_then(|t| t.as_array())
        .and_then(|arr| arr.last())
        .and_then(|t| t.get("url"))
        .and_then(|u| u.as_str())
        .map(|s| s.to_string())
        .or_else(|| Some(video_thumbnail(video_id)))
}

/// UC プレフィックス付きチャンネル ID（`UC` + 22 文字）かどうか。
/// マイグレーション v4 以降、DB 内の channel_id はこの形に正規化されている。
fn is_uc_channel_id(s: &str) -> bool {
    s.len() == 24 && s.starts_with("UC")
}

/// `--dump-json --flat-playlist` の行単位 JSONL を `SearchResult` へ変換する。
/// パース不能な行は捨てる（1 行の腐敗で全体を失敗させない暫定仕様）。
fn parse_search_jsonl(bytes: &[u8]) -> Vec<crate::model::SearchResult> {
    let mut out = Vec::new();
    for line in bytes.split(|&b| b == b'\n') {
        let line = line.trim_ascii();
        if line.is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_slice::<serde_json::Value>(line) else {
            continue;
        };
        let Some(id) = v.get("id").and_then(|s| s.as_str()) else {
            continue;
        };
        // flat エントリの id は動画 ID。プレイリスト等は _type=url で url 側を見るべきだが、
        // ytsearch のエントリは動画なので id のみ採用する。
        let title = v
            .get("title")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string();
        // channel_id は UC ID、uploader_id は @handle のことがある（実測確認）。
        // ブロック・購読の判定が UC 前提なので、UC 形の値だけを採用する。
        let channel_id = ["channel_id", "uploader_id"]
            .iter()
            .filter_map(|k| v.get(*k).and_then(|s| s.as_str()))
            .find(|s| is_uc_channel_id(s))
            .map(|s| s.to_string());
        // @handle はブロックキーに使えないが subscribe_channel の入力には使えるため、
        // UC ID が取れなかった結果でも購読導線を残せるよう別フィールドで露出する。
        let uploader_id = v
            .get("uploader_id")
            .and_then(|s| s.as_str())
            .filter(|s| s.starts_with('@'))
            .map(|s| s.to_string());
        let channel_title = v
            .get("channel")
            .or_else(|| v.get("uploader"))
            .and_then(|s| s.as_str())
            .map(|s| s.to_string());
        out.push(crate::model::SearchResult {
            video_id: id.to_string(),
            title,
            channel_id,
            uploader_id,
            channel_title,
            duration_sec: v.get("duration").and_then(|d| d.as_i64()),
            view_count: v.get("view_count").and_then(|c| c.as_i64()),
            thumbnail_url: pick_thumbnail(&v, id),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_jsonl_parses_flat_entries() {
        // yt-dlp --flat-playlist の出力形に沿った JSONL
        let input = br#"{"_type":"url","ie_key":"Youtube","id":"dQw4w9WgXcQ","url":"https://www.youtube.com/watch?v=dQw4w9WgXcQ","title":"Never Gonna Give You Up","description":"","duration":213,"channel_id":"UCuAXFkgsw1L7xaCfnd5JJOw","channel":"Rick Astley","view_count":1600000000,"thumbnails":[{"url":"https://i.ytimg.com/vi/dQw4w9WgXcQ/maxres.jpg"}]}
{"_type":"url","ie_key":"Youtube","id":"abc123def45","title":"No Meta Video","uploader":"Some Uploader","uploader_id":"UCuploader00000000000000","duration":60}
"#;
        let out = parse_search_jsonl(input);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].video_id, "dQw4w9WgXcQ");
        assert_eq!(
            out[0].channel_id.as_deref(),
            Some("UCuAXFkgsw1L7xaCfnd5JJOw")
        );
        assert_eq!(out[0].duration_sec, Some(213));
        assert_eq!(out[0].view_count, Some(1_600_000_000));
        assert_eq!(
            out[0].thumbnail_url.as_deref(),
            Some("https://i.ytimg.com/vi/dQw4w9WgXcQ/maxres.jpg")
        );
        // thumbnails 欠落時は ytimg 定形 URL にフォールバック
        assert_eq!(
            out[1].channel_id.as_deref(),
            Some("UCuploader00000000000000")
        );
        assert_eq!(out[1].channel_title.as_deref(), Some("Some Uploader"));
        assert_eq!(
            out[1].thumbnail_url.as_deref(),
            Some("https://i.ytimg.com/vi/abc123def45/hqdefault.jpg")
        );
    }

    /// R-2: uploader_id が @handle（UC 形でない）だけのエントリは channel_id=None にする。
    /// ハンドルを UC キーの blocked/channels と比較させないため。
    #[test]
    fn search_jsonl_rejects_handle_uploader_id() {
        let input = "{\"id\":\"abc123def45\",\"title\":\"Handle Only\",\"uploader\":\"Some Uploader\",\"uploader_id\":\"@SomeHandle\"}\n{\"id\":\"xyz987wvu65\",\"title\":\"UC in uploader_id\",\"uploader_id\":\"UCabcdef0000000000000000\"}\n{\"id\":\"jpn123def45\",\"title\":\"Unicode Handle\",\"uploader_id\":\"@\\u30d2\\u30ab\\u30ad\\u30f3\"}\n".as_bytes();
        let out = parse_search_jsonl(input);
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].channel_id, None);
        // @handle はブロックキーにはしないが、購読参照として uploader_id に残す
        assert_eq!(out[0].uploader_id.as_deref(), Some("@SomeHandle"));
        assert_eq!(out[0].channel_title.as_deref(), Some("Some Uploader"));
        // Unicode ハンドルも購読参照として露出する（parse_channel_ref が受理）
        assert_eq!(out[2].channel_id, None);
        assert_eq!(out[2].uploader_id.as_deref(), Some("@ヒカキン"));
        assert_eq!(
            out[1].channel_id.as_deref(),
            Some("UCabcdef0000000000000000")
        );
    }

    #[test]
    fn search_jsonl_skips_bad_lines() {
        let input = b"not json\n{\"id\":\"abc123def45\",\"title\":\"ok\"}\n{\"no_id\":1}\n";
        let out = parse_search_jsonl(input);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].video_id, "abc123def45");
    }

    #[test]
    fn channel_ref_candidates_orders_and_normalizes() {
        // channel_id 優先、uploader_id は @ を補う、"None" 文字列は空扱い
        let v = serde_json::json!({
            "channel_id": "UCuAXFkgsw1L7xaCfnd5JJOw",
            "uploader_id": "RickAstley",
            "channel_url": "https://www.youtube.com/channel/UCuAXFkgsw1L7xaCfnd5JJOw",
        });
        let c = channel_ref_candidates(&v);
        assert_eq!(
            c,
            vec![
                "UCuAXFkgsw1L7xaCfnd5JJOw".to_string(),
                "@RickAstley".to_string(),
                "https://www.youtube.com/channel/UCuAXFkgsw1L7xaCfnd5JJOw".to_string()
            ]
        );

        let v = serde_json::json!({
            "channel_id": "None",
            "uploader_id": "@handle1",
        });
        assert_eq!(channel_ref_candidates(&v), vec!["@handle1".to_string()]);

        // uploader_id が UC 形の場合は @ を付けない（実例: yt-dlp が
        // uploader_id に UC ID を返すことがある）
        let v = serde_json::json!({
            "channel_id": "None",
            "uploader_id": "UCuAXFkgsw1L7xaCfnd5JJOw",
            "channel_url": "https://www.youtube.com/channel/UCuAXFkgsw1L7xaCfnd5JJOw",
        });
        assert_eq!(
            channel_ref_candidates(&v),
            vec![
                "UCuAXFkgsw1L7xaCfnd5JJOw".to_string(),
                "https://www.youtube.com/channel/UCuAXFkgsw1L7xaCfnd5JJOw".to_string()
            ]
        );

        let v = serde_json::json!({
            "channel_id": "",
            "uploader_id": null,
            "channel_url": "https://www.youtube.com/channel/UCabc",
        });
        assert_eq!(
            channel_ref_candidates(&v),
            vec!["https://www.youtube.com/channel/UCabc".to_string()]
        );

        assert!(channel_ref_candidates(&serde_json::json!({})).is_empty());
    }
}
