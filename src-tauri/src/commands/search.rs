//! 検索・関連動画系コマンド（設計書 §3.1、FR-4・FR-100）。

use std::collections::HashSet;
use std::sync::Arc;

use tauri::State;

use super::parse_video_id;
use crate::db::Db;
use crate::error::UiError;
use crate::filter::{Matcher, NgMatcher};
use crate::model::{is_channel_id, SearchPage, SearchResult};
use crate::yt::{self, YtDlpResolver};
use crate::ytapi::{QuotaBucket, YtApi, YtVideoItem, SEARCH_MAX_PAGES_PER_CALL, VIDEOS_BATCH_MAX};

/// `search`（設計書 §3.1、FR-4・FR-100・仕様決定 EF）。
/// `youtube.api_key` が有効な間は Data API v3 `SearchSession`（`search.list` +
/// `videos.list` 併用・10 分 TTL）を優先し、未設定・無効キー・クォータ上限・障害時は
/// `yt-dlp ytsearch` + 条件付き `videos.list` 補強へ透過フォールバックする。
/// `count` は返す表示件数の上限（省略時 `SEARCH_LIMIT`）。
/// 「さらに読み込む」は累積 `count` 増加で呼び出し、キャッシュ済み生存数が満たせば
/// 追加通信なしで先頭 `[0..count]` を返す。2 ページ上限やフィルタで `items.len() < count`
/// となっても次ページやバッファ残余があれば `has_more: true` を返す（FR-25・FR-100）。
#[tauri::command]
pub async fn search(
    query: String,
    count: Option<u32>,
    db: State<'_, Db>,
    ytapi: State<'_, Arc<YtApi>>,
    resolver: State<'_, YtDlpResolver>,
    ng: State<'_, Arc<NgMatcher>>,
) -> Result<SearchPage, UiError> {
    let matcher = ng.get();
    search_with(&query, count, &db, &ytapi, &resolver, &matcher).await
}

pub(crate) async fn search_with(
    query: &str,
    count: Option<u32>,
    db: &Db,
    ytapi: &YtApi,
    resolver: &YtDlpResolver,
    matcher: &Matcher,
) -> Result<SearchPage, UiError> {
    let q = query.trim().to_string();
    if q.is_empty() {
        return Err(UiError::invalid_input("検索語が空です"));
    }
    if q.len() > 256 {
        return Err(UiError::invalid_input("検索語が長すぎます"));
    }
    let count = count.unwrap_or(SEARCH_LIMIT).clamp(1, SEARCH_COUNT_MAX);
    let blocked = db.blocked_ids()?;

    match ytapi
        .search_videos(&q, SEARCH_MAX_PAGES_PER_CALL, |items| {
            items
                .iter()
                .filter(|it| is_api_item_allowed(it, &blocked, matcher))
                .count()
                >= count as usize
        })
        .await
    {
        Ok(outcome) => {
            let mut seen = HashSet::new();
            let mut results: Vec<SearchResult> = outcome
                .items
                .into_iter()
                .filter(|it| is_api_item_allowed(it, &blocked, matcher))
                .filter(|it| seen.insert(it.video_id.clone()))
                .map(|it| it.to_search_result())
                .collect();
            let has_buffered_more = results.len() > count as usize;
            results.truncate(count as usize);
            return Ok(SearchPage {
                items: results,
                has_more: (has_buffered_more || outcome.has_more) && count < SEARCH_COUNT_MAX,
            });
        }
        Err(e) => {
            let _ = e.should_fallback();
            tracing::debug!(error = %e, "YtApi search.list に失敗したため yt-dlp ytsearch へフォールバック");
        }
    }

    let path = resolver.resolve(db).await.ok_or(yt::YtError::NotFound)?;
    let raw = yt::search(
        &path,
        &q,
        count * SEARCH_FETCH_FACTOR,
        yt::po_token_setting(db).as_deref(),
    )
    .await?;
    let raw_len = raw.len();
    let items =
        filter_and_enrich_fallback_results(raw, count as usize, &blocked, matcher, ytapi).await;
    let has_more = (items.len() >= count as usize
        || raw_len >= (count * SEARCH_FETCH_FACTOR) as usize)
        && count < SEARCH_COUNT_MAX;
    Ok(SearchPage { items, has_more })
}

