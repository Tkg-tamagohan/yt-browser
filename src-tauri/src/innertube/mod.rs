//! InnerTube クライアント（設計書 §6.1、FR-4、FR-6）。
//! watch ページの `ytcfg`（`INNERTUBE_API_KEY` / `INNERTUBE_CONTEXT_CLIENT_VERSION` /
//! `VISITOR_DATA`）を一度だけ取得してキャッシュし、`post_json` を提供する。
//! Phase 5 では関連動画取得（`next`）が、Phase 6 のチャットポーラーがこれを共用する。

use std::sync::Mutex;

use reqwest::Client;
use serde_json::{json, Value};
use thiserror::Error;

use crate::model::SearchResult;

/// ytcfg の取得元。どの watch ページでも同じ値が入るため固定動画で取得する。
const YTCFG_URL: &str = "https://www.youtube.com/watch?v=dQw4w9WgXcQ";
const API_BASE: &str = "https://www.youtube.com/youtubei/v1";

#[derive(Debug, Error)]
pub enum InnerTubeError {
    #[error("InnerTube への接続に失敗: {0}")]
    Http(#[from] reqwest::Error),
    #[error("InnerTube がエラー応答を返した: {0}")]
    Status(reqwest::StatusCode),
    #[error("watch ページから {0} を抽出できなかった")]
    CfgMissing(&'static str),
}

/// watch ページから取得するクライアント構成。
#[derive(Debug, Clone)]
struct YtCfg {
    api_key: String,
    client_version: String,
    visitor_data: String,
}

/// InnerTube クライアント。`tauri::State` として 1 個を共有する。
pub struct InnerTube {
    client: Client,
    cfg: Mutex<Option<YtCfg>>,
}

impl InnerTube {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
            cfg: Mutex::new(None),
        }
    }

    /// watch ページ HTML の `ytcfg` からキー群を抽出する。
    /// HTML 内に `"KEY":"value"` の形で埋め込まれている。
    fn extract_cfg(html: &str) -> Result<YtCfg, InnerTubeError> {
        Ok(YtCfg {
            api_key: find_json_str(html, "INNERTUBE_API_KEY")
                .ok_or(InnerTubeError::CfgMissing("INNERTUBE_API_KEY"))?,
            client_version: find_json_str(html, "INNERTUBE_CONTEXT_CLIENT_VERSION").ok_or(
                InnerTubeError::CfgMissing("INNERTUBE_CONTEXT_CLIENT_VERSION"),
            )?,
            visitor_data: find_json_str(html, "VISITOR_DATA")
                .ok_or(InnerTubeError::CfgMissing("VISITOR_DATA"))?,
        })
    }

    /// ytcfg を取得してキャッシュする。既にキャッシュ済みならそれを返す。
    async fn cfg(&self) -> Result<YtCfg, InnerTubeError> {
        if let Some(c) = self.cfg.lock().unwrap().clone() {
            return Ok(c);
        }
        let html = self.client.get(YTCFG_URL).send().await?.text().await?;
        let cfg = Self::extract_cfg(&html)?;
        *self.cfg.lock().unwrap() = Some(cfg.clone());
        Ok(cfg)
    }

    /// InnerTube エンドポイントへの POST。403/401 時は ytcfg を破棄して 1 回だけ再取得する。
    async fn post_json(&self, endpoint: &str, mut body: Value) -> Result<Value, InnerTubeError> {
        for attempt in 0..2 {
            let cfg = self.cfg().await?;
            body["context"]["client"]["clientName"] = json!("WEB");
            body["context"]["client"]["clientVersion"] = json!(cfg.client_version);
            body["context"]["client"]["visitorData"] = json!(cfg.visitor_data);
            let url = format!("{API_BASE}/{endpoint}?key={}", cfg.api_key);
            let res = self.client.post(&url).json(&body).send().await?;
            if res.status().is_success() {
                return Ok(res.json::<Value>().await?);
            }
            let status = res.status();
            // 認証系の失敗はキャッシュが腐った可能性があるので一度だけ再取得
            if attempt == 0
                && (status == reqwest::StatusCode::FORBIDDEN
                    || status == reqwest::StatusCode::UNAUTHORIZED)
            {
                *self.cfg.lock().unwrap() = None;
                continue;
            }
            return Err(InnerTubeError::Status(status));
        }
        unreachable!()
    }

