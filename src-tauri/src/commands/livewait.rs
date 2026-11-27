//! ライブ配信待機のコマンド（FR-44、仕様決定 BT、設計書 §3.1）。

use std::sync::Arc;

use tauri::{AppHandle, State};

use super::parse_video_id;
use crate::db::Db;
use crate::error::UiError;
use crate::innertube::InnerTube;
use crate::livewait::{classify_broadcast, emit_changed, epoch_ms, LiveWaitPoller, WaitBroadcast};
use crate::model::{BroadcastStatus, LiveWait, LiveWaitAddOutcome, VideoRef};
use crate::ytapi::{QuotaBucket, YtApi};

/// 待機項目の各文字列の上限（queue_save と同じ扱い）。
const MAX_WAIT_TEXT_LEN: usize = 1024;

async fn resolve_live_wait_candidate(
    video_id: &str,
    need_meta: bool,
    ytapi: &YtApi,
    innertube: &InnerTube,
) -> Result<(BroadcastStatus, Option<VideoRef>), UiError> {
    if ytapi.can_use(QuotaBucket::Shared) {
        match ytapi.videos_list(&[video_id]).await {
            Ok(mut map) => {
                if let Some(item) = map.remove(video_id) {
                    let meta = if need_meta && !item.video_ref.title.trim().is_empty() {
                        Some(item.video_ref)
                    } else {
                        None
                    };
                    return Ok((item.meta.broadcast, meta));
                }
            }
            Err(e) => {
                let _ = e.should_fallback();
            }
        }
    }
    let status = innertube.broadcast_status(video_id).await?;
    let meta = if need_meta && matches!(classify_broadcast(&status), WaitBroadcast::Upcoming { .. })
    {
        innertube.video_meta(video_id).await.ok().flatten()
    } else {
        None
    };
    Ok((status, meta))
}

/// `live_wait_add`（仕様決定 BT(1)・ED）。行のメタとともに待機登録する。
/// 登録前に YouTube Data API v3（優先）または InnerTube `player` で放送状態を確認し、
/// 開始前（ロビー）のものだけを `added` として登録する。放送中・終了済み・非放送は
/// それぞれの区分を返し、登録はしない（呼び出し側が案内を出す）。
/// 同一動画の重複登録は `duplicate` を返して抑止する。
#[tauri::command]
pub async fn live_wait_add(
    video: VideoRef,
    db: State<'_, Db>,
    ytapi: State<'_, Arc<YtApi>>,
    innertube: State<'_, Arc<InnerTube>>,
    poller: State<'_, Arc<LiveWaitPoller>>,
    app: AppHandle,
) -> Result<LiveWaitAddOutcome, UiError> {
    let video_id = parse_video_id(&video.video_id)?;
    let too_long =
        |s: &Option<String>| s.as_deref().unwrap_or("").chars().count() > MAX_WAIT_TEXT_LEN;
    if video.title.chars().count() > MAX_WAIT_TEXT_LEN
        || too_long(&video.channel_id)
        || too_long(&video.channel_title)
        || too_long(&video.thumbnail_url)
    {
        return Err(UiError::invalid_input("待機項目の文字列が長すぎます"));
    }
    // 重複抑止は外部確認より先に判定する（確認不要なので通信を省く）
    if db.live_wait_get(&video_id)?.is_some() {
        return Ok(LiveWaitAddOutcome::Duplicate);
    }
    let mut title = video.title.trim().to_string();
    let mut channel_id = video.channel_id.clone();
    let mut channel_title = video.channel_title.clone();
    let mut thumbnail_url = video.thumbnail_url.clone();
    // 登録時の放送状態確認（仕様決定 BT(1)）。開始前のみ登録へ進む
    let (status, maybe_meta) =
        resolve_live_wait_candidate(&video_id, title.is_empty(), &ytapi, &innertube).await?;
    let scheduled_ms = match classify_broadcast(&status) {
        WaitBroadcast::Upcoming { scheduled_ms } => scheduled_ms,
        WaitBroadcast::Live => return Ok(LiveWaitAddOutcome::Live),
        WaitBroadcast::Ended => return Ok(LiveWaitAddOutcome::Ended),
        WaitBroadcast::NotBroadcast => return Ok(LiveWaitAddOutcome::NotBroadcast),
    };
    if let Some(meta) = maybe_meta {
        title = meta.title;
        channel_id = channel_id.or(meta.channel_id);
        channel_title = channel_title.or(meta.channel_title);
        thumbnail_url = thumbnail_url.or(meta.thumbnail_url);
    }
    if title.is_empty() {
        title = video_id.clone();
    }
    let entry = LiveWait {
        video_id: video_id.clone(),
        title,
        channel_id,
        channel_title,
        thumbnail_url,
        scheduled_start_ms: scheduled_ms,
        registered_ms: epoch_ms(),
    };
    // 確認から挿入までの競合は INSERT OR IGNORE が吸収する
    if !db.live_wait_insert(&entry)? {
        return Ok(LiveWaitAddOutcome::Duplicate);
    }
    emit_changed(&app, Some(&video_id));
    poller.wake_now();
    Ok(LiveWaitAddOutcome::Added {
        scheduled_start_ms: scheduled_ms,
    })
}