fn is_api_item_allowed(item: &YtVideoItem, blocked: &HashSet<String>, matcher: &Matcher) -> bool {
    let channel_id = item
        .video_ref
        .channel_id
        .as_deref()
        .filter(|s| is_channel_id(s));
    let not_blocked = channel_id.map(|c| !blocked.contains(c)).unwrap_or(true);
    not_blocked
        && !matcher.is_video_ng(
            &item.video_ref.title,
            item.video_ref.channel_title.as_deref(),
            channel_id,
            Some(item.meta.description.as_str()),
        )
}

fn is_fallback_pre_allowed(r: &SearchResult, blocked: &HashSet<String>, matcher: &Matcher) -> bool {
    let not_blocked = r
        .channel_id
        .as_deref()
        .map(|c| !blocked.contains(c))
        .unwrap_or(true);
    not_blocked
        && !matcher.is_video_ng(
            &r.title,
            r.channel_title.as_deref(),
            r.channel_id.as_deref(),
            None,
        )
}

fn needs_fallback_enrichment(
    r: &SearchResult,
    blocked: &HashSet<String>,
    matcher: &Matcher,
) -> bool {
    matcher.has_video_desc_rules()
        || (r.channel_id.is_none() && (!blocked.is_empty() || matcher.has_channel_id_rules()))
        || (r.channel_title.is_none() && matcher.has_channel_title_rules())
}

async fn filter_and_enrich_fallback_results(
    raw: Vec<SearchResult>,
    count: usize,
    blocked: &HashSet<String>,
    matcher: &Matcher,
    ytapi: &YtApi,
) -> Vec<SearchResult> {
    let mut seen = HashSet::new();
    let pre_filtered: Vec<SearchResult> = raw
        .into_iter()
        .filter(|r| seen.insert(r.video_id.clone()))
        .filter(|r| is_fallback_pre_allowed(r, blocked, matcher))
        .collect();

    let mut survivors: Vec<SearchResult> = Vec::with_capacity(count.min(pre_filtered.len()));
    let mut enrichment_enabled = ytapi.can_use(QuotaBucket::Shared);
    let mut idx = 0usize;

    while idx < pre_filtered.len() && survivors.len() < count {
        if !enrichment_enabled || !needs_fallback_enrichment(&pre_filtered[idx], blocked, matcher) {
            survivors.push(pre_filtered[idx].clone());
            idx += 1;
            continue;
        }

        let mut end = idx;
        let mut batch_ids: Vec<&str> = Vec::new();
        while end < pre_filtered.len() && batch_ids.len() < VIDEOS_BATCH_MAX {
            if needs_fallback_enrichment(&pre_filtered[end], blocked, matcher) {
                batch_ids.push(pre_filtered[end].video_id.as_str());
            }
            end += 1;
        }

        match ytapi.videos_list(&batch_ids).await {
            Ok(map) => {
                for r in &pre_filtered[idx..end] {
                    if survivors.len() >= count {
                        break;
                    }
                    let mut cand = r.clone();
                    if needs_fallback_enrichment(&cand, blocked, matcher) {
                        if let Some(item) = map.get(&cand.video_id) {
                            if let Some(cid) = item
                                .video_ref
                                .channel_id
                                .as_deref()
                                .filter(|s| is_channel_id(s))
                            {
                                cand.channel_id = Some(cid.to_string());
                            }
                            if let Some(ct) = item.video_ref.channel_title.as_deref() {
                                cand.channel_title = Some(ct.to_string());
                            }
                            if item.duration_sec.is_some() {
                                cand.duration_sec = item.duration_sec;
                            }
                            if item.view_count.is_some() {
                                cand.view_count = item.view_count;
                            }
                            if item.video_ref.thumbnail_url.is_some() {
                                cand.thumbnail_url = item.video_ref.thumbnail_url.clone();
                            }
                            let not_blocked = cand
                                .channel_id
                                .as_deref()
                                .map(|c| !blocked.contains(c))
                                .unwrap_or(true);
                            let ng = matcher.is_video_ng(
                                &cand.title,
                                cand.channel_title.as_deref(),
                                cand.channel_id.as_deref(),
                                Some(item.meta.description.as_str()),
                            );
                            if not_blocked && !ng {
                                survivors.push(cand);
                            }
                        } else {
                            survivors.push(cand);
                        }
                    } else {
                        survivors.push(cand);
                    }
                }
            }
            Err(e) => {
                let _ = e.should_fallback();
                tracing::debug!(
                    error = %e,
                    "yt::search フォールバック時の videos.list 補強に失敗したため無補強で続行"
                );
                enrichment_enabled = false;
                for r in &pre_filtered[idx..end] {
                    if survivors.len() >= count {
                        break;
                    }
                    survivors.push(r.clone());
                }
            }
        }

        idx = end;
    }

    survivors
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
            // InnerTube next の応答にも説明文フィールドは無いため
            // video_desc は評価しない（search と同じ暫定解釈）
            && !matcher.is_video_ng(
                &r.title,
                r.channel_title.as_deref(),
                r.channel_id.as_deref(),
                None,
            )
    });
    Ok(results)
}

