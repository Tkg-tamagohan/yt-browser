//! SponsorBlock（設計書 §4.4、技術方針 N）。
//!
//! `GET https://sponsor.ajay.app/api/skipSegments` で区間を取得し、
//! `time-pos` の監視で `[start, end)` に入ったら seek で飛ばして `sponsor://skipped` を発火する。
//! カテゴリごとの「スキップ / 通知のみ / 無効」は設定 `sponsor.categories`（JSON マップ）で持つ。

use std::collections::HashMap;
use std::time::Duration;

use serde::Deserialize;
use thiserror::Error;

/// SponsorBlock API のエンドポイント（公式公開インスタンス）。
pub const API_BASE: &str = "https://sponsor.ajay.app";

/// 設定キー: カテゴリごとの動作（JSON マップ `{"sponsor":"skip",...}`）。
pub const SETTING_CATEGORIES: &str = "sponsor.categories";

const FETCH_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Error)]
pub enum SponsorError {
    #[error("SponsorBlock API への接続に失敗: {0}")]
    Http(#[from] reqwest::Error),
}

/// カテゴリごとの動作。既定は `Off`（何もしない）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CategoryAction {
    /// 区間に入ったら末尾へ seek してスキップする。
    Skip,
    /// スキップせず通知だけ出す。
    Notify,
    /// 何もしない。
    #[default]
    Off,
}

impl CategoryAction {
    fn parse(s: &str) -> Self {
        match s {
            "skip" => Self::Skip,
            "notify" => Self::Notify,
            _ => Self::Off,
        }
    }
}

/// API レスポンス 1 件分。
#[derive(Debug, Clone, Deserialize)]
struct ApiSegment {
    category: String,
    segment: [f64; 2],
}

/// 取得済み区間と動作（再生インスタンスごとに保持する判定用の形）。
#[derive(Debug, Clone)]
pub struct ActiveSegment {
    pub category: String,
    pub start: f64,
    pub end: f64,
    pub action: CategoryAction,
}

/// 未発火の候補のうち `pos` が入る最初の区間のインデックス。
/// 境界は `[start, end)`。発火済み区間とバックオフ中の区間は除外するので、
/// 区間同士が重なっていても後の候補に到達できる。
pub fn next_candidate(
    segments: &[ActiveSegment],
    fired: &std::collections::HashSet<usize>,
    backoff: &HashMap<usize, std::time::Instant>,
    pos: f64,
) -> Option<usize> {
    let now = std::time::Instant::now();
    (0..segments.len()).find(|&i| {
        let s = &segments[i];
        s.action != CategoryAction::Off
            && pos >= s.start
            && pos < s.end
            && !fired.contains(&i)
            && backoff.get(&i).is_none_or(|until| now >= *until)
    })
}

/// 設定 `sponsor.categories` の値を解析してカテゴリ→動作のマップを返す。
/// 未設定・空なら既定（sponsor のみスキップ）を返す。
pub fn parse_categories(raw: Option<&str>) -> HashMap<String, CategoryAction> {
    let Some(raw) = raw.filter(|s| !s.trim().is_empty()) else {
        return default_categories();
    };
    match serde_json::from_str::<HashMap<String, String>>(raw) {
        Ok(map) => map
            .into_iter()
            .map(|(k, v)| (k, CategoryAction::parse(&v)))
            .collect(),
        Err(e) => {
            tracing::warn!(error = %e, "sponsor.categories の解析に失敗。既定を使用");
            default_categories()
        }
    }
}

/// 既定のカテゴリ設定: sponsor のみスキップ（SponsorBlock 拡張の既定に倣う）。
pub fn default_categories() -> HashMap<String, CategoryAction> {
    let mut m = HashMap::new();
    m.insert("sponsor".to_string(), CategoryAction::Skip);
    m
}

/// 動作が Off でないカテゴリだけを API に問い合わせる。
/// 404（区間なし）は空リストとして扱う。取得失敗は呼び出し側でログに留め、再生を阻害しない。
pub async fn fetch_segments(
    client: &reqwest::Client,
    video_id: &str,
    categories: &HashMap<String, CategoryAction>,
) -> Result<Vec<ActiveSegment>, SponsorError> {
    let wanted: Vec<(&str, CategoryAction)> = categories
        .iter()
        .filter(|(_, a)| **a != CategoryAction::Off)
        .map(|(k, a)| (k.as_str(), *a))
        .collect();
    if wanted.is_empty() {
        return Ok(Vec::new());
    }
    let cats: Vec<&str> = wanted.iter().map(|(c, _)| *c).collect();
    let url = format!("{API_BASE}/api/skipSegments");
    let resp = client
        .get(url)
        .query(&[
            ("videoID", video_id),
            (
                "categories",
                &serde_json::to_string(&cats).unwrap_or_default(),
            ),
        ])
        .timeout(FETCH_TIMEOUT)
        .send()
        .await?;
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(Vec::new());
    }
    let segs = resp.error_for_status()?.json::<Vec<ApiSegment>>().await?;
    let by_cat: HashMap<&str, CategoryAction> = wanted.into_iter().collect();
    Ok(segs
        .into_iter()
        .filter_map(|s| {
            by_cat.get(s.category.as_str()).map(|action| ActiveSegment {
                category: s.category,
                start: s.segment[0],
                end: s.segment[1],
                action: *action,
            })
        })
        .collect())
}