    /// `next` エンドポイントで関連動画を取得する（設計書 §6.1）。
    /// 応答は 2026 年時点では `lockupViewModel` 形式だが、旧来の
    /// `compactVideoRenderer` / `videoWithContextRenderer` も併せて走査する。
    pub async fn related(&self, video_id: &str) -> Result<Vec<SearchResult>, InnerTubeError> {
        let v = self
            .post_json("next", json!({ "videoId": video_id }))
            .await?;
        Ok(parse_related(&v))
    }

    /// 指定動画の watch ページ HTML を取得する（チャットの初期継続トークン用）。
    pub async fn watch_html(&self, video_id: &str) -> Result<String, InnerTubeError> {
        let url = format!("https://www.youtube.com/watch?v={video_id}");
        Ok(self.client.get(&url).send().await?.text().await?)
    }

    /// `live_chat/get_live_chat`（設計書 §6.2）。continuation で差分アクションと
    /// 次の継続トークンを取る。応答の正規化は `chat` モジュールが担当する。
    pub async fn get_live_chat(&self, continuation: &str) -> Result<Value, InnerTubeError> {
        self.post_json(
            "live_chat/get_live_chat",
            json!({ "continuation": continuation }),
        )
        .await
    }

    /// `live_chat/get_live_chat_replay`（FR-24、仕様決定 AQ）。
    /// 終了済み配信のリプレイはこのエンドポイントで辿る。watch ページの
    /// `reloadContinuationData` をそのまま最初の continuation として使え、
    /// 応答には `replayChatItemAction`（`videoOffsetTimeMsec` 付き）と
    /// 次の `liveChatReplayContinuationData` が入る（2026-10 実機確認）。
    /// `get_live_chat` に同じトークンを投げると 400 になる点に注意。
    pub async fn get_live_chat_replay(&self, continuation: &str) -> Result<Value, InnerTubeError> {
        self.post_json(
            "live_chat/get_live_chat_replay",
            json!({ "continuation": continuation }),
        )
        .await
    }
}

/// 継続トークンの種別（仕様決定 AQ のライブ/リプレイ自動判定）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContinuationKind {
    /// 進行中ライブ（timed / invalidation / reload 系の継続）。
    Live,
    /// 終了済み配信のアーカイブリプレイ
    /// （`liveChatReplayContinuationData`）。
    Replay,
}

/// watch ページ HTML の `ytInitialData` からチャットの初期継続トークンと
/// 種別を取る（設計書 §6.2）。チャットの無い動画（非ライブ・チャット無効）は None。
pub fn extract_initial_continuation(html: &str) -> Option<(String, ContinuationKind)> {
    let data = extract_yt_initial_data(html)?;
    let lcr = find_key(&data, "liveChatRenderer")?;
    next_continuation(lcr).map(|(token, _, kind)| (token, kind))
}

/// `liveChatRenderer`（watch HTML）または `liveChatContinuation`（ポーリング応答）の
/// `continuations[]` から次の継続トークン・待機時間・種別を取る。
/// エントリは `{<type>ContinuationData: {continuation, timeoutMs?}}` の形で、
/// リプレイは `liveChatReplayContinuationData` のキーを持つ。
/// リプレイ継続が混在していればそれを優先する（アーカイブの続きを辿る経路。
/// ライブ継続と混在するのは終了直後の切替期のみのはず）。
/// 継続候補が無い場合（配信終了など）は None。
pub fn next_continuation(node: &Value) -> Option<(String, u64, ContinuationKind)> {
    let conts = node.get("continuations")?.as_array()?;
    let parse = |c: &Value| -> Option<(String, u64, ContinuationKind)> {
        c.as_object()?.iter().find_map(|(key, data)| {
            let token = data.get("continuation").and_then(|t| t.as_str())?;
            let kind = if key == "liveChatReplayContinuationData" {
                ContinuationKind::Replay
            } else {
                ContinuationKind::Live
            };
            Some((
                token.to_string(),
                data.get("timeoutMs").and_then(|t| t.as_u64()).unwrap_or(0),
                kind,
            ))
        })
    };
    conts
        .iter()
        .find_map(|c| parse(c).filter(|(_, _, k)| *k == ContinuationKind::Replay))
        .or_else(|| conts.iter().find_map(parse))
}

