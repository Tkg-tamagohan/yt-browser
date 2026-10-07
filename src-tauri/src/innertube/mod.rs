//! InnerTube クライアント（設計書 §6.1、FR-4）。
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
}