/// `get_description`（設計書 §3.1、FR-33・FR-98、仕様決定 BB・ED）。
/// YouTube Data API v3 の利用が可能なら `videos.list` を優先し、未設定・障害時は
/// InnerTube `player` の `videoDetails.shortDescription` へ透過フォールバックする。
/// 概要欄パネルが開くたびと再生中動画の切替で呼ばれ、結果はキャッシュしない。
#[tauri::command]
pub async fn get_description(
    video_id: String,
    ytapi: State<'_, Arc<crate::ytapi::YtApi>>,
    innertube: State<'_, Arc<crate::innertube::InnerTube>>,
) -> Result<String, UiError> {
    let video_id = parse_video_id(&video_id)?;
    fetch_description(&video_id, &ytapi, &innertube).await
}

async fn fetch_description(
    video_id: &str,
    ytapi: &crate::ytapi::YtApi,
    innertube: &crate::innertube::InnerTube,
) -> Result<String, UiError> {
    if ytapi.can_use(crate::ytapi::QuotaBucket::Shared) {
        match ytapi.videos_list(&[video_id]).await {
            Ok(mut map) => {
                if let Some(item) = map.remove(video_id) {
                    return Ok(item.meta.description);
                }
            }
            Err(e) => {
                let _ = e.should_fallback();
            }
        }
    }
    Ok(innertube.description(video_id).await?)
}