/// リプレイのオフセット補正に使う放送窓（FR-24、仕様決定 AQ）。
/// `ytInitialPlayerResponse` の `liveBroadcastDetails` と `videoDetails` から取る。
#[derive(Debug, Clone, Copy, Default)]
pub struct BroadcastWindow {
    /// 放送開始（epoch ms）。`liveBroadcastDetails.startTimestamp`。
    pub start_ms: Option<i64>,
    /// 放送終了（epoch ms）。`liveBroadcastDetails.endTimestamp`。
    pub end_ms: Option<i64>,
    /// 動画ファイルの長さ（秒）。`videoDetails.lengthSeconds`。
    pub length_secs: Option<i64>,
    /// 現在放送中か（`liveBroadcastDetails.isLiveNow`）。
    /// 終了済み判定に使う（None = 不明）。
    pub is_live_now: Option<bool>,
}

impl BroadcastWindow {
    /// 終了済みの放送か（ライブ継続ではなくリプレイ経路を選ぶ判定）。
    /// `endTimestamp` の存在を終了の証拠とする。予約配信のロビーは
    /// `isLiveNow: false` でも `endTimestamp` が無いためライブ経路を維持し、
    /// 放送中（`isLiveNow: true`）に end が出る矛盾形もライブ経路に留める。
    pub fn is_ended(&self) -> bool {
        self.is_live_now != Some(true) && self.end_ms.is_some()
    }
}

/// watch ページ HTML の `ytInitialPlayerResponse` から放送窓を取る。
/// プレミア公開では放送窓の長さが動画長より長く、その差が
/// カウントダウン等の前置き分（オフセット補正量の推定に使う）。
pub fn extract_broadcast_window(html: &str) -> BroadcastWindow {
    let Some(v) = extract_embedded_json(html, "ytInitialPlayerResponse") else {
        return BroadcastWindow::default();
    };
    let details = find_key(&v, "liveBroadcastDetails");
    let video = find_key(&v, "videoDetails");
    BroadcastWindow {
        start_ms: details
            .and_then(|d| d.get("startTimestamp"))
            .and_then(|t| t.as_str())
            .and_then(iso8601_ms),
        end_ms: details
            .and_then(|d| d.get("endTimestamp"))
            .and_then(|t| t.as_str())
            .and_then(iso8601_ms),
        length_secs: video
            .and_then(|d| d.get("lengthSeconds"))
            .and_then(|s| s.as_str())
            .and_then(|s| s.parse().ok()),
        is_live_now: details
            .and_then(|d| d.get("isLiveNow"))
            .and_then(|b| b.as_bool()),
    }
}

