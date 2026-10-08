//! チャンネル購読・フィード系コマンド（設計書 §3.1）。

use std::sync::Arc;

use tauri::{AppHandle, Emitter, State};

use crate::db::Db;
use crate::error::UiError;
use crate::feed::{self, FeedPoller};
use crate::model::{
    parse_channel_ref, Category, Channel, ChannelRef, FeedFilter, FeedItem, FeedNewItems,
    PlayingChannel,
};
use crate::yt::{self, YtDlpResolver};

use super::parse_video_id;

/// `subscribe_channel`（設計書 §3.1）。UC ID・channel URL・@handle を受け取り、
/// RSS を 1 回取得して存在確認と初期一覧の投入を行う。取得したエントリは未読で積む
/// （登録直後に新着が一覧に現れるのが受け入れ条件）。
#[tauri::command]
pub async fn subscribe_channel(
    input: String,
    category_id: Option<i64>,
    db: State<'_, Db>,
    poller: State<'_, Arc<FeedPoller>>,
    resolver: State<'_, YtDlpResolver>,
    app: AppHandle,
) -> Result<Channel, UiError> {
    let input = input.trim().to_string();
    let r = parse_channel_ref(&input).ok_or_else(|| {
        UiError::invalid_input(
            "チャンネル ID（UC...）、youtube.com/channel/UC...、または @handle を入力してください",
        )
    })?;
    let channel_id = match r {
        ChannelRef::Id(id) => id,
        ChannelRef::Handle(h) => {
            let path = resolver.resolve(&db).await.ok_or(yt::YtError::NotFound)?;
            // 非 ASCII ハンドルは Url::parse 経由でパーセントエンコードしてから渡す
            let url = url::Url::parse(&format!("https://www.youtube.com/@{h}"))
                .map_err(|_| UiError::invalid_input("ハンドルの形式が不正です"))?;
            yt::channel_id(&path, url.as_str())
                .await
                .map_err(UiError::from)?
        }
    };
    match feed::fetch_feed(&poller.client, &channel_id, None, None).await? {
        feed::FetchOutcome::Parsed {
            feed,
            etag,
            last_modified,
        } => {
            // フィードの channel_id が入力と一致しない場合はフィード側を採る
            // （UC ID の誤入力より URL 解決のリダイレクトを信用する暫定仕様）。
            // 購読登録と初回投入は一トランザクション（feed_subscribe）:
            // 新規購読と判定された場合のみ既存行も未読へ戻す
            let items: Vec<crate::db::NewVideo> = feed
                .entries
                .iter()
                .map(|e| crate::db::NewVideo {
                    video_id: &e.video_id,
                    channel_id: &feed.channel_id,
                    channel_title: &feed.channel_title,
                    title: &e.title,
                    thumbnail_url: e.thumbnail_url.as_deref(),
                    published_at: e.published_at.as_deref(),
                    kind: "video",
                })
                .collect();
            let out = db.feed_subscribe(&crate::db::SubscribeArgs {
                channel_id: &feed.channel_id,
                title: &feed.channel_title,
                thumbnail_url: None,
                category_id,
                entries: &items,
                etag: etag.as_deref(),
                last_modified: last_modified.as_deref(),
            })?;
            // 新規挿入に加えて既読→未読の戻し（再購読）も画面更新が要る変化
            if out.touched() {
                let _ = app.emit(
                    "feed://new_items",
                    FeedNewItems {
                        count: out.inserted + out.unread_changed,
                    },
                );
            }
            // 初回投入分も shorts 判定の対象（仕様決定 V）
            poller.spawn_kind_detection(out.new_video_ids);
            poller.wake_now();
            db.channel_get(&feed.channel_id)?
                .ok_or_else(|| UiError::internal("購読登録直後のチャンネル取得に失敗"))
        }
        // 条件付きヘッダを送っていないので 304 は返らないはず
        feed::FetchOutcome::NotModified => {
            Err(UiError::internal("初回取得が NotModified を返した"))
        }
    }
}

