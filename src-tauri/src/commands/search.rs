//! 検索・関連動画系コマンド（設計書 §3.1、FR-4）。

use std::sync::Arc;

use tauri::State;

use super::parse_video_id;
use crate::db::Db;
use crate::error::UiError;
use crate::yt::{self, YtDlpResolver};

/// `search`（設計書 §3.1、FR-4）。`yt-dlp ytsearch` の結果から
/// ブロック済みチャンネルと動画系 NG フィルタを除いて返す。
/// `count` は返す表示件数の上限（省略時 `SEARCH_LIMIT`）。
/// ytsearch に継続が無いため「さらに読み込む」は count を増やした
/// 再取得で行い、既表示との差分は画面側で追記する（FR-25、仕様決定 AR）。
#[tauri::command]
pub async fn search(
    query: String,
    count: Option<u32>,
    db: State<'_, Db>,
    resolver: State<'_, YtDlpResolver>,
    ng: State<'_, Arc<crate::filter::NgMatcher>>,
) -> Result<Vec<crate::model::SearchResult>, UiError> {
    let q = query.trim().to_string();
    if q.is_empty() {
        return Err(UiError::invalid_input("検索語が空です"));
    }
    if q.len() > 256 {
        return Err(UiError::invalid_input("検索語が長すぎます"));
    }
    let count = count.unwrap_or(SEARCH_LIMIT).clamp(1, SEARCH_COUNT_MAX);
    let path = resolver.resolve(&db).await.ok_or(yt::YtError::NotFound)?;
    let mut results = yt::search(&path, &q, count * SEARCH_FETCH_FACTOR).await?;
    let blocked = db.blocked_ids()?;
    let matcher = ng.get();
    results.retain(|r| {
        r.channel_id
            .as_deref()
            .map(|c| !blocked.contains(c))
            .unwrap_or(true)
            && !matcher.is_video_ng(
                &r.title,
                r.channel_title.as_deref(),
                r.channel_id.as_deref(),
            )
    });
    results.truncate(count as usize);
    Ok(results)
}

/// 検索の既定表示件数（設計書 §5 の `ytsearch<N>`）。
const SEARCH_LIMIT: u32 = 20;
/// `count` 引数の上限。「さらに読み込む」の増分はこの範囲で行う。
const SEARCH_COUNT_MAX: u32 = 500;
/// ytsearch のフェッチ倍率。ブロック・NG フィルタで抜けた分を
/// 後続候補で埋めるため、表示件数の 3 倍を取ってから絞る。
const SEARCH_FETCH_FACTOR: u32 = 3;

/// `get_related`（設計書 §3.1、FR-4）。InnerTube `next` の関連動画から
/// ブロック済みチャンネルと動画系 NG フィルタを除いて返す。
#[tauri::command]
pub async fn get_related(
    video_id: String,
    db: State<'_, Db>,
    innertube: State<'_, Arc<crate::innertube::InnerTube>>,
    ng: State<'_, Arc<crate::filter::NgMatcher>>,
) -> Result<Vec<crate::model::SearchResult>, UiError> {
    let video_id = parse_video_id(&video_id)?;
    let mut results = innertube.related(&video_id).await?;
    let blocked = db.blocked_ids()?;
    let matcher = ng.get();
    results.retain(|r| {
        r.channel_id
            .as_deref()
            .map(|c| !blocked.contains(c))
            .unwrap_or(true)
            && !matcher.is_video_ng(
                &r.title,
                r.channel_title.as_deref(),
                r.channel_id.as_deref(),
            )
    });
    Ok(results)
}