/// `"YYYY-MM-DDTHH:MM:SS(.sss)?(Z|±HH:MM)"` を epoch ミリ秒へ変換する最小パーサ。
/// YouTube の liveBroadcastDetails タイムスタンプ形式に限定して扱う。
fn iso8601_ms(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    let num = |i: usize, n: usize| -> Option<i64> {
        let slice = b.get(i..i + n)?;
        if !slice.iter().all(|c| c.is_ascii_digit()) {
            return None;
        }
        let mut x = 0i64;
        for c in slice {
            x = x * 10 + (c - b'0') as i64;
        }
        Some(x)
    };
    if b.len() < 19
        || b[4] != b'-'
        || b[7] != b'-'
        || (b[10] != b'T' && b[10] != b' ')
        || b[13] != b':'
        || b[16] != b':'
    {
        return None;
    }
    let (y, mo, d) = (num(0, 4)?, num(5, 2)?, num(8, 2)?);
    let (h, mi, sec) = (num(11, 2)?, num(14, 2)?, num(17, 2)?);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || mi > 59 || sec > 60 {
        return None;
    }
    // days-from-civil（Howard Hinnant の式）で 1970-01-01 からの日数を得る
    let y2 = if mo <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let mp = (mo + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let days = era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468;
    let mut ms = (days * 86_400 + h * 3_600 + mi * 60 + sec) * 1_000;
    let mut i = 19;
    if b.get(i) == Some(&b'.') {
        i += 1;
        let start = i;
        while b.get(i).is_some_and(|c| c.is_ascii_digit()) {
            i += 1;
        }
        let digits = i - start;
        if digits == 0 {
            return None;
        }
        // 小数秒はミリ秒（3 桁）へ丸める
        let f = num(start, digits.min(3))?;
        ms += f * 10i64.pow(3 - digits.min(3) as u32);
    }
    match b.get(i) {
        None | Some(&b'Z') => {}
        Some(sign @ (&b'+' | &b'-')) => {
            let oh = num(i + 1, 2)?;
            let om = if b.get(i + 3) == Some(&b':') {
                num(i + 4, 2)?
            } else {
                num(i + 3, 2)?
            };
            let off = (oh * 3_600 + om * 60) * 1_000;
            ms += if *sign == b'+' { -off } else { off };
        }
        _ => return None,
    }
    Some(ms)
}

/// HTML 中の `marker` 代入に続く JSON オブジェクトを取り出す。
fn extract_embedded_json(html: &str, marker: &str) -> Option<Value> {
    let idx = html.find(marker)?;
    let rest = &html[idx..];
    let eq = rest.find('=')?;
    let start = rest[eq..].find('{')? + eq;
    let end = json_object_end(&rest[start..])?;
    serde_json::from_str(&rest[start..start + end]).ok()
}

/// HTML 中の `ytInitialData` 代入に続く JSON オブジェクトを取り出す。
fn extract_yt_initial_data(html: &str) -> Option<Value> {
    extract_embedded_json(html, "ytInitialData")
}

/// 先頭 `{` から対応する閉じ括弧までのバイト長を返す。
/// 文字列リテラル内の波括弧を数えないよう、文字列とエスケープをスキップする。
fn json_object_end(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    if b.first() != Some(&b'{') {
        return None;
    }
    let mut depth = 0usize;
    let mut in_str = false;
    let mut esc = false;
    for (i, &c) in b.iter().enumerate() {
        if in_str {
            if esc {
                esc = false;
            } else if c == b'\\' {
                esc = true;
            } else if c == b'"' {
                in_str = false;
            }
            continue;
        }
        match c {
            b'"' => in_str = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i + 1);
                }
            }
            _ => {}
        }
    }
    None
}

/// JSON ツリー内で指定キーを持つ最初の値を深さ優先で返す。
fn find_key<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    match v {
        Value::Object(m) => {
            if let Some(x) = m.get(key) {
                return Some(x);
            }
            m.values().find_map(|x| find_key(x, key))
        }
        Value::Array(a) => a.iter().find_map(|x| find_key(x, key)),
        _ => None,
    }
}

