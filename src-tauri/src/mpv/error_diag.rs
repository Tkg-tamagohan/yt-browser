//! mpv の `ytdl_hook` ログメッセージから再生開始失敗の原因を分類する診断器
//! （FR-95、仕様決定 EA、設計書 §4.23）。
//!
//! 外部依存は標準ライブラリと `regex` のみに留め、3 層一方向依存（上位層 → ドメイン層
//! → 下位共有層）を維持する。

use regex::{Captures, Regex};
use std::collections::VecDeque;
use std::sync::OnceLock;
use std::time::Duration;

/// 1 プレイヤーインスタンスあたり保持する診断行の最大数（仕様決定 EA）。
pub(crate) const MAX_BUFFER_LINES: usize = 50;
/// 1 プレイヤーインスタンスあたり保持する診断行の合計 UTF-8 バイト上限（16 KiB）。
pub(crate) const MAX_BUFFER_BYTES: usize = 16 * 1024;
/// `end-file` 到着時に終端印が未到着だった場合の最大待機時間（仕様決定 EA）。
pub(crate) const END_MARKER_WAIT: Duration = Duration::from_millis(200);
/// サニタイズ済み文字列の最大文字数（Unicode スカラ値単位）。
const MAX_SANITIZED_CHARS: usize = 200;

/// 再生開始失敗の原因カテゴリ（仕様決定 EA の優先順位表と 1 対 1 に対応）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlaybackErrorKind {
    VideoUnavailable,
    CookieFailed,
    Blocked,
    JsChallenge,
    NoFormat,
    Network,
    Unknown,
}

impl PlaybackErrorKind {
    /// `player://ended` の `errorKind` へ載せる識別子文字列。
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::VideoUnavailable => "video_unavailable",
            Self::CookieFailed => "cookie_failed",
            Self::Blocked => "blocked",
            Self::JsChallenge => "js_challenge",
            Self::NoFormat => "no_format",
            Self::Network => "network",
            Self::Unknown => "unknown",
        }
    }
}

/// 分類結果。`detail` は許可リストと伏せ字処理を通した値のみ保持する。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PlaybackErrorSummary {
    pub kind: PlaybackErrorKind,
    pub detail: Option<String>,
}

/// `ytdl_hook` 由来の診断行を 1 プレイヤーインスタンス単位で保持するリングバッファ。
#[derive(Debug, Default)]
pub(crate) struct YtdlErrorBuffer {
    lines: VecDeque<String>,
    total_bytes: usize,
    /// 現在の試行（初回または再送）で終端印 `youtube-dl failed` を受信済みか。
    end_marker_seen: bool,
    /// IPC 切断により後続の `log-message` が届かない状態か。
    disconnected: bool,
}

impl YtdlErrorBuffer {
    /// mpv の `log-message` を選別して追加する。
    /// 保持対象は `prefix == "ytdl_hook"` かつ `ERROR:` / `WARNING:` /
    /// `[debug] WARNING:` / `youtube-dl failed` で始まる行のみとし、
    /// `Command-line config` 等の設定行は受信時点で破棄する。
    /// 終端印（`youtube-dl failed`）を受理した場合に `true` を返す。
    pub(crate) fn push_log_message(&mut self, prefix: &str, text: &str) -> bool {
        if prefix != "ytdl_hook" {
            return false;
        }
        let trimmed = text.trim_end_matches(['\r', '\n']);
        let is_end_marker = trimmed.starts_with("youtube-dl failed");
        let accepted = is_end_marker
            || trimmed.starts_with("ERROR:")
            || trimmed.starts_with("WARNING:")
            || trimmed.starts_with("[debug] WARNING:");
        if !accepted {
            return false;
        }

        let entry = truncate_utf8_bytes(trimmed, MAX_BUFFER_BYTES).to_string();
        self.total_bytes += entry.len();
        self.lines.push_back(entry);

        while self.lines.len() > MAX_BUFFER_LINES || self.total_bytes > MAX_BUFFER_BYTES {
            if let Some(oldest) = self.lines.pop_front() {
                self.total_bytes = self.total_bytes.saturating_sub(oldest.len());
            } else {
                self.total_bytes = 0;
                break;
            }
        }

        if is_end_marker {
            self.end_marker_seen = true;
        }
        is_end_marker
    }