/// 再生中動画のチャンネル解決（FR-12、仕様決定 U）。
/// `videos.channel_id` → `watch_history.channel_id` → yt-dlp メタの順に
/// `subscribe_channel` へ渡せる入力を決めて返す。各段の値が空文字・NULL の
/// 場合は「未知」として次段へ進む。解決不能は `input: null`。
#[tauri::command]
pub async fn playing_channel(
    video_id: String,
    db: State<'_, Db>,
    resolver: State<'_, YtDlpResolver>,
) -> Result<PlayingChannel, UiError> {
    let id = parse_video_id(&video_id)?;
    let mut input: Option<String> = None;
    let mut title: Option<String> = None;
    // 1) videos 台帳（フィード・お気に入り・プレイリスト経由で既知情報がある）
    let (cid, ct) = db.video_channel(&id)?;
    input = cid;
    title = ct;
    // 2) watch_history（台帳に無い動画も履歴の再視聴で拾う）
    if input.is_none() {
        if let Some(h) = db.history_get(&id)? {
            input = h.channel_id.filter(|s| !s.is_empty());
            if title.is_none() {
                title = h.channel_title;
            }
        }
    }
    // 3) yt-dlp メタ（DB に知識が無いときだけ。失敗は解決不能扱い）
    if input.is_none() {
        if let Some(path) = resolver.resolve(&db).await {
            if let Ok(cands) = yt::video_channel_ref(&path, &id).await {
                // 受理できない形（解釈不能な channel_id 等）は次候補へ
                input = cands.into_iter().find(|c| parse_channel_ref(c).is_some());
            }
        }
    }
    // 購読済み判定は UC ID が確定しているときだけ可能
    // （@handle は subscribe_channel が解決するまで ID が分からない）
    let subscribed = input
        .as_deref()
        .and_then(parse_channel_ref)
        .and_then(|r| match r {
            ChannelRef::Id(cid) => Some(cid),
            _ => None,
        })
        .map(|cid| db.channel_get(&cid).map(|c| c.is_some()).unwrap_or(false))
        .unwrap_or(false);
    Ok(PlayingChannel {
        input,
        title,
        subscribed,
    })
}

/// `unsubscribe_channel`。購読解除し、そのチャンネルの未読を既読化する。
#[tauri::command]
pub fn unsubscribe_channel(channel_id: String, db: State<'_, Db>) -> Result<(), UiError> {
    db.channel_delete(&channel_id)?;
    Ok(())
}

/// 購読チャンネル一覧。
#[tauri::command]
pub fn list_channels(db: State<'_, Db>) -> Result<Vec<Channel>, UiError> {
    Ok(db.channel_list()?)
}

/// チャンネルのカテゴリ割り当て（null で未分類に戻す）。
#[tauri::command]
pub fn set_channel_category(
    channel_id: String,
    category_id: Option<i64>,
    db: State<'_, Db>,
) -> Result<(), UiError> {
    db.channel_set_category(&channel_id, category_id)?;
    Ok(())
}

#[tauri::command]
pub fn list_categories(db: State<'_, Db>) -> Result<Vec<Category>, UiError> {
    Ok(db.category_list()?)
}

#[tauri::command]
pub fn create_category(name: String, db: State<'_, Db>) -> Result<Category, UiError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(UiError::invalid_input("カテゴリ名が空です"));
    }
    Ok(db.category_create(name)?)
}

/// `list_feed`（設計書 §3.1）。NG フィルタの動画系 target もここで適用する（FR-9）。
#[tauri::command]
pub fn list_feed(
    filter: FeedFilter,
    db: State<'_, Db>,
    ng: State<'_, Arc<crate::filter::NgMatcher>>,
) -> Result<Vec<FeedItem>, UiError> {
    let matcher = ng.get();
    // 述語適合が FEED_LIST_LIMIT 件に達するまで走査するため、
    // 先頭が NG で抜けても後続の適合行を拾える
    Ok(
        db.feed_list_filtered(&filter, crate::db::FEED_LIST_LIMIT, |i| {
            !matcher.is_video_ng(&i.title, i.channel_title.as_deref(), Some(&i.channel_id))
        })?,
    )
}

/// `mark_read`（設計書 §3.1）。`all: true` で一括既読、それ以外は `video_ids` を個別既読。
#[tauri::command]
pub fn mark_read(
    video_ids: Option<Vec<String>>,
    all: Option<bool>,
    db: State<'_, Db>,
) -> Result<u64, UiError> {
    if all == Some(true) {
        return Ok(db.videos_mark_all_read()?);
    }
    let ids = video_ids.unwrap_or_default();
    db.videos_mark_read(&ids)?;
    Ok(ids.len() as u64)
}

/// 手動の即時更新。全チャンネル（または指定チャンネル）の次回予定を現在に戻す。
#[tauri::command]
pub fn feed_refresh(
    channel_id: Option<String>,
    poller: State<'_, Arc<FeedPoller>>,
) -> Result<(), UiError> {
    poller.force_refresh(channel_id.as_deref());
    Ok(())
}