/// `"KEY":"value"` 形式の埋め込み値を抽出する（エスケープ `"` は最小限対応）。
fn find_json_str(html: &str, key: &str) -> Option<String> {
    let pat = format!("\"{key}\":\"");
    let start = html.find(&pat)? + pat.len();
    let rest = &html[start..];
    // 値にエスケープが出ない前提で、単純に次の " までを取る
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

/// `next` 応答から `compactVideoRenderer` / `videoWithContextRenderer` /
/// `lockupViewModel`（2026 年時点の実応答形式）を再帰的に拾って
/// `SearchResult` に変換する。レイアウト差分に強いよう固定パスではなく
/// キー名で探索する。同一 video_id は最初の出現だけを採る。
fn parse_related(v: &Value) -> Vec<SearchResult> {
    let mut out = Vec::new();
    collect_renderers(v, &mut out);
    let mut seen = std::collections::HashSet::new();
    out.retain(|r: &SearchResult| seen.insert(r.video_id.clone()));
    out
}

fn collect_renderers(v: &Value, out: &mut Vec<SearchResult>) {
    if let Value::Object(map) = v {
        for key in ["compactVideoRenderer", "videoWithContextRenderer"] {
            if let Some(r) = map.get(key) {
                if let Some(item) = renderer_to_result(r) {
                    out.push(item);
                }
            }
        }
        if let Some(r) = map.get("lockupViewModel") {
            if let Some(item) = lockup_to_result(r) {
                out.push(item);
            }
        }
        for value in map.values() {
            collect_renderers(value, out);
        }
    } else if let Value::Array(arr) = v {
        for item in arr {
            collect_renderers(item, out);
        }
    }
}

fn renderer_to_result(r: &Value) -> Option<SearchResult> {
    let video_id = r.get("videoId")?.as_str()?.to_string();
    let title = text_of(r.get("title")?);
    let channel_run = r
        .get("shortBylineText")
        .or_else(|| r.get("longBylineText"))
        .and_then(|b| b.get("runs"))
        .and_then(|runs| runs.as_array())
        .and_then(|runs| runs.first());
    let channel_title = channel_run
        .and_then(|run| run.get("text"))
        .and_then(|t| t.as_str())
        .map(|s| s.to_string());
    let channel_id = channel_run
        .and_then(|run| {
            run.get("navigationEndpoint")
                .and_then(|n| n.get("browseEndpoint"))
                .and_then(|b| b.get("browseId"))
        })
        .and_then(|id| id.as_str())
        .map(|s| s.to_string());
    Some(SearchResult {
        video_id: video_id.clone(),
        title,
        channel_id,
        uploader_id: None,
        channel_title,
        duration_sec: r
            .get("lengthText")
            .and_then(|l| l.get("simpleText"))
            .and_then(|t| t.as_str())
            .and_then(parse_length_text),
        view_count: r
            .get("viewCountText")
            .and_then(|c| c.get("simpleText"))
            .and_then(|t| t.as_str())
            .and_then(parse_view_count),
        thumbnail_url: r
            .get("thumbnail")
            .and_then(|t| t.get("thumbnails"))
            .and_then(|a| a.as_array())
            .and_then(|a| a.last())
            .and_then(|t| t.get("url"))
            .and_then(|u| u.as_str())
            .map(|s| s.to_string()),
    })
}

/// `lockupViewModel`（2026 年時点の `next` 応答で使われる形式）を `SearchResult` に
/// 変換する。`LOCKUP_CONTENT_TYPE_VIDEO` 以外（プレイリスト等）は拾わない。
fn lockup_to_result(r: &Value) -> Option<SearchResult> {
    if r.get("contentType")?.as_str()? != "LOCKUP_CONTENT_TYPE_VIDEO" {
        return None;
    }
    let video_id = r.get("contentId")?.as_str()?.to_string();
    let meta = r.get("metadata")?.get("lockupMetadataViewModel")?;
    let title = meta
        .get("title")
        .and_then(|t| t.get("content"))
        .and_then(|t| t.as_str())
        .unwrap_or_default()
        .to_string();
    // metadataRows[0] = チャンネル名、metadataRows[1] = 再生数・投稿時期
    let rows = meta
        .get("metadata")
        .and_then(|m| m.get("contentMetadataViewModel"))
        .and_then(|m| m.get("metadataRows"))
        .and_then(|r| r.as_array());
    let part_text = |row: usize, part: usize| -> Option<String> {
        rows.and_then(|r| r.get(row))
            .and_then(|r| r.get("metadataParts"))
            .and_then(|p| p.as_array())
            .and_then(|p| p.get(part))
            .and_then(|p| p.get("text"))
            .and_then(|t| t.get("content"))
            .and_then(|t| t.as_str())
            .map(|s| s.to_string())
    };
    // チャンネル ID はアバターの browseEndpoint.browseId（UC〜）に入っている。
    // アバター領域を優先し、見つからなければ lockup 全体をフォールバック走査する
    // （メニュー項目など無関係な browseId の誤拾いを避ける）。
    let channel_id = meta
        .get("image")
        .and_then(find_uc_browse_id)
        .or_else(|| find_uc_browse_id(r));
    Some(SearchResult {
        video_id,
        title,
        channel_id,
        uploader_id: None,
        channel_title: part_text(0, 0),
        duration_sec: find_thumbnail_badge_text(r).and_then(|s| parse_length_text(&s)),
        view_count: part_text(1, 0).and_then(|s| parse_compact_count(&s)),
        thumbnail_url: r
            .get("contentImage")
            .and_then(|c| c.get("thumbnailViewModel"))
            .and_then(|t| t.get("image"))
            .and_then(|i| i.get("sources"))
            .and_then(|a| a.as_array())
            .and_then(|a| a.last())
            .and_then(|t| t.get("url"))
            .and_then(|u| u.as_str())
            .map(|s| s.to_string()),
    })
}

/// ツリー内の `browseEndpoint.browseId` から `UC` 始まりのチャンネル ID を探す。
fn find_uc_browse_id(v: &Value) -> Option<String> {
    if let Value::Object(map) = v {
        if let Some(id) = map
            .get("browseId")
            .and_then(|b| b.as_str())
            .filter(|b| b.starts_with("UC"))
        {
            return Some(id.to_string());
        }
        for value in map.values() {
            if let Some(id) = find_uc_browse_id(value) {
                return Some(id);
            }
        }
    } else if let Value::Array(arr) = v {
        for item in arr {
            if let Some(id) = find_uc_browse_id(item) {
                return Some(id);
            }
        }
    }
    None
}

/// サムネイル下部オーバーレイの `thumbnailBadgeViewModel.text`（動画長や LIVE）を探す。
fn find_thumbnail_badge_text(v: &Value) -> Option<String> {
    if let Value::Object(map) = v {
        if let Some(badge) = map.get("thumbnailBadgeViewModel") {
            if let Some(t) = badge.get("text").and_then(|t| t.as_str()) {
                return Some(t.to_string());
            }
        }
        for value in map.values() {
            if let Some(t) = find_thumbnail_badge_text(value) {
                return Some(t);
            }
        }
    } else if let Value::Array(arr) = v {
        for item in arr {
            if let Some(t) = find_thumbnail_badge_text(item) {
                return Some(t);
            }
        }
    }
    None
}

/// "32K" / "1.2M" / "1,234" のような短縮表記から数値を拾う（InnerTube の
/// metadataParts 表記用）。K=千・M=百万・B=十億の倍率を掛ける。
fn parse_compact_count(s: &str) -> Option<i64> {
    let trimmed = s.trim();
    let (num_part, mult) = match trimmed.chars().last() {
        Some('K') | Some('k') => (&trimmed[..trimmed.len() - 1], 1_000i64),
        Some('M') => (&trimmed[..trimmed.len() - 1], 1_000_000i64),
        Some('B') => (&trimmed[..trimmed.len() - 1], 1_000_000_000i64),
        _ => (trimmed, 1i64),
    };
    let cleaned: String = num_part
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    let n: f64 = cleaned.parse().ok()?;
    Some((n * mult as f64) as i64)
}

/// `{ "simpleText": "..." }` / `{ "runs": [{"text": "..."}] }` のどちらからも文字列を取る。
fn text_of(v: &Value) -> String {
    if let Some(t) = v.get("simpleText").and_then(|s| s.as_str()) {
        return t.to_string();
    }
    v.get("runs")
        .and_then(|r| r.as_array())
        .map(|runs| {
            runs.iter()
                .filter_map(|r| r.get("text").and_then(|t| t.as_str()))
                .collect::<String>()
        })
        .unwrap_or_default()
}

/// "12:34" / "1:02:03" のような長さ表記を秒に変換する。
fn parse_length_text(s: &str) -> Option<i64> {
    let mut total: i64 = 0;
    let mut any = false;
    for part in s.split(':') {
        let n: i64 = part.trim().parse().ok()?;
        total = total * 60 + n;
        any = true;
    }
    any.then_some(total)
}

/// "1,234 回視聴" / "1.2M views" のような表記から数値を拾う。
/// 桁区切り以外の非数字を落として素直にパースする暫定（接頭辞の K/M は拾わない）。
fn parse_view_count(s: &str) -> Option<i64> {
    let digits: String = s.chars().filter(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_cfg_finds_keys() {
        let html = r#"<script>ytcfg.set({"INNERTUBE_API_KEY":"AIzaSyDUMMY","INNERTUBE_CONTEXT_CLIENT_VERSION":"2.20260101.00.00","VISITOR_DATA":"CgsxMjM0NTY3ODkwIw%3D%3D"});</script>"#;
        let cfg = InnerTube::extract_cfg(html).unwrap();
        assert_eq!(cfg.api_key, "AIzaSyDUMMY");
        assert_eq!(cfg.client_version, "2.20260101.00.00");
        assert_eq!(cfg.visitor_data, "CgsxMjM0NTY3ODkwIw%3D%3D");
    }

    #[test]
    fn extract_cfg_missing_key_is_error() {
        let err = InnerTube::extract_cfg("<html></html>").unwrap_err();
        assert!(matches!(
            err,
            InnerTubeError::CfgMissing("INNERTUBE_API_KEY")
        ));
    }

    #[test]
    fn parse_related_extracts_compact_video_renderer() {
        let v = serde_json::json!({
            "contents": {
                "twoColumnWatchNextResults": {
                    "secondaryResults": {
                        "secondaryResults": {
                            "results": [
                                {
                                    "compactVideoRenderer": {
                                        "videoId": "rel123abcde",
                                        "title": {"simpleText": "関連動画A"},
                                        "lengthText": {"simpleText": "10:30"},
                                        "viewCountText": {"simpleText": "123,456 回視聴"},
                                        "shortBylineText": {"runs": [{
                                            "text": "チャンネルA",
                                            "navigationEndpoint": {
                                                "browseEndpoint": {"browseId": "UCchannel0000000000001"}
                                            }
                                        }]},
                                        "thumbnail": {"thumbnails": [
                                            {"url": "https://i.ytimg.com/vi/rel123abcde/default.jpg"},
                                            {"url": "https://i.ytimg.com/vi/rel123abcde/hqdefault.jpg"}
                                        ]}
                                    }
                                },
                                {"compactPlaylistRenderer": {"playlistId": "PLxxx"}}
                            ]
                        }
                    }
                }
            }
        });
        let out = parse_related(&v);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].video_id, "rel123abcde");
        assert_eq!(out[0].title, "関連動画A");
        assert_eq!(out[0].duration_sec, Some(630));
        assert_eq!(out[0].view_count, Some(123456));
        assert_eq!(out[0].channel_id.as_deref(), Some("UCchannel0000000000001"));
        assert_eq!(
            out[0].thumbnail_url.as_deref(),
            Some("https://i.ytimg.com/vi/rel123abcde/hqdefault.jpg")
        );
    }

    /// golden fixture: 実際に取得した `/youtubei/v1/next` 応答（技術方針 O）。
    /// 2026-10 時点の WEB クライアント応答は `lockupViewModel` 形式で、
    /// `LOCKUP_CONTENT_TYPE_PLAYLIST`（Mix 等）は除外する。
    #[test]
    fn parse_related_golden_fixture() {
        let v: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/youtube_next_related.json"
        ))
        .unwrap();
        let out = parse_related(&v);
        assert_eq!(out.len(), 2);
        let first = &out[0];
        assert_eq!(first.video_id, "j4Rj2-bvnQM");
        assert!(first.title.starts_with("80s Greatest Hits"));
        assert_eq!(first.channel_title.as_deref(), Some("Pure 80s Retro"));
        assert_eq!(
            first.channel_id.as_deref(),
            Some("UC_zEaYJqYB67M8DGHk5Xo8A")
        );
        assert_eq!(first.duration_sec, Some(3690)); // "1:01:30"
        assert_eq!(first.view_count, Some(32000)); // "32K"
        assert!(first
            .thumbnail_url
            .as_deref()
            .unwrap_or("")
            .contains("i.ytimg.com/vi/j4Rj2-bvnQM"));
        let second = &out[1];
        assert_eq!(second.video_id, "pLvKkPyDl7M");
        assert_eq!(
            second.channel_id.as_deref(),
            Some("UCSu7x-J8ESHojsq8mCvFOZw")
        );
        assert_eq!(second.duration_sec, Some(848)); // "14:08"
        assert_eq!(second.view_count, Some(156000)); // "156K"
    }

    #[test]
    fn parse_compact_count_variants() {
        assert_eq!(parse_compact_count("32K"), Some(32000));
        assert_eq!(parse_compact_count("1.2M"), Some(1200000));
        assert_eq!(parse_compact_count("3B"), Some(3000000000));
        assert_eq!(parse_compact_count("1,234"), Some(1234));
        assert_eq!(parse_compact_count("views"), None);
        assert_eq!(parse_compact_count(""), None);
    }

    #[test]
    fn parse_length_text_variants() {
        assert_eq!(parse_length_text("3:05"), Some(185));
        assert_eq!(parse_length_text("1:02:03"), Some(3723));
        assert_eq!(parse_length_text(""), None);
        assert_eq!(parse_length_text("LIVE"), None);
    }

    /// FR-24/AQ: 継続トークンの種別判定。`liveChatReplayContinuationData`
    /// はリプレイ、その他（timed/invalidation/reload）はライブ。
    /// 混在時はリプレイを優先する。
    #[test]
    fn next_continuation_detects_kind() {
        let live = serde_json::json!({"continuations": [
            {"timedContinuationData": {"continuation": "LIVETOKEN", "timeoutMs": 8000}}
        ]});
        let (t, ms, k) = next_continuation(&live).unwrap();
        assert_eq!((t.as_str(), ms, k), ("LIVETOKEN", 8000, ContinuationKind::Live));

        let replay = serde_json::json!({"continuations": [
            {"liveChatReplayContinuationData": {"continuation": "REPLAYTOKEN"}}
        ]});
        let (t, _, k) = next_continuation(&replay).unwrap();
        assert_eq!((t.as_str(), k), ("REPLAYTOKEN", ContinuationKind::Replay));

        // 混在（終了直後の切替期）はリプレイ優先
        let mixed = serde_json::json!({"continuations": [
            {"timedContinuationData": {"continuation": "LIVE2", "timeoutMs": 500}},
            {"liveChatReplayContinuationData": {"continuation": "REPLAY2"}}
        ]});
        let (t, _, k) = next_continuation(&mixed).unwrap();
        assert_eq!((t.as_str(), k), ("REPLAY2", ContinuationKind::Replay));

        assert!(next_continuation(&serde_json::json!({})).is_none());
        assert!(next_continuation(&serde_json::json!({"continuations": []})).is_none());
    }

    /// FR-24/AQ: liveBroadcastDetails タイムスタンプの ISO8601 パース。
    #[test]
    fn iso8601_ms_parses_timestamps() {
        // Z 終端
        assert_eq!(iso8601_ms("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(
            iso8601_ms("2026-05-01T12:00:00Z"),
            Some(1_777_636_800_000)
        );
        // 小数秒とオフセット
        assert_eq!(iso8601_ms("1970-01-01T00:00:00.500Z"), Some(500));
        assert_eq!(iso8601_ms("1970-01-01T09:00:00+09:00"), Some(0));
        // 負オフセットは UTC 側へ進める（00:30 - (-00:30) = 01:00Z）
        assert_eq!(iso8601_ms("1970-01-01T00:30:00-00:30"), Some(3_600_000));
        // 非ゼロオフセットの実例（+09:00 は 9 時間前）
        assert_eq!(
            iso8601_ms("2026-05-01T21:00:00+09:00"),
            iso8601_ms("2026-05-01T12:00:00Z")
        );
        // 不正形は None
        assert_eq!(iso8601_ms("2026/05/01"), None);
        assert_eq!(iso8601_ms(""), None);
        assert_eq!(iso8601_ms("not a date"), None);
    }

    /// FR-24/AQ: ytInitialPlayerResponse から放送窓を取る。
    #[test]
    fn extract_broadcast_window_from_player_response() {
        let html = r#"window["ytInitialPlayerResponse"] = {"videoDetails":{"lengthSeconds":"600"},"microformat":{"playerMicroformatRenderer":{"liveBroadcastDetails":{"startTimestamp":"2026-05-01T12:00:00Z","endTimestamp":"2026-05-01T12:15:00Z","isLiveNow":false}}}};"#;
        let w = extract_broadcast_window(html);
        assert_eq!(w.start_ms, Some(1_777_636_800_000));
        assert_eq!(w.end_ms, Some(1_777_637_700_000));
        assert_eq!(w.length_secs, Some(600));
        assert_eq!(w.is_live_now, Some(false));

        // 情報の無い HTML は全て None
        let w = extract_broadcast_window("<html>no data</html>");
        assert!(w.start_ms.is_none() && w.end_ms.is_none() && w.length_secs.is_none());
        assert!(w.is_live_now.is_none());
    }

    /// FR-24/AQ: 終了済み判定（リプレイ経路のルーティング）。
    /// `isLiveNow` が優先、欠落時は `endTimestamp` の有無で判断する。
    #[test]
    fn broadcast_window_is_ended() {
        let w = |is_live_now: Option<bool>, end_ms: Option<i64>| BroadcastWindow {
            is_live_now,
            end_ms,
            ..Default::default()
        };
        // 実機確認値: 終了済みは isLiveNow:false + endTimestamp あり
        assert!(w(Some(false), Some(1)).is_ended());
        assert!(!w(Some(true), None).is_ended());
        // 予約配信のロビーは isLiveNow:false でも endTimestamp が無い
        assert!(!w(Some(false), None).is_ended());
        // isLiveNow 欠落時は endTimestamp の有無で判断
        assert!(w(None, Some(1)).is_ended());
        assert!(!w(None, None).is_ended());
        // isLiveNow:true だが end がある矛盾形は「放送中」を優先
        assert!(!w(Some(true), Some(1)).is_ended());
    }
}
