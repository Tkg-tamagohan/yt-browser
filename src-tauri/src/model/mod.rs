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

/// `channels` テーブルの 1 行（設計書 §8）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Channel {
    pub channel_id: String,
    pub title: String,
    pub thumbnail_url: Option<String>,
    pub category_id: Option<i64>,
    pub subscribed_at: String,
    pub last_polled_at: Option<String>,
}

/// `categories` テーブルの 1 行（設計書 §8）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Category {
    pub id: i64,
    pub name: String,
    pub sort_order: i64,
}

/// `list_feed` のフィルタ（設計書 §3.1）。全項目省略可。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FeedFilter {
    /// true で未読のみ。
    pub unread_only: bool,
    /// カテゴリで絞る。`Some(0)` は未分類のみ、`Some(n)` はそのカテゴリ。
    pub category_id: Option<i64>,
    /// 公開日が指定日数以降のものだけに絞る。
    pub days: Option<u32>,
}

/// `videos` テーブルの 1 行（設計書 §8）。`list_feed` の返却型。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedItem {
    pub video_id: String,
    pub channel_id: String,
    pub channel_title: Option<String>,
    pub title: String,
    pub thumbnail_url: Option<String>,
    pub published_at: Option<String>,
    pub kind: String,
    pub is_read: bool,
}

/// `feed://new_items` イベントのペイロード（設計書 §3.2）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedNewItems {
    pub count: usize,
}
/// `search` / `get_related` コマンドの結果行（設計書 §3.1 の SearchResult）。
/// 検索（yt-dlp flat playlist）と関連動画（InnerTube `next`）の共通型。
/// 片方の経路でしか取れない値は Option にする。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub video_id: String,
    pub title: String,
    /// UC 形チャンネル ID。ブロック・購読の判定キーとして使えるのはこの値だけ。
    pub channel_id: Option<String>,
    /// `@handle` 形の投稿者 ID。ブロックキーには使えないが、
    /// `subscribe_channel` は @handle を解決できるため購読導線の代替入力として露出する。
    pub uploader_id: Option<String>,
    pub channel_title: Option<String>,
    pub duration_sec: Option<i64>,
    pub view_count: Option<i64>,
    pub thumbnail_url: Option<String>,
}

/// `blocked_channels` テーブルの 1 行（設計書 §8）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockedChannel {
    pub channel_id: String,
    pub title: String,
    pub created_at: String,
}

/// `feed://status` イベントのペイロード（設計書 §3.2）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedStatus {
    pub channel_id: Option<String>,
    /// "info" | "warn" | "error"。
    pub level: String,
    pub message: String,
}

/// チャットイベントの種別（設計書 §3.1 の ChatEvent、§8 の kind CHECK と一致）。
/// 削除アクションは元メッセージを消さず `deleted` イベントとして記録する。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ChatKind {
    Text,
    Superchat,
    Membership,
    /// 削除アクション（実測では `removeChatItemAction`）。`message` に削除対象の item ID を入れる。
    Deleted,
    /// 上記以外の renderer（バナー・投票・エンゲージメント等）。表示はしないが保存はする。
    Other,
}

impl ChatKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Superchat => "superchat",
            Self::Membership => "membership",
            Self::Deleted => "deleted",
            Self::Other => "other",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "superchat" => Self::Superchat,
            "membership" => Self::Membership,
            "deleted" => Self::Deleted,
            "other" => Self::Other,
            _ => Self::Text,
        }
    }
}

/// ライブチャット 1 イベント（設計書 §3.1 の ChatEvent）。
/// `chat://message` バッチの要素かつ `chat_logs` への保存単位。
/// `raw_json` は未正規化フィールドの保存用で、UI には送らない。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatEvent {
    /// InnerTube のアイテム ID（削除参照・重複除去用）。
    /// 削除イベントは `message` に対象 ID を持つため、UI 側の行突合にも使う。
    pub item_id: String,
    pub video_id: String,
    /// `timestampUsec`（1970-01-01 からのマイクロ秒）。
    pub posted_at_usec: i64,
    pub author_channel_id: Option<String>,
    pub author_name: Option<String>,
    pub kind: ChatKind,
    pub message: String,
    pub amount_display: Option<String>,
    /// NG フィルタで非表示と判定されたか。保存も送信もするが、UI は非表示にする。
    pub ng: bool,
    /// renderer 原文（ストリームで取れた範囲の生 JSON）。UI には送らない。
    #[serde(skip_serializing)]
    pub raw_json: String,
}

/// `chat://status` イベントのペイロード（設計書 §3.2）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatStatus {
    pub video_id: Option<String>,
    /// "info" | "warn" | "error"。
    pub level: String,
    pub message: String,
}

/// `filters` テーブルの 1 行（設計書 §8 の NG フィルタ）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Filter {
    pub id: i64,
    /// 評価対象。`video_title` `video_desc` `channel_title` `channel_id`
    /// `chat_text` `chat_author` のいずれか。
    pub target: String,
    /// `literal` または `regex`。
    pub kind: String,
    pub pattern: String,
    pub enabled: bool,
    pub created_at: String,
}