/// `sponsor://skipped` / 通知イベントのペイロード（設計書 §3.2）。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkippedPayload {
    pub instance_id: u32,
    pub video_id: String,
    pub category: String,
    /// `[start, end]`。
    pub segment: [f64; 2],
    /// "skip" | "notify"。
    pub action: &'static str,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(start: f64, end: f64, action: CategoryAction) -> ActiveSegment {
        ActiveSegment {
            category: "sponsor".into(),
            start,
            end,
            action,
        }
    }

    fn no_fired() -> (
        std::collections::HashSet<usize>,
        HashMap<usize, std::time::Instant>,
    ) {
        (std::collections::HashSet::new(), HashMap::new())
    }

    #[test]
    fn next_candidate_returns_segment_inside_half_open_range() {
        let (f, b) = no_fired();
        let segs = vec![seg(10.0, 20.0, CategoryAction::Skip)];
        assert!(next_candidate(&segs, &f, &b, 15.0).is_some());
        assert!(next_candidate(&segs, &f, &b, 10.0).is_some()); // start 境界は含む
        assert!(next_candidate(&segs, &f, &b, 20.0).is_none()); // end 境界は含まない
        assert!(next_candidate(&segs, &f, &b, 9.9).is_none());
    }

    #[test]
    fn next_candidate_ignores_off_categories() {
        let (f, b) = no_fired();
        let segs = vec![seg(10.0, 20.0, CategoryAction::Off)];
        assert!(next_candidate(&segs, &f, &b, 15.0).is_none());
    }

    #[test]
    fn next_candidate_returns_first_matching() {
        let (f, b) = no_fired();
        let segs = vec![
            seg(10.0, 20.0, CategoryAction::Notify),
            seg(15.0, 25.0, CategoryAction::Skip),
        ];
        let idx = next_candidate(&segs, &f, &b, 16.0).unwrap();
        assert_eq!(segs[idx].action, CategoryAction::Notify);
    }

    #[test]
    fn next_candidate_skips_fired_segment_to_reach_overlapped() {
        // 通知区間とスキップ区間が重なるとき、発火済みの前者を越えて後者に到達する
        let (mut f, b) = no_fired();
        let segs = vec![
            seg(10.0, 30.0, CategoryAction::Notify),
            seg(15.0, 20.0, CategoryAction::Skip),
        ];
        f.insert(0);
        let idx = next_candidate(&segs, &f, &b, 16.0).unwrap();
        assert_eq!(segs[idx].action, CategoryAction::Skip);
    }

    #[test]
    fn next_candidate_skips_backed_off_segment() {
        let (f, mut b) = no_fired();
        let segs = vec![seg(10.0, 20.0, CategoryAction::Skip)];
        b.insert(0, std::time::Instant::now() + Duration::from_secs(60));
        assert!(next_candidate(&segs, &f, &b, 15.0).is_none());
        b.insert(0, std::time::Instant::now() - Duration::from_secs(1));
        assert!(next_candidate(&segs, &f, &b, 15.0).is_some());
    }

    #[test]
    fn parse_categories_defaults_to_sponsor_skip() {
        let m = parse_categories(None);
        assert_eq!(m.get("sponsor"), Some(&CategoryAction::Skip));
        assert_eq!(m.get("intro"), None);
        let m = parse_categories(Some(""));
        assert_eq!(m.get("sponsor"), Some(&CategoryAction::Skip));
    }

    #[test]
    fn parse_categories_reads_json_map() {
        let m = parse_categories(Some(r#"{"sponsor":"notify","intro":"skip","outro":"off"}"#));
        assert_eq!(m.get("sponsor"), Some(&CategoryAction::Notify));
        assert_eq!(m.get("intro"), Some(&CategoryAction::Skip));
        assert_eq!(m.get("outro"), Some(&CategoryAction::Off));
    }

    #[test]
    fn parse_categories_falls_back_on_bad_json() {
        let m = parse_categories(Some("not json"));
        assert_eq!(m.get("sponsor"), Some(&CategoryAction::Skip));
    }
}
