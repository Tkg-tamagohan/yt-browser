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
    let mut results = yt::search(
        &path,
        &q,
        count * SEARCH_FETCH_FACTOR,
        yt::po_token_setting(&db).as_deref(),
    )
    .await?;
    let blocked = db.blocked_ids()?;
    let matcher = ng.get();
    results.retain(|r| {
        r.channel_id
            .as_deref()
            .map(|c| !blocked.contains(c))
            .unwrap_or(true)
            // ytsearch flat に説明文フィールドは無いため video_desc は
            // 評価しない（FR-85、仕様決定 DJ。暫定解釈: 説明文を持たない
            // 経路では未評価のまま通す）
            && !matcher.is_video_ng(
                &r.title,
                r.channel_title.as_deref(),
                r.channel_id.as_deref(),
                None,
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
}