    /// 終端印を受信済み、または IPC 切断により待機継続が不要なら `true`。
    pub(crate) fn is_ready_to_classify(&self) -> bool {
        self.end_marker_seen || self.disconnected
    }

    /// 現在の保持行の複製（スナップショット）を返す。
    pub(crate) fn snapshot(&self) -> Vec<String> {
        self.lines.iter().cloned().collect()
    }

    /// 新しい動画読み込み（`arm_load`）時に保持行と待機状態をすべて空にする。
    pub(crate) fn clear(&mut self) {
        self.lines.clear();
        self.total_bytes = 0;
        self.end_marker_seen = false;
        self.disconnected = false;
    }

    /// 同一読み込みの 1 回自動再送（仕様決定 BJ）向けに、初回と再送の行を合算できるよう
    /// 保持行を残したまま終端印フラグだけをリセットする。
    pub(crate) fn reset_end_marker_for_retry(&mut self) {
        self.end_marker_seen = false;
    }

    /// IPC 切断時に呼び、終端印待ちを即座に打ち切れるようにする。
    pub(crate) fn mark_disconnected(&mut self) {
        self.disconnected = true;
    }

    /// IPC 切断済みなら `true`（死んだソケットへの自動再送抑止に使う）。
    pub(crate) fn is_disconnected(&self) -> bool {
        self.disconnected
    }
}