#[cfg(test)]
mod tests {
    use super::fetch_description;
    use crate::innertube::InnerTube;
    use crate::ytapi::YtApi;
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn fetch_description_prefers_ytapi_and_falls_back_to_innertube() {
        let yt_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let yt_addr = yt_listener.local_addr().unwrap();
        let yt_srv = tokio::spawn(async move {
            let mut count = 0usize;
            while let Ok((mut stream, _)) = yt_listener.accept().await {
                count += 1;
                let mut buf = [0u8; 2048];
                let _ = stream.read(&mut buf).await;
                let resp = match count {
                    1 => {
                        let body = r#"{"items":[{"id":"dQw4w9WgXcQ","snippet":{"title":"T","description":"From YtApi"}}]}"#;
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            body.len()
                        )
                    }
                    2 => {
                        let body = r#"{"items":[{"id":"dQw4w9WgXcQ","snippet":{"title":"T","description":""}}]}"#;
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            body.len()
                        )
                    }
                    3 => {
                        let body = r#"{"items":[]}"#;
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            body.len()
                        )
                    }
                    _ => "HTTP/1.1 500 Internal Server Error\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}".to_string(),
                };
                let _ = stream.write_all(resp.as_bytes()).await;
            }
        });

        let tube_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let tube_addr = tube_listener.local_addr().unwrap();
        let tube_srv = tokio::spawn(async move {
            let html = r#"<script>ytcfg.set({"INNERTUBE_API_KEY":"k","INNERTUBE_CONTEXT_CLIENT_VERSION":"v","VISITOR_DATA":"d"});</script>"#;
            while let Ok((mut stream, _)) = tube_listener.accept().await {
                let mut buf = [0u8; 2048];
                let n = stream.read(&mut buf).await.unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]);
                let resp = if req.starts_with("GET ") {
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{html}",
                        html.len()
                    )
                } else {
                    let body = r#"{"videoDetails":{"videoId":"dQw4w9WgXcQ","shortDescription":"From InnerTube"}}"#;
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                };
                let _ = stream.write_all(resp.as_bytes()).await;
            }
        });

        let ytapi = YtApi::with_test_base_url(
            Some("AIzaSyDESC_KEY".to_string()),
            format!("http://{yt_addr}"),
        );
        let tube = InnerTube::with_test_urls(
            format!("http://{tube_addr}/watch"),
            format!("http://{tube_addr}/api"),
            Duration::from_secs(5),
        );

        assert_eq!(
            fetch_description("dQw4w9WgXcQ", &ytapi, &tube)
                .await
                .unwrap(),
            "From YtApi"
        );
        // 空説明文はそのまま "" を返し InnerTube へフォールバックしない
        assert_eq!(
            fetch_description("dQw4w9WgXcQ", &ytapi, &tube)
                .await
                .unwrap(),
            ""
        );
        // videos.list 200 応答から動画が欠落した場合は InnerTube へフォールバックする
        assert_eq!(
            fetch_description("dQw4w9WgXcQ", &ytapi, &tube)
                .await
                .unwrap(),
            "From InnerTube"
        );
        // 通信/サーバ障害時は InnerTube へフォールバックする
        assert_eq!(
            fetch_description("dQw4w9WgXcQ", &ytapi, &tube)
                .await
                .unwrap(),
            "From InnerTube"
        );

        yt_srv.abort();
        tube_srv.abort();
    }

    struct TempFile(std::path::PathBuf);
    impl Drop for TempFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    fn write_fake_ytsearch_script(tag: &str, jsonl_body: &str) -> (TempFile, TempFile) {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let pid = std::process::id();
        let data_path =
            std::env::temp_dir().join(format!("ytb_search_data_{tag}_{pid}_{nonce}.jsonl"));
        std::fs::write(&data_path, jsonl_body).unwrap();

        #[cfg(windows)]
        let script_path =
            std::env::temp_dir().join(format!("ytb_fake_search_{tag}_{pid}_{nonce}.cmd"));
        #[cfg(not(windows))]
        let script_path =
            std::env::temp_dir().join(format!("ytb_fake_search_{tag}_{pid}_{nonce}.sh"));

        #[cfg(windows)]
        {
            let body = format!("@echo off\r\ntype \"{}\"\r\n", data_path.display());
            std::fs::write(&script_path, body).unwrap();
        }
        #[cfg(not(windows))]
        {
            use std::os::unix::fs::PermissionsExt;
            let body = format!("#!/bin/sh\ncat \"{}\"\n", data_path.display());
            std::fs::write(&script_path, body).unwrap();
            std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        (TempFile(script_path), TempFile(data_path))
    }

    async fn spawn_search_mock(
        replies: Vec<(u16, String)>,
    ) -> (
        String,
        std::sync::Arc<std::sync::Mutex<Vec<String>>>,
        tokio::task::JoinHandle<()>,
    ) {
        use std::collections::VecDeque;
        use std::sync::{Arc as StdArc, Mutex as StdMutex};

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let recorded: StdArc<StdMutex<Vec<String>>> = StdArc::new(StdMutex::new(Vec::new()));
        let rec_clone = StdArc::clone(&recorded);
        let queue: StdArc<StdMutex<VecDeque<(u16, String)>>> =
            StdArc::new(StdMutex::new(VecDeque::from(replies)));

        let srv = tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };
                let rec = StdArc::clone(&rec_clone);
                let q = StdArc::clone(&queue);
                tokio::spawn(async move {
                    let mut buf = [0u8; 4096];
                    let n = stream.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]);
                    let line = req.lines().next().unwrap_or_default().to_string();
                    rec.lock().unwrap().push(line);
                    let (status, body) = q
                        .lock()
                        .unwrap()
                        .pop_front()
                        .unwrap_or((200, r#"{"items":[]}"#.to_string()));
                    let reason = match status {
                        200 => "OK",
                        403 => "Forbidden",
                        500 => "Internal Server Error",
                        _ => "Error",
                    };
                    let resp = format!(
                        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = stream.write_all(resp.as_bytes()).await;
                    let _ = stream.shutdown().await;
                });
            }
        });
        (format!("http://{addr}"), recorded, srv)
    }

    #[tokio::test]
    async fn search_with_prefers_ytapi_session_and_applies_filters_without_ytdlp() {
        use super::search_with;
        use crate::db::Db;
        use crate::filter::Matcher;
        use crate::model::Filter;
        use crate::yt::{YtDlpResolver, SETTING_YTDLP_PATH};

        let db = Db::connect_in_memory().unwrap();
        // yt-dlp 不在環境でも SearchSession 優先経路が動作することを確認する
        db.setting_set(SETTING_YTDLP_PATH, "/nonexistent/yt-dlp-should-not-run")
            .unwrap();
        db.blocked_add("UCblocked000000000000001", "Blocked CH")
            .unwrap();

        let matcher = Matcher::rebuild(&[Filter {
            id: 1,
            target: "video_desc".to_string(),
            kind: "literal".to_string(),
            pattern: "案件リンク".to_string(),
            enabled: true,
            created_at: String::new(),
        }]);

        let (base, recorded, srv) = spawn_search_mock(vec![
            // Page 1: ブロック 1 件、desc NG 1 件、生存 2 件（vidApi00003, vidApi00004）
            (
                200,
                r#"{"nextPageToken":"p2","items":[
                    {"id":{"videoId":"vidApi00001"}},
                    {"id":{"videoId":"vidApi00002"}},
                    {"id":{"videoId":"vidApi00003"}},
                    {"id":{"videoId":"vidApi00004"}}
                ]}"#
                .to_string(),
            ),
            (
                200,
                r#"{"items":[
                    {"id":"vidApi00001","snippet":{"title":"Blocked","channelId":"UCblocked000000000000001","description":"ok"}},
                    {"id":"vidApi00002","snippet":{"title":"Sponsored","channelId":"UCgood000000000000000002","description":"詳細は案件リンクへ"}},
                    {"id":"vidApi00003","snippet":{"title":"Good 1","channelId":"UCgood000000000000000003","channelTitle":"Good CH 3","description":"normal"},"contentDetails":{"duration":"PT1M30S"},"statistics":{"viewCount":"100"}},
                    {"id":"vidApi00004","snippet":{"title":"Good 2","channelId":"UCgood000000000000000004","channelTitle":"Good CH 4","description":"clean"},"contentDetails":{"duration":"PT2M"},"statistics":{"viewCount":"200"}}
                ]}"#
                .to_string(),
            ),
            // Page 2: 追加の生存 2 件（count=3 要求時に 1 ページ追加して truncate(3)）
            (
                200,
                r#"{"items":[
                    {"id":{"videoId":"vidApi00005"}},
                    {"id":{"videoId":"vidApi00006"}}
                ]}"#
                .to_string(),
            ),
            (
                200,
                r#"{"items":[
                    {"id":"vidApi00005","snippet":{"title":"Good 3","channelId":"UCgood000000000000000005","description":"clean"}},
                    {"id":"vidApi00006","snippet":{"title":"Good 4","channelId":"UCgood000000000000000006","description":"clean"}}
                ]}"#
                .to_string(),
            ),
        ])
        .await;

        let ytapi = YtApi::with_test_base_url(Some("AIzaSyCMD_SEARCH".to_string()), base);
        let resolver = YtDlpResolver::new(None);

        // 1) 初回 count=2 -> フィルタ後 2 件で page 2 を呼ばずに完了（nextPageToken があるため has_more=true）
        let page1 = search_with("rust", Some(2), &db, &ytapi, &resolver, &matcher)
            .await
            .unwrap();
        assert!(page1.has_more);
        let res1 = page1.items;
        assert_eq!(res1.len(), 2);
        assert_eq!(res1[0].video_id, "vidApi00003");
        assert_eq!(res1[0].view_count, Some(100));
        assert_eq!(res1[1].video_id, "vidApi00004");
        assert_eq!(recorded.lock().unwrap().len(), 2);

        // 2) 同一クエリ count=1 -> 0 HTTP キャッシュヒット + truncate(1)
        let page_cached = search_with("rust", Some(1), &db, &ytapi, &resolver, &matcher)
            .await
            .unwrap();
        assert!(page_cached.has_more);
        let res_cached = page_cached.items;
        assert_eq!(res_cached.len(), 1);
        assert_eq!(res_cached[0].video_id, "vidApi00003");
        assert_eq!(recorded.lock().unwrap().len(), 2);

        // 3) 同一クエリ count=3 -> Page 2 を取得し truncate(3)（バッファ残余 1 件があるため has_more=true）
        let page_more = search_with("rust", Some(3), &db, &ytapi, &resolver, &matcher)
            .await
            .unwrap();
        assert!(page_more.has_more);
        let res_more = page_more.items;
        assert_eq!(res_more.len(), 3);
        assert_eq!(res_more[2].video_id, "vidApi00005");
        assert_eq!(recorded.lock().unwrap().len(), 4);

        // 4) 同一クエリ count=4 -> バッファ残余を返し終端のため has_more=false
        let page_end = search_with("rust", Some(4), &db, &ytapi, &resolver, &matcher)
            .await
            .unwrap();
        assert!(!page_end.has_more);
        assert_eq!(page_end.items.len(), 4);
        assert_eq!(recorded.lock().unwrap().len(), 4);

        srv.abort();
    }

    #[tokio::test]
    async fn search_with_keeps_has_more_true_when_2_page_cap_returns_fewer_than_count_with_next_token(
    ) {
        use super::search_with;
        use crate::db::Db;
        use crate::filter::Matcher;
        use crate::model::Filter;
        use crate::yt::YtDlpResolver;

        let db = Db::connect_in_memory().unwrap();
        let matcher = Matcher::rebuild(&[Filter {
            id: 1,
            target: "video_desc".to_string(),
            kind: "literal".to_string(),
            pattern: "除外".to_string(),
            enabled: true,
            created_at: String::new(),
        }]);

        let (base, recorded, srv) = spawn_search_mock(vec![
            // Page 1: NG で 1 件落ち、1 件生存、nextPageToken=p2
            (
                200,
                r#"{"nextPageToken":"p2","items":[{"id":{"videoId":"vidShort001"}},{"id":{"videoId":"vidShort002"}}]}"#.to_string(),
            ),
            (
                200,
                r#"{"items":[
                    {"id":"vidShort001","snippet":{"title":"Drop 1","channelId":"UCsh00000000000000000001","description":"除外ワード"}},
                    {"id":"vidShort002","snippet":{"title":"Keep 1","channelId":"UCsh00000000000000000002","description":"正常"}}
                ]}"#.to_string(),
            ),
            // Page 2: 0 件生存、nextPageToken=p3（2 ページ上限でここで打ち切り）
            (
                200,
                r#"{"nextPageToken":"p3","items":[{"id":{"videoId":"vidShort003"}}]}"#.to_string(),
            ),
            (
                200,
                r#"{"items":[
                    {"id":"vidShort003","snippet":{"title":"Drop 2","channelId":"UCsh00000000000000000003","description":"除外ワード"}}
                ]}"#.to_string(),
            ),
            // Page 3: 1 件生存、nextPageToken なし（終端）
            (
                200,
                r#"{"items":[{"id":{"videoId":"vidShort004"}}]}"#.to_string(),
            ),
            (
                200,
                r#"{"items":[
                    {"id":"vidShort004","snippet":{"title":"Keep 2","channelId":"UCsh00000000000000000004","description":"正常"}}
                ]}"#.to_string(),
            ),
        ])
        .await;

        let ytapi = YtApi::with_test_base_url(Some("AIzaSySHORT_PAGE".to_string()), base);
        let resolver = YtDlpResolver::new(None);

        // count=20 に対して 2 ページで 1 件しか残らなくても、nextPageToken=p3 があるため has_more=true
        let p1 = search_with("short", Some(20), &db, &ytapi, &resolver, &matcher)
            .await
            .unwrap();
        assert_eq!(p1.items.len(), 1);
        assert_eq!(p1.items[0].video_id, "vidShort002");
        assert!(p1.has_more);
        assert_eq!(recorded.lock().unwrap().len(), 4);

        // 追加読み込みで Page 3 を取得し、終端到達で has_more=false
        let p2 = search_with("short", Some(40), &db, &ytapi, &resolver, &matcher)
            .await
            .unwrap();
        assert_eq!(p2.items.len(), 2);
        assert_eq!(p2.items[1].video_id, "vidShort004");
        assert!(!p2.has_more);
        assert_eq!(recorded.lock().unwrap().len(), 6);

        srv.abort();
    }

    #[tokio::test]
    async fn search_with_falls_back_to_yt_search_and_conditionally_enriches_via_videos_list() {
        use super::search_with;
        use crate::db::Db;
        use crate::filter::Matcher;
        use crate::model::Filter;
        use crate::yt::{YtDlpResolver, SETTING_YTDLP_PATH};
        use crate::ytapi::QuotaBucket;

        let mut jsonl_lines = Vec::with_capacity(60);
        // 0 番目は @handle のみ（channel_id: None）
        jsonl_lines.push(
            r#"{"id":"vidFall0000","title":"Handle Video","uploader":"Handle CH","uploader_id":"@blocked_handle","duration":60,"view_count":10}"#.to_string(),
        );
        for i in 1..60 {
            let vid = format!("vidFall{i:04}");
            let ch_id = format!("UCfall000000000000000{i:03}");
            jsonl_lines.push(format!(
                r#"{{"id":"{vid}","title":"Fallback {i}","channel_id":"{ch_id}","channel":"Ch {i}","duration":90,"view_count":{i}}}"#
            ));
        }
        let (_script_guard, _data_guard) =
            write_fake_ytsearch_script("fallback60", &jsonl_lines.join("\n"));

        let db = Db::connect_in_memory().unwrap();
        db.setting_set(SETTING_YTDLP_PATH, _script_guard.0.to_str().unwrap())
            .unwrap();
        let resolver = YtDlpResolver::new(None);

        // 50 件チャンク用のモック応答（vidFall0000 は blocked 対象、vidFall0001 は desc NG 対象）
        let mut items_50 = Vec::with_capacity(50);
        items_50.push(
            r#"{"id":"vidFall0000","snippet":{"title":"Handle Video","channelId":"UCblocked000000000000001","channelTitle":"Blocked Resolved","description":"normal"}}"#.to_string(),
        );
        items_50.push(
            r#"{"id":"vidFall0001","snippet":{"title":"Fallback 1","channelId":"UCfall000000000000000001","channelTitle":"Ch 1","description":"NGキーワード入り"}}"#.to_string(),
        );
        for i in 2..50 {
            let vid = format!("vidFall{i:04}");
            let ch_id = format!("UCfall000000000000000{i:03}");
            items_50.push(format!(
                r#"{{"id":"{vid}","snippet":{{"title":"Fallback {i}","channelId":"{ch_id}","channelTitle":"Ch {i}","description":"clean desc"}},"contentDetails":{{"duration":"PT1M30S"}},"statistics":{{"viewCount":"{}"}}}}"#,
                i * 100
            ));
        }
        let batch_50_body = format!(r#"{{"items":[{}]}}"#, items_50.join(","));

        let (base, recorded, srv) = spawn_search_mock(vec![
            // 1) /search 403 quotaExceeded -> Search 枠のみ遮断（Shared は有効維持）
            (
                403,
                r#"{"error":{"errors":[{"reason":"quotaExceeded"}]}}"#.to_string(),
            ),
            // 2) (b) video_desc ルールあり -> 50 件チャンク 1 回のみ
            (200, batch_50_body),
            // 3) (c) channel_id: None の 1 件のみ解決
            (
                200,
                r#"{"items":[{"id":"vidFall0000","snippet":{"title":"Handle Video","channelId":"UCblocked000000000000001","channelTitle":"Blocked Resolved"}}]}"#.to_string(),
            ),
            // 4) (d) Shared 枠も 403 quotaExceeded -> 無補強フォールバック
            (
                403,
                r#"{"error":{"errors":[{"reason":"quotaExceeded"}]}}"#.to_string(),
            ),
        ])
        .await;

        let ytapi = YtApi::with_test_base_url(Some("AIzaSyFALL_CMD".to_string()), base);

        // (a) ルール無し時は Search 403 でフォールバックし、videos.list は 0 回
        let res_no_rules = search_with("q", Some(20), &db, &ytapi, &resolver, &Matcher::empty())
            .await
            .unwrap()
            .items;
        assert_eq!(res_no_rules.len(), 20);
        assert!(!ytapi.can_use(QuotaBucket::Search));
        assert!(ytapi.can_use(QuotaBucket::Shared));
        assert_eq!(recorded.lock().unwrap().len(), 1);

        // (b) video_desc ルール有効時は 50 件チャンク 1 回のみで survivors >= 20 早期打ち切り + NG 除外
        let desc_matcher = Matcher::rebuild(&[Filter {
            id: 1,
            target: "video_desc".to_string(),
            kind: "literal".to_string(),
            pattern: "NGキーワード".to_string(),
            enabled: true,
            created_at: String::new(),
        }]);
        let res_desc = search_with("q", Some(20), &db, &ytapi, &resolver, &desc_matcher)
            .await
            .unwrap()
            .items;
        assert_eq!(res_desc.len(), 20);
        assert!(!res_desc.iter().any(|r| r.video_id == "vidFall0001"));
        assert_eq!(res_desc[0].video_id, "vidFall0000");
        assert_eq!(
            res_desc[0].channel_id.as_deref(),
            Some("UCblocked000000000000001")
        );
        assert_eq!(recorded.lock().unwrap().len(), 2);

        // (c) channel_id: None + ブロック有効時は未解決 ID のみ videos.list で UC 解決しブロック除外
        db.blocked_add("UCblocked000000000000001", "Blocked CH")
            .unwrap();
        let res_blocked = search_with("q", Some(20), &db, &ytapi, &resolver, &Matcher::empty())
            .await
            .unwrap()
            .items;
        assert_eq!(res_blocked.len(), 20);
        assert!(!res_blocked.iter().any(|r| r.video_id == "vidFall0000"));
        assert_eq!(res_blocked[0].video_id, "vidFall0001");
        {
            let reqs = recorded.lock().unwrap();
            assert_eq!(reqs.len(), 3);
            assert!(reqs[2].contains("id=vidFall0000"));
            assert!(!reqs[2].contains("vidFall0001"));
        }

        // (d) videos.list が 403 quotaExceeded を返して Shared も枯渇しても、透過フォールバックで返却
        let res_shared_exhausted =
            search_with("q", Some(20), &db, &ytapi, &resolver, &desc_matcher)
                .await
                .unwrap()
                .items;
        assert_eq!(res_shared_exhausted.len(), 20);
        assert!(!ytapi.can_use(QuotaBucket::Shared));
        assert_eq!(recorded.lock().unwrap().len(), 4);

        // Shared 既遮断後の再実行は videos.list 通信 0 回
        let res_after_exhaust = search_with("q", Some(20), &db, &ytapi, &resolver, &desc_matcher)
            .await
            .unwrap()
            .items;
        assert_eq!(res_after_exhaust.len(), 20);
        assert_eq!(recorded.lock().unwrap().len(), 4);

        srv.abort();
    }
}