/// filters.target の許容値（DDL の CHECK と一致）。
pub const FILTER_TARGETS: &[&str] = &[
    "video_title",
    "video_desc",
    "channel_title",
    "channel_id",
    "chat_text",
    "chat_author",
];

/// filters.kind の許容値（DDL の CHECK と一致）。
pub const FILTER_KINDS: &[&str] = &["literal", "regex"];

/// `favorite_add` / `playlist_add` の動画参照。
/// 検索・関連・フィード行のメタ情報をそのまま受け取り、`videos` 台帳へ登録する。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoRef {
    pub video_id: String,
    pub title: String,
    pub channel_id: Option<String>,
    pub channel_title: Option<String>,
    pub thumbnail_url: Option<String>,
}

/// `favorites` 一覧の 1 行。動画メタは `videos` 台帳から JOIN で取る。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FavoriteEntry {
    pub video_id: String,
    pub title: String,
    pub channel_id: Option<String>,
    pub channel_title: Option<String>,
    pub thumbnail_url: Option<String>,
    pub added_at: String,
}

/// `playlists` 一覧の 1 行（`item_count` は LEFT JOIN の集計）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Playlist {
    pub id: i64,
    pub name: String,
    pub sort_order: i64,
    pub item_count: i64,
}

/// `playlist_items` 一覧の 1 行（登録順の position 付き）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistEntry {
    pub position: i64,
    pub video_id: String,
    pub title: String,
    pub channel_id: Option<String>,
    pub channel_title: Option<String>,
    pub thumbnail_url: Option<String>,
}

/// チャンネル ID（`UC` プレフィックス + 22 文字）の形式チェック。
pub fn is_channel_id(s: &str) -> bool {
    s.len() == 24
        && s.starts_with("UC")
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// 購読入力（UC ID、channel URL、@handle）を正規化する。
/// 戻り値は `(channel_id または @handle, channel_id 確定か)`。
/// UC 形式に解決できる入力は `Ok(ChannelRef::Id(_))`、@handle は
/// `Ok(ChannelRef::Handle(_))`（要 yt-dlp 解決）。解釈不能は Err 相当の None。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChannelRef {
    /// 確定済みのチャンネル ID（UC...）。
    Id(String),
    /// ハンドル（@xxx）。チャンネル ID への変換が必要。
    Handle(String),
}

/// `subscribe_channel` の入力を正規化する。
/// 受理形式: `UC...`、youtube.com/channel/UC...、`@handle`、youtube.com/@handle。
pub fn parse_channel_ref(input: &str) -> Option<ChannelRef> {
    let input = input.trim();
    if is_channel_id(input) {
        return Some(ChannelRef::Id(input.to_string()));
    }
    if let Some(h) = input.strip_prefix('@') {
        return is_handle(h).then(|| ChannelRef::Handle(h.to_string()));
    }
    let url = url::Url::parse(input).ok()?;
    let host = url.host_str()?.to_ascii_lowercase();
    if !(host == "youtube.com" || host.ends_with(".youtube.com")) {
        return None;
    }
    let mut segs = url.path_segments()?;
    match segs.next()? {
        "channel" => segs
            .next()
            .and_then(|s| is_channel_id(s).then(|| ChannelRef::Id(s.to_string()))),
        seg => {
            // path_segments はパーセントエンコードのまま返るため、先にデコードする
            // （非 ASCII ハンドルはエンコードされて届く。生の @ ・ %40 両対応）
            let seg = percent_encoding::percent_decode_str(seg)
                .decode_utf8()
                .ok()?;
            seg.strip_prefix('@')
                .filter(|h| is_handle(h))
                .map(|h| ChannelRef::Handle(h.to_string()))
        }
    }
}

/// @handle の形式チェック（文字・数字・`-`・`_`・`.` の 3〜30 文字）。
/// YouTube のハンドルは非ラテン文字（日本語等）を許容するため、
/// 文字判定は Unicode の alphanumeric とし、長さは文字数で数える。
fn is_handle(s: &str) -> bool {
    (3..=30).contains(&s.chars().count())
        && s.chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.'))
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

    /// R-3: ハンドルは Unicode 文字を許容する（YouTube の実仕様に揃える）。
    #[test]
    fn channel_ref_accepts_unicode_handle() {
        assert_eq!(
            parse_channel_ref("@ヒカキン"),
            Some(ChannelRef::Handle("ヒカキン".into()))
        );
        assert_eq!(
            parse_channel_ref("https://www.youtube.com/@ヒカキン"),
            Some(ChannelRef::Handle("ヒカキン".into()))
        );
        assert_eq!(
            parse_channel_ref("@Hikakin.TV"),
            Some(ChannelRef::Handle("Hikakin.TV".into()))
        );
    }

    #[test]
    fn channel_ref_rejects_bad_handle() {
        for bad in ["@ab", "@a b", "@handle/with/slash", "@"] {
            assert_eq!(parse_channel_ref(bad), None, "{bad}");
        }
        let long = format!("@{}", "a".repeat(31));
        assert_eq!(parse_channel_ref(&long), None);
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