/// `live_wait_remove`（仕様決定 BT(2)）。待機の個別解除。
/// 未登録の動画 ID でも正常終了とする（冪等）。
#[tauri::command]
pub async fn live_wait_remove(
    video_id: String,
    db: State<'_, Db>,
    app: AppHandle,
) -> Result<(), UiError> {
    let id = parse_video_id(&video_id)?;
    db.live_wait_remove(&id)?;
    emit_changed(&app, Some(&id));
    Ok(())
}

/// `live_wait_list`（仕様決定 BT(2)(5)）。待機一覧の復元・表示用。
#[tauri::command]
pub async fn live_wait_list(db: State<'_, Db>) -> Result<Vec<LiveWait>, UiError> {
    Ok(db.live_wait_list()?)
}

#[cfg(test)]
mod tests {
    use super::resolve_live_wait_candidate;
    use crate::innertube::InnerTube;
    use crate::livewait::{classify_broadcast, WaitBroadcast};
    use crate::ytapi::YtApi;
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn resolve_live_wait_candidate_prefers_ytapi_and_falls_back() {
        let yt_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let yt_addr = yt_listener.local_addr().unwrap();
        let yt_srv = tokio::spawn(async move {
            let mut count = 0usize;
            while let Ok((mut stream, _)) = yt_listener.accept().await {
                count += 1;
                let mut buf = [0u8; 2048];
                let _ = stream.read(&mut buf).await;
                let resp = if count == 1 {
                    let body = r#"{"items":[{"id":"lwCmd000001","snippet":{"title":"Upcoming From YtApi","channelId":"UClwcmd0000000000000001","channelTitle":"CH","liveBroadcastContent":"upcoming"},"liveStreamingDetails":{"scheduledStartTime":"2026-11-27T15:00:00Z"}}]}"#;
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                } else {
                    "HTTP/1.1 500 Internal Server Error\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}".to_string()
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
                    let body = r#"{"videoDetails":{"videoId":"lwCmd000001","title":"Fallback Title","isUpcoming":true},"microformat":{"playerMicroformatRenderer":{"liveBroadcastDetails":{"isLiveNow":false,"scheduledStartTimestamp":"2026-11-27T16:00:00+00:00"}}}}"#;
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                };
                let _ = stream.write_all(resp.as_bytes()).await;
            }
        });

        let ytapi = YtApi::with_test_base_url(
            Some("AIzaSyLWCMD_KEY".to_string()),
            format!("http://{yt_addr}"),
        );
        let tube = InnerTube::with_test_urls(
            format!("http://{tube_addr}/watch"),
            format!("http://{tube_addr}/api"),
            Duration::from_secs(5),
        );

        let (st1, meta1) = resolve_live_wait_candidate("lwCmd000001", true, &ytapi, &tube)
            .await
            .unwrap();
        assert!(matches!(
            classify_broadcast(&st1),
            WaitBroadcast::Upcoming {
                scheduled_ms: Some(_)
            }
        ));
        assert_eq!(meta1.unwrap().title, "Upcoming From YtApi");

        let (st2, meta2) = resolve_live_wait_candidate("lwCmd000001", true, &ytapi, &tube)
            .await
            .unwrap();
        assert!(matches!(
            classify_broadcast(&st2),
            WaitBroadcast::Upcoming {
                scheduled_ms: Some(_)
            }
        ));
        assert_eq!(meta2.unwrap().title, "Fallback Title");

        yt_srv.abort();
        tube_srv.abort();
    }
}