/// UTF-8 文字境界を壊さず最大 `max_bytes` バイト以内の先頭スライスを返す。
fn truncate_utf8_bytes(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// UI やアプリログへ渡す文字列の伏せ字処理（仕様決定 EA）。
/// 1. URL（`https?://\S+`）→ `***`
/// 2. 引用符で囲まれた文字列（`"[^"]*"`・`'[^']*'`）→ `***`
/// 3. パス（最初の `[A-Za-z]:\\`・`\\\\`・`~/`・行頭または空白直後の `/` から行末まで）→ `***`
/// 4. `=` の右辺（`=\S+`）→ `=***`
/// 5. 先頭 200 文字へ切り詰め
pub(crate) fn sanitize_line(line: &str) -> String {
    static RE_URL: OnceLock<Regex> = OnceLock::new();
    static RE_QUOTED: OnceLock<Regex> = OnceLock::new();
    static RE_PATH: OnceLock<Regex> = OnceLock::new();
    static RE_KV: OnceLock<Regex> = OnceLock::new();

    let re_url = RE_URL.get_or_init(|| Regex::new(r"https?://\S+").unwrap());
    let re_quoted = RE_QUOTED.get_or_init(|| Regex::new(r#""[^"]*"|'[^']*'"#).unwrap());
    let re_path = RE_PATH.get_or_init(|| {
        Regex::new(r"(?P<ws>^|\s)/.*|(?:file://|[A-Za-z]:[\\/]|\\\\|~/).*").unwrap()
    });
    let re_kv = RE_KV.get_or_init(|| Regex::new(r"=\S+").unwrap());

    let step1 = re_url.replace_all(line, "***");
    let step2 = re_quoted.replace_all(&step1, "***");
    let step3 = re_path.replace(&step2, |caps: &Captures<'_>| {
        let ws = caps.name("ws").map_or("", |m| m.as_str());
        format!("{ws}***")
    });
    let step4 = re_kv.replace_all(&step3, "=***");
    step4.chars().take(MAX_SANITIZED_CHARS).collect()
}

/// スナップショット行をサニタイズした列を返す（アプリログ出力用）。
pub(crate) fn sanitize_lines(lines: &[String]) -> Vec<String> {
    lines.iter().map(|line| sanitize_line(line)).collect()
}

/// 保持スナップショットから優先順位付きで再生開始失敗の原因を分類する（仕様決定 EA）。
pub(crate) fn classify(lines: &[String]) -> PlaybackErrorSummary {
    static RE_PRIVATE: OnceLock<Regex> = OnceLock::new();
    static RE_UNAVAILABLE: OnceLock<Regex> = OnceLock::new();
    static RE_MEMBERS: OnceLock<Regex> = OnceLock::new();
    static RE_UPCOMING: OnceLock<Regex> = OnceLock::new();
    static RE_COOKIE: OnceLock<Regex> = OnceLock::new();
    static RE_BLOCKED: OnceLock<Regex> = OnceLock::new();
    static RE_JS_CHALLENGE: OnceLock<Regex> = OnceLock::new();
    static RE_NO_FORMAT: OnceLock<Regex> = OnceLock::new();
    static RE_NETWORK: OnceLock<Regex> = OnceLock::new();

    let re_private = RE_PRIVATE.get_or_init(|| Regex::new(r"(?i)private video").unwrap());
    let re_unavailable = RE_UNAVAILABLE
        .get_or_init(|| Regex::new(r"(?i)video unavailable|this video is unavailable").unwrap());
    let re_members = RE_MEMBERS.get_or_init(|| Regex::new(r"(?i)members-only").unwrap());
    let re_upcoming =
        RE_UPCOMING.get_or_init(|| Regex::new(r"(?i)this live event will begin").unwrap());
    let re_cookie = RE_COOKIE
        .get_or_init(|| Regex::new(r"(?i)could not (?:copy|find) .*cookie|failed to decrypt").unwrap());
    let re_blocked = RE_BLOCKED.get_or_init(|| {
        Regex::new(r"(?i)^ERROR:.*(?:sign in to confirm you.re not a bot|http error 403)").unwrap()
    });
    let re_js_challenge =
        RE_JS_CHALLENGE.get_or_init(|| Regex::new(r"(?i)challenge solving failed").unwrap());
    let re_no_format = RE_NO_FORMAT.get_or_init(|| {
        Regex::new(r"(?i)requested format is not available|no video formats found").unwrap()
    });
    let re_network = RE_NETWORK
        .get_or_init(|| Regex::new(r"(?i)timed out|connection reset|unable to download").unwrap());

    // 順位 1: 動画側の理由（video_unavailable）
    for line in lines {
        if re_private.is_match(line) {
            return PlaybackErrorSummary {
                kind: PlaybackErrorKind::VideoUnavailable,
                detail: Some("private".to_string()),
            };
        }
        if re_unavailable.is_match(line) {
            return PlaybackErrorSummary {
                kind: PlaybackErrorKind::VideoUnavailable,
                detail: Some("unavailable".to_string()),
            };
        }
        if re_members.is_match(line) {
            return PlaybackErrorSummary {
                kind: PlaybackErrorKind::VideoUnavailable,
                detail: Some("members_only".to_string()),
            };
        }
        if re_upcoming.is_match(line) {
            return PlaybackErrorSummary {
                kind: PlaybackErrorKind::VideoUnavailable,
                detail: Some("upcoming".to_string()),
            };
        }
    }

    // 順位 2: Cookie の読み取り失敗（cookie_failed）
    if lines.iter().any(|l| re_cookie.is_match(l)) {
        return PlaybackErrorSummary {
            kind: PlaybackErrorKind::CookieFailed,
            detail: None,
        };
    }

    // 順位 3: bot 判定やアクセス拒否（blocked、ERROR 行のみ対象）
    if lines.iter().any(|l| re_blocked.is_match(l)) {
        return PlaybackErrorSummary {
            kind: PlaybackErrorKind::Blocked,
            detail: None,
        };
    }

    // 順位 4: JS チャレンジの失敗（js_challenge）
    if lines.iter().any(|l| re_js_challenge.is_match(l)) {
        return PlaybackErrorSummary {
            kind: PlaybackErrorKind::JsChallenge,
            detail: None,
        };
    }

    // 順位 5: 再生可能なフォーマットなし（no_format）
    if lines.iter().any(|l| re_no_format.is_match(l)) {
        return PlaybackErrorSummary {
            kind: PlaybackErrorKind::NoFormat,
            detail: None,
        };
    }

    // 順位 6: ネットワーク（network）
    if lines.iter().any(|l| re_network.is_match(l)) {
        return PlaybackErrorSummary {
            kind: PlaybackErrorKind::Network,
            detail: None,
        };
    }

    // 順位 7: 分類不能（unknown）。最初の ERROR: 行の本体を伏せ字処理して返す。
    let detail = lines.iter().find_map(|line| {
        let rest = line.strip_prefix("ERROR:")?;
        let cleaned = sanitize_line(rest.trim());
        let trimmed = cleaned.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    });

    PlaybackErrorSummary {
        kind: PlaybackErrorKind::Unknown,
        detail,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        classify, sanitize_line, sanitize_lines, PlaybackErrorKind, PlaybackErrorSummary,
        YtdlErrorBuffer, MAX_BUFFER_BYTES, MAX_BUFFER_LINES,
    };

    /// 生の `log-message` 列（prefix, text）をバッファ経由で選別してから分類するヘルパ。
    fn feed_and_classify(events: &[(&str, &str)]) -> (YtdlErrorBuffer, PlaybackErrorSummary) {
        let mut buf = YtdlErrorBuffer::default();
        for &(prefix, text) in events {
            buf.push_log_message(prefix, text);
        }
        let summary = classify(&buf.snapshot());
        (buf, summary)
    }

    /// DB-ED-01: 受信選別を通した実出力フィクスチャのカテゴリ分類（受け入れ基準 1）。
    /// `ytdl_hook` 以外の prefix や `[debug]` 設定行は選別で捨てられ、対象行だけで分類される。
    #[test]
    fn classify_real_fixtures_through_buffer() {
        // 1. video_unavailable（yt-dlp 2026.11.23 実出力: This video is unavailable）
        let (buf, sum) = feed_and_classify(&[
            ("cplayer", "ERROR: unrelated mpv error\n"),
            (
                "ytdl_hook",
                "[debug] Command-line config: ['--extractor-args', 'youtube:po_token=SECRET']\n",
            ),
            (
                "ytdl_hook",
                "ERROR: [youtube] 00000000000: This video is unavailable\r\n",
            ),
            ("ytdl_hook", "youtube-dl failed: unexpected error occurred\n"),
        ]);
        assert!(buf.is_ready_to_classify());
        assert_eq!(buf.snapshot().len(), 2);
        assert_eq!(
            sum,
            PlaybackErrorSummary {
                kind: PlaybackErrorKind::VideoUnavailable,
                detail: Some("unavailable".to_string()),
            }
        );

        // 1b. video_unavailable（private / members_only / upcoming / Video unavailable）
        for (line, expected_detail) in [
            ("ERROR: [youtube] abcdefghijk: Private video. Sign in if you've been granted access", "private"),
            ("ERROR: [youtube] abcdefghijk: Video unavailable", "unavailable"),
            ("ERROR: [youtube] abcdefghijk: Join this channel to get access to members-only content", "members_only"),
            ("ERROR: [youtube] abcdefghijk: This live event will begin in 30 minutes.", "upcoming"),
        ] {
            let (_, s) = feed_and_classify(&[("ytdl_hook", line)]);
            assert_eq!(s.kind, PlaybackErrorKind::VideoUnavailable);
            assert_eq!(s.detail.as_deref(), Some(expected_detail));
        }

        // 2. cookie_failed（yt-dlp 2026.11.23 実出力 + Chrome ロック / 復号失敗）
        for line in [
            "ERROR: could not find firefox cookies database in 'C:\\Users\\Alice Smith\\AppData\\Roaming\\Mozilla\\Firefox\\Profiles\\bad'\n",
            "ERROR: Could not copy Chrome cookie database. See https://github.com/yt-dlp/yt-dlp/issues/7271\n",
            "ERROR: Failed to decrypt with DPAPI\n",
        ] {
            let (_, s) = feed_and_classify(&[("ytdl_hook", line)]);
            assert_eq!(s.kind, PlaybackErrorKind::CookieFailed);
            assert_eq!(s.detail, None);
        }

        // 3. blocked（ERROR 行のみ対象。WARNING 行の 403 は blocked にしない）
        let (_, blocked_bot) = feed_and_classify(&[(
            "ytdl_hook",
            "ERROR: [youtube] dQw4w9WgXcQ: Sign in to confirm you're not a bot.\n",
        )]);
        assert_eq!(blocked_bot.kind, PlaybackErrorKind::Blocked);
        assert_eq!(blocked_bot.detail, None);

        let (_, blocked_403) = feed_and_classify(&[(
            "ytdl_hook",
            "ERROR: unable to download video data: HTTP Error 403: Forbidden\n",
        )]);
        assert_eq!(blocked_403.kind, PlaybackErrorKind::Blocked);

        let (_, warning_only_403) = feed_and_classify(&[
            (
                "ytdl_hook",
                "[debug] WARNING: [youtube] dQw4w9WgXcQ: HTTP Error 403: Forbidden\n",
            ),
            (
                "ytdl_hook",
                "ERROR: [youtube] dQw4w9WgXcQ: Requested format is not available.\n",
            ),
        ]);
        assert_eq!(warning_only_403.kind, PlaybackErrorKind::NoFormat);

        // 4. js_challenge（yt-dlp 2026.11.23 `--no-js-runtimes --verbose` 実出力）
        let (_, jsc) = feed_and_classify(&[
            (
                "ytdl_hook",
                "[debug] WARNING: [youtube] jNQXAC9IVRw: n challenge solving failed: Some formats may be missing.\n",
            ),
            (
                "ytdl_hook",
                "ERROR: [youtube] jNQXAC9IVRw: No video formats found!\n",
            ),
            ("ytdl_hook", "youtube-dl failed: unexpected error occurred\n"),
        ]);
        assert_eq!(jsc.kind, PlaybackErrorKind::JsChallenge);
        assert_eq!(jsc.detail, None);

        // 5. no_format（yt-dlp 2026.11.23 実出力）
        let (_, no_fmt) = feed_and_classify(&[
            (
                "ytdl_hook",
                "ERROR: [youtube] jNQXAC9IVRw: Requested format is not available. Use --list-formats for a list of available formats\n",
            ),
            ("ytdl_hook", "youtube-dl failed: unexpected error occurred\n"),
        ]);
        assert_eq!(no_fmt.kind, PlaybackErrorKind::NoFormat);
        assert_eq!(no_fmt.detail, None);

        // 6. network（yt-dlp 2026.11.23 dead proxy 実出力）
        let (_, net) = feed_and_classify(&[
            (
                "ytdl_hook",
                "[debug] WARNING: [youtube] ('Unable to connect to proxy', ConnectTimeoutError('Connection to 127.0.0.1 timed out.'))\n",
            ),
            (
                "ytdl_hook",
                "ERROR: [youtube] jNQXAC9IVRw: Unable to download API page: timed out\n",
            ),
        ]);
        assert_eq!(net.kind, PlaybackErrorKind::Network);
        assert_eq!(net.detail, None);
    }

    /// DB-ED-02: 複数カテゴリの行が混在するときの優先順位（受け入れ基準 1）。
    #[test]
    fn classify_respects_priority_order() {
        // video_unavailable (1) > cookie_failed (2) > blocked (3) > js_challenge (4) > no_format (5) > network (6)
        let lines = vec![
            "ERROR: Unable to download webpage: timed out".to_string(),
            "ERROR: Requested format is not available".to_string(),
            "[debug] WARNING: n challenge solving failed".to_string(),
            "ERROR: HTTP Error 403: Forbidden".to_string(),
            "ERROR: Could not copy Chrome cookie database".to_string(),
            "ERROR: [youtube] abcdefghijk: Private video".to_string(),
        ];
        assert_eq!(classify(&lines).kind, PlaybackErrorKind::VideoUnavailable);
        assert_eq!(classify(&lines[..5]).kind, PlaybackErrorKind::CookieFailed);
        assert_eq!(classify(&lines[..4]).kind, PlaybackErrorKind::Blocked);
        assert_eq!(classify(&lines[..3]).kind, PlaybackErrorKind::JsChallenge);
        assert_eq!(classify(&lines[..2]).kind, PlaybackErrorKind::NoFormat);
        assert_eq!(classify(&lines[..1]).kind, PlaybackErrorKind::Network);
    }

    /// DB-ED-03: PO Token を含む `[debug] Command-line config` のみの入力は選別で破棄され `unknown`（detail なし）になる（受け入れ基準 1）。
    #[test]
    fn command_line_config_with_po_token_is_discarded() {
        let (buf, summary) = feed_and_classify(&[
            (
                "ytdl_hook",
                "[debug] Command-line config: ['--extractor-args', 'youtube:po_token=mweb.gvs+VERY_SECRET_TOKEN_VALUE']\n",
            ),
            ("ytdl_hook", "[debug] Encodings: locale cp932, fs utf-8\n"),
            ("ytdl_hook", "youtube-dl failed: unexpected error occurred\n"),
        ]);
        let snap = buf.snapshot();
        assert_eq!(snap, vec!["youtube-dl failed: unexpected error occurred"]);
        assert_eq!(
            summary,
            PlaybackErrorSummary {
                kind: PlaybackErrorKind::Unknown,
                detail: None,
            }
        );
    }

    /// DB-ED-04: 伏せ字処理が秘匿情報（po_token、googlevideo 署名 URL、空白入り Windows パス、UNC パス、Unix パス、引用符、200 文字超過）を完全に除去する（受け入れ基準 2）。
    #[test]
    fn sanitize_line_redacts_all_sensitive_patterns() {
        // po_token= の値
        let s1 = sanitize_line("ERROR: extractor failed po_token=mweb.gvs+SUPER_SECRET_12345");
        assert!(!s1.contains("SUPER_SECRET_12345"), "got: {s1}");
        assert!(s1.contains("po_token=***"), "got: {s1}");

        // googlevideo の署名付き URL
        let s2 = sanitize_line(
            "ERROR: failed https://rr1---sn-abc.googlevideo.com/videoplayback?expire=123&sig=SECRET_SIG&id=1",
        );
        assert!(!s2.contains("googlevideo.com"), "got: {s2}");
        assert!(!s2.contains("SECRET_SIG"), "got: {s2}");
        assert!(s2.contains("***"), "got: {s2}");

        // 空白を含むユーザー名のある Windows パス
        let s3 = sanitize_line(
            r"ERROR: cannot open C:\Users\Alice Smith\AppData\Roaming\yt-dlp\cookies.txt for reading",
        );
        assert!(!s3.contains("Alice Smith"), "got: {s3}");
        assert!(!s3.contains("cookies.txt"), "got: {s3}");
        assert_eq!(s3, "ERROR: cannot open ***");

        // UNC パス
        let s4 = sanitize_line(r"ERROR: failed at \\fileserver\private share\secret.db");
        assert!(!s4.contains("fileserver"), "got: {s4}");
        assert!(!s4.contains("secret.db"), "got: {s4}");
        assert_eq!(s4, "ERROR: failed at ***");

        // Unix パス（空白直後と行頭）およびチルダパス
        let s5 = sanitize_line("ERROR: failed reading /home/alice smith/.config/yt-dlp/cookies.txt");
        assert!(!s5.contains("alice smith"), "got: {s5}");
        assert_eq!(s5, "ERROR: failed reading ***");

        let s6 = sanitize_line("/var/private/secret.txt failed");
        assert!(!s6.contains("secret.txt"), "got: {s6}");
        assert_eq!(s6, "***");

        let s7 = sanitize_line("ERROR: bad config ~/secret dir/config");
        assert!(!s7.contains("secret dir"), "got: {s7}");
        assert_eq!(s7, "ERROR: bad config ***");

        // スラッシュ区切りの Windows パスと file:// パスも伏せる
        let s_fwd = sanitize_line("ERROR: failed C:/Users/Alice Smith/cookies.txt");
        assert!(!s_fwd.contains("Alice Smith"), "got: {s_fwd}");
        assert_eq!(s_fwd, "ERROR: failed ***");

        let s_file = sanitize_line("ERROR: failed file:///home/alice smith/cookies.txt");
        assert!(!s_file.contains("alice smith"), "got: {s_file}");
        assert_eq!(s_file, "ERROR: failed ***");

        // 非パスのスラッシュ（1/3 等）は残る
        let s_ratio = sanitize_line("WARNING: Retrying (1/3) due to error");
        assert_eq!(s_ratio, "WARNING: Retrying (1/3) due to error");

        // 引用符内の値
        let s8 = sanitize_line(r#"ERROR: bad header "Authorization: Bearer TOPSECRET" and 'cookie=XYZ'"#);
        assert!(!s8.contains("TOPSECRET"), "got: {s8}");
        assert!(!s8.contains("XYZ"), "got: {s8}");

        // 200 文字切り詰め（マルチバイト文字でも安全に 200 文字で切れる）
        let long = format!("ERROR: {}", "あ".repeat(250));
        let s9 = sanitize_line(&long);
        assert_eq!(s9.chars().count(), 200);
    }

    /// DB-ED-05: `unknown` の `errorDetail` およびアプリログ用 `sanitize_lines` の双方に秘匿情報が残らない（受け入れ基準 2）。
    #[test]
    fn unknown_detail_and_log_output_are_sanitized() {
        let lines = vec![
            r"ERROR: weird failure po_token=SECRET_PO https://rr2.googlevideo.com/v?sig=SIG C:\Users\Bob Jones\cookies.txt".to_string(),
            "WARNING: extra info /home/bob/secret.txt".to_string(),
        ];
        let summary = classify(&lines);
        assert_eq!(summary.kind, PlaybackErrorKind::Unknown);
        let detail = summary.detail.expect("unknown は先頭 ERROR 行から detail を作る");
        for secret in ["SECRET_PO", "googlevideo", "Bob Jones", "cookies.txt"] {
            assert!(!detail.contains(secret), "detail に {secret} が残った: {detail}");
        }
        let logged = sanitize_lines(&lines);
        for line in &logged {
            for secret in ["SECRET_PO", "googlevideo", "Bob Jones", "cookies.txt", "/home/bob"] {
                assert!(!line.contains(secret), "ログに {secret} が残った: {line}");
            }
        }
    }

    /// DB-ED-06: 保持上限（50 行かつ UTF-8 16 KiB）を超えたら古い行から捨てる（受け入れ基準 8）。
    #[test]
    fn buffer_evicts_oldest_on_line_and_byte_limits() {
        // 行数上限 50 行
        let mut buf = YtdlErrorBuffer::default();
        for i in 0..70 {
            buf.push_log_message("ytdl_hook", &format!("ERROR: line-{i}\n"));
        }
        let snap = buf.snapshot();
        assert_eq!(snap.len(), MAX_BUFFER_LINES);
        assert_eq!(snap.first().unwrap(), "ERROR: line-20");
        assert_eq!(snap.last().unwrap(), "ERROR: line-69");

        // バイト上限 16 KiB（1 行約 2000 バイト × 12 行 = 約 24 KiB → 古い行から破棄）
        let mut buf_bytes = YtdlErrorBuffer::default();
        for i in 0..12 {
            let line = format!("ERROR: {i:02} {}", "x".repeat(2000));
            buf_bytes.push_log_message("ytdl_hook", &line);
        }
        let snap_b = buf_bytes.snapshot();
        let total: usize = snap_b.iter().map(String::len).sum();
        assert!(total <= MAX_BUFFER_BYTES, "total={total}");
        assert!(snap_b.last().unwrap().starts_with("ERROR: 11 "));
        assert!(!snap_b.first().unwrap().starts_with("ERROR: 00 "));

        // 単体で 16 KiB 超のマルチバイト行も UTF-8 境界で安全に切り詰めて格納する
        let mut buf_single = YtdlErrorBuffer::default();
        let huge = format!("ERROR: {}", "あ".repeat(10_000));
        buf_single.push_log_message("ytdl_hook", &huge);
        let snap_s = buf_single.snapshot();
        assert_eq!(snap_s.len(), 1);
        assert!(snap_s[0].len() <= MAX_BUFFER_BYTES);
    }

    /// DB-ED-07: 新規読み込みでのクリア、再送での保持維持と終端印リセット、別インスタンス分離（受け入れ基準 8）。
    #[test]
    fn buffer_lifecycle_clear_retry_and_isolation() {
        let mut buf_a = YtdlErrorBuffer::default();
        let mut buf_b = YtdlErrorBuffer::default();

        buf_a.push_log_message("ytdl_hook", "ERROR: Requested format is not available\n");
        buf_a.push_log_message("ytdl_hook", "youtube-dl failed: unexpected error occurred\n");
        buf_b.push_log_message("ytdl_hook", "ERROR: [youtube] xyz: Private video\n");

        // インスタンス間で混ざらない
        assert_eq!(classify(&buf_a.snapshot()).kind, PlaybackErrorKind::NoFormat);
        assert_eq!(
            classify(&buf_b.snapshot()).kind,
            PlaybackErrorKind::VideoUnavailable
        );

        // 再送では行を保持しつつ終端印だけを未到着へ戻す
        assert!(buf_a.is_ready_to_classify());
        buf_a.reset_end_marker_for_retry();
        assert!(!buf_a.is_ready_to_classify());
        buf_a.push_log_message(
            "ytdl_hook",
            "[debug] WARNING: [youtube] abc: n challenge solving failed\n",
        );
        buf_a.push_log_message("ytdl_hook", "youtube-dl failed: unexpected error occurred\n");
        assert!(buf_a.is_ready_to_classify());
        // 初回（no_format）と再送（js_challenge）の合算で優先順位の高い js_challenge になる
        assert_eq!(
            classify(&buf_a.snapshot()).kind,
            PlaybackErrorKind::JsChallenge
        );

        // 新規読み込みで空になる
        buf_a.clear();
        assert!(buf_a.snapshot().is_empty());
        assert!(!buf_a.is_ready_to_classify());
        assert_eq!(classify(&buf_a.snapshot()).kind, PlaybackErrorKind::Unknown);
    }

    /// DB-ED-08: 空スナップショットと `ERROR:` のみ（本体空）の境界値。
    #[test]
    fn classify_empty_and_blank_error_edge_cases() {
        assert_eq!(
            classify(&[]),
            PlaybackErrorSummary {
                kind: PlaybackErrorKind::Unknown,
                detail: None,
            }
        );
        assert_eq!(
            classify(&["ERROR:   ".to_string()]),
            PlaybackErrorSummary {
                kind: PlaybackErrorKind::Unknown,
                detail: None,
            }
        );
    }
}
