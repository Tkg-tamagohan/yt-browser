//! UI とやり取りする共通型。

use serde::{Deserialize, Serialize};

/// `db_status` コマンドの戻り値。DB の生存確認に使う。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DbStatus {
    /// 適用済みマイグレーションの最新バージョン。未適用なら 0。
    pub schema_version: u32,
}

/// 再生状態の区分。`player://state` の `state` フィールドに載せる。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PlayStatus {
    /// ファイル未ロードまたは開始直後。
    Idle,
    Playing,
    Paused,
    /// キャッシュ切れで一時停止中（mpv `paused-for-cache`）。
    Buffering,
    Ended,
}

/// mpv インスタンスの現在状態。`player://state` イベントのペイロード（設計書 §3.2）。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerState {
    pub instance_id: u32,
    pub video_id: String,
    pub pause: bool,
    /// 再生位置（秒）。未ロード時は 0。
    pub position: f64,
    /// 動画長（秒）。取得前は 0。
    pub duration: f64,
    /// 現在ソースの fps。取得前は 0。
    pub fps: f64,
    pub state: PlayStatus,
    pub volume: f64,
    pub speed: f64,
    pub media_title: String,
}

/// `player://ended` イベントのペイロード（設計書 §3.2）。
/// reason は mpv の end-file reason（eof / stop / error）か内部理由（process_exit）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerEnded {
    pub instance_id: u32,
    pub video_id: String,
    pub reason: String,
}

/// `player_control` の操作指定（設計書 §3.1）。
/// `{ "type": "pause", "value": true }` のようなタグ付き列挙で受け取る。
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PlayerAction {
    /// 一時停止 / 再開（mpv `pause` プロパティ）。
    Pause { value: bool },
    /// 絶対位置シーク（秒）。
    Seek { seconds: f64 },
    /// 音量の絶対設定（mpv 仕様に合わせ 0-130）。
    Volume { value: f64 },
    /// 再生速度の絶対設定。
    Speed { value: f64 },
    /// 画質式（`ytdl-format`）の変更。変更後は現在位置を保持してリロードする。
    Quality { format: String },
    /// 1 フレーム前進（コマ送り）。
    FrameStep,
    /// 1 フレーム後退。
    FrameBackStep,
}

/// `watch_history` テーブルの 1 行（設計書 §8）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchHistory {
    pub video_id: String,
    pub title: String,
    pub channel_id: Option<String>,
    pub channel_title: Option<String>,
    pub position_sec: i64,
    pub duration_sec: Option<i64>,
    pub last_watched_at: String,
    pub completed: bool,
}

/// `ytdlp_status` コマンドの戻り値。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct YtDlpStatus {
    /// 解決した yt-dlp のパスまたはコマンド名。見つからない場合は null。
    pub path: Option<String>,
    /// `yt-dlp --version` の出力。取得失敗時は null。
    pub version: Option<String>,
}

/// YouTube の動画入力（URL 各形式または 11 文字の動画 ID）を動画 ID へ正規化する。
/// 受理する形式: 裸の ID、`watch?v=`、`youtu.be/`、`/shorts/`、`/live/`、`/embed/`。
/// 戻り値は 11 文字の動画 ID。解釈できない入力は None。
pub fn normalize_video_id(input: &str) -> Option<String> {
    let input = input.trim();
    if is_video_id(input) {
        return Some(input.to_string());
    }
    let url = url::Url::parse(input).ok()?;
    let host = url.host_str()?.to_ascii_lowercase();
    if host == "youtu.be" {
        let seg = url.path_segments()?.next()?;
        return is_video_id(seg).then(|| seg.to_string());
    }
    if !(host == "youtube.com" || host.ends_with(".youtube.com")) {
        return None;
    }
    if let Some(v) = url.query_pairs().find(|(k, _)| k == "v").map(|(_, v)| v) {
        if is_video_id(&v) {
            return Some(v.into_owned());
        }
    }
    // /shorts/<id> / /live/<id> / /embed/<id>
    let mut segs = url.path_segments()?;
    if matches!(segs.next(), Some("shorts" | "live" | "embed")) {
        if let Some(seg) = segs.next() {
            if is_video_id(seg) {
                return Some(seg.to_string());
            }
        }
    }
    None
}

/// YouTube 動画 ID の形式チェック（英数字・`-`・`_` の 11 文字）。
fn is_video_id(s: &str) -> bool {
    s.len() == 11
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_accepts_bare_id() {
        assert_eq!(
            normalize_video_id("dQw4w9WgXcQ"),
            Some("dQw4w9WgXcQ".to_string())
        );
        assert_eq!(
            normalize_video_id("  dQw4w9WgXcQ  "),
            Some("dQw4w9WgXcQ".into())
        );
    }

    #[test]
    fn normalize_accepts_watch_url() {
        assert_eq!(
            normalize_video_id("https://www.youtube.com/watch?v=dQw4w9WgXcQ"),
            Some("dQw4w9WgXcQ".into())
        );
        assert_eq!(
            normalize_video_id("https://www.youtube.com/watch?v=dQw4w9WgXcQ&t=42s"),
            Some("dQw4w9WgXcQ".into())
        );
    }

    #[test]
    fn normalize_accepts_short_and_alt_forms() {
        for url in [
            "https://youtu.be/dQw4w9WgXcQ",
            "https://youtu.be/dQw4w9WgXcQ?si=abc",
            "https://www.youtube.com/shorts/dQw4w9WgXcQ",
            "https://www.youtube.com/live/dQw4w9WgXcQ",
            "https://www.youtube.com/embed/dQw4w9WgXcQ",
        ] {
            assert_eq!(normalize_video_id(url), Some("dQw4w9WgXcQ".into()), "{url}");
        }
    }

    #[test]
    fn normalize_rejects_invalid() {
        for bad in [
            "",
            "too-short",
            "dQw4w9WgXcQ extra",
            "https://example.com/watch?v=dQw4w9WgXcQ",
            "https://www.youtube.com/watch?v=short",
            "not a url",
        ] {
            assert_eq!(normalize_video_id(bad), None, "{bad}");
        }
    }
}
