//! Tauri invoke ハンドラ（設計書 §2, §3.1）。
//! 入力検証と UI 向けの直列化に徹し、実処理は各モジュールへ委譲する。

use std::sync::Arc;

use tauri::{AppHandle, Emitter, State};

use crate::db::Db;
use crate::error::UiError;
use crate::feed::{self, FeedPoller};
use crate::model::{
    normalize_video_id, parse_channel_ref, Category, Channel, ChannelRef, DbStatus, FeedFilter,
    FeedItem, FeedNewItems, PlayerAction, WatchHistory, YtDlpStatus,
};
use crate::mpv::PlayerManager;
use crate::yt::{self, YtDlpResolver};

const MAX_SETTING_KEY_LEN: usize = 128;
const MAX_SETTING_VALUE_LEN: usize = 16 * 1024;

fn validate_setting_key(key: &str) -> Result<(), UiError> {
    if key.is_empty() {
        return Err(UiError::invalid_input("key が空です"));
    }
    if key.len() > MAX_SETTING_KEY_LEN {
        return Err(UiError::invalid_input(format!(
            "key が長すぎます（{MAX_SETTING_KEY_LEN} 文字上限）"
        )));
    }
    Ok(())
}

/// 入力（URL 各形式または動画 ID）を動画 ID へ正規化する共通処理。
fn parse_video_id(input: &str) -> Result<String, UiError> {
    normalize_video_id(input).ok_or_else(|| {
        UiError::invalid_input("YouTube の動画 URL または 11 文字の動画 ID を入力してください")
    })
}

/// DB の生存確認。起動直後のヘルスチェック用。
#[tauri::command]
pub fn db_status(db: State<'_, Db>) -> Result<DbStatus, UiError> {
    Ok(DbStatus {
        schema_version: db.schema_version()?,
    })
}

#[tauri::command]
pub fn settings_get(db: State<'_, Db>, key: String) -> Result<Option<String>, UiError> {
    validate_setting_key(&key)?;
    Ok(db.setting_get(&key)?)
}

#[tauri::command]
pub fn settings_set(db: State<'_, Db>, key: String, value: String) -> Result<(), UiError> {
    validate_setting_key(&key)?;
    if value.len() > MAX_SETTING_VALUE_LEN {
        return Err(UiError::invalid_input(format!(
            "value が長すぎます（{MAX_SETTING_VALUE_LEN} バイト上限）"
        )));
    }
    Ok(db.setting_set(&key, &value)?)
}

/// `play_video`（設計書 §3.1）。`video_id` は URL 各形式も受け取り正規化する。
/// `resume` が true のとき、未完了の履歴位置から再開する。
#[tauri::command]
pub async fn play_video(
    video_id: String,
    resume: bool,
    players: State<'_, PlayerManager>,
    db: State<'_, Db>,
) -> Result<u32, UiError> {
    let id = parse_video_id(&video_id)?;
    let start_sec = if resume {
        db.history_get(&id)?
            .filter(|h| !h.completed)
            .map(|h| h.position_sec as f64)
            .unwrap_or(0.0)
    } else {
        0.0
    };
    let ytdl_format = db.setting_get(yt::SETTING_QUALITY_FORMAT)?;
    players
        .play(&id, start_sec, ytdl_format)
        .await
        .map_err(UiError::from)
}

/// `player_control`（設計書 §3.1）。操作は `PlayerAction` のタグ付き列挙で受け取る。
#[tauri::command]
pub async fn player_control(
    instance_id: u32,
    action: PlayerAction,
    players: State<'_, PlayerManager>,
) -> Result<(), UiError> {
    players
        .control(instance_id, &action)
        .await
        .map_err(UiError::from)
}

/// `player_close`（設計書 §3.1）。
#[tauri::command]
pub async fn player_close(
    instance_id: u32,
    players: State<'_, PlayerManager>,
) -> Result<(), UiError> {
    players.close(instance_id).await.map_err(UiError::from)
}

/// 動画 1 件分の視聴履歴。`video_id` は `play_video` と同じ正規化を経る。
#[tauri::command]
pub fn history_get(video_id: String, db: State<'_, Db>) -> Result<Option<WatchHistory>, UiError> {
    let id = parse_video_id(&video_id)?;
    Ok(db.history_get(&id)?)
}

/// yt-dlp の解決パスとバージョン（設計書 §5 の運用確認用）。
#[tauri::command]
pub async fn ytdlp_status(
    db: State<'_, Db>,
    resolver: State<'_, YtDlpResolver>,
) -> Result<YtDlpStatus, UiError> {
    let (path, version) = resolver.status(&db).await;
    Ok(YtDlpStatus { path, version })
}

/// `yt-dlp -U` による更新（設計書 §5 の更新機構）。
/// システム管理パスでは権限不足で失敗し得る。その場合もエラーをそのまま返す。
#[tauri::command]
pub async fn ytdlp_update(
    db: State<'_, Db>,
    resolver: State<'_, YtDlpResolver>,
) -> Result<String, UiError> {
    let path = resolver.resolve(&db).await.ok_or(yt::YtError::NotFound)?;
    Ok(yt::update(&path).await?)
}

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

/// `list_feed`（設計書 §3.1）。
#[tauri::command]
pub fn list_feed(filter: FeedFilter, db: State<'_, Db>) -> Result<Vec<FeedItem>, UiError> {
    Ok(db.feed_list(&filter)?)
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

/// `search`（設計書 §3.1、FR-4）。`yt-dlp ytsearch` の結果から
/// ブロック済みチャンネルを除いて返す。
#[tauri::command]
pub async fn search(
    query: String,
    db: State<'_, Db>,
    resolver: State<'_, YtDlpResolver>,
) -> Result<Vec<crate::model::SearchResult>, UiError> {
    let q = query.trim().to_string();
    if q.is_empty() {
        return Err(UiError::invalid_input("検索語が空です"));
    }
    if q.len() > 256 {
        return Err(UiError::invalid_input("検索語が長すぎます"));
    }
    let path = resolver.resolve(&db).await.ok_or(yt::YtError::NotFound)?;
    let mut results = yt::search(&path, &q, SEARCH_LIMIT).await?;
    let blocked = db.blocked_ids()?;
    results.retain(|r| {
        r.channel_id
            .as_deref()
            .map(|c| !blocked.contains(c))
            .unwrap_or(true)
    });
    Ok(results)
}

/// 検索の既定取得件数（設計書 §5 の `ytsearch<N>`）。
const SEARCH_LIMIT: u32 = 20;

/// `get_related`（設計書 §3.1、FR-4）。InnerTube `next` の関連動画から
/// ブロック済みチャンネルを除いて返す。
#[tauri::command]
pub async fn get_related(
    video_id: String,
    db: State<'_, Db>,
    innertube: State<'_, Arc<crate::innertube::InnerTube>>,
) -> Result<Vec<crate::model::SearchResult>, UiError> {
    let video_id = parse_video_id(&video_id)?;
    let mut results = innertube.related(&video_id).await?;
    let blocked = db.blocked_ids()?;
    results.retain(|r| {
        r.channel_id
            .as_deref()
            .map(|c| !blocked.contains(c))
            .unwrap_or(true)
    });
    Ok(results)
}

/// `block_channel`（設計書 §3.1、FR-5）。検索・関連・フィードの全一覧から除外される。
#[tauri::command]
pub fn block_channel(channel_id: String, title: String, db: State<'_, Db>) -> Result<(), UiError> {
    let channel_id = channel_id.trim();
    if channel_id.is_empty() {
        return Err(UiError::invalid_input("channel_id が空です"));
    }
    db.blocked_add(channel_id, title.trim())?;
    Ok(())
}

/// `unblock_channel`（FR-5）。設定画面のブロック一覧から解除する。
#[tauri::command]
pub fn unblock_channel(channel_id: String, db: State<'_, Db>) -> Result<(), UiError> {
    db.blocked_remove(channel_id.trim())?;
    Ok(())
}

/// ブロック中チャンネル一覧（設定画面用）。
#[tauri::command]
pub fn blocked_channels(db: State<'_, Db>) -> Result<Vec<crate::model::BlockedChannel>, UiError> {
    Ok(db.blocked_list()?)
}

/// `chat_start`（設計書 §3.1、FR-6）。指定動画のライブチャット取得を
/// バックグラウンドで開始する。見つからない・失敗した場合の通知は
/// `chat://status` イベントに流れる。
#[tauri::command]
pub fn chat_start(
    video_id: String,
    poller: State<'_, Arc<crate::chat::ChatPoller>>,
) -> Result<(), UiError> {
    let id = parse_video_id(&video_id)?;
    poller.start(&id);
    Ok(())
}

/// `chat_stop`。指定動画のチャット取得を止める。
#[tauri::command]
pub fn chat_stop(
    video_id: String,
    poller: State<'_, Arc<crate::chat::ChatPoller>>,
) -> Result<(), UiError> {
    let id = parse_video_id(&video_id)?;
    poller.stop(&id);
    Ok(())
}

/// `chat_history_search`（FR-8）。本文・投稿者名の FTS5 AND 検索。
#[tauri::command]
pub fn chat_history_search(
    video_id: Option<String>,
    query: String,
    limit: Option<u32>,
    db: State<'_, Db>,
) -> Result<Vec<crate::model::ChatEvent>, UiError> {
    let q = query.trim().to_string();
    if q.is_empty() {
        return Err(UiError::invalid_input("検索語が空です"));
    }
    if q.len() > 256 {
        return Err(UiError::invalid_input("検索語が長すぎます"));
    }
    let vid = match &video_id {
        Some(v) if !v.trim().is_empty() => Some(parse_video_id(v)?),
        _ => None,
    };
    Ok(db.chat_search(vid.as_deref(), &q, limit.unwrap_or(100).min(1000))?)
}

/// `filter_add`（設計書 §3.1、FR-7）。target/kind/pattern を検証して登録し、
/// 稼働中の NG 評価器を作り直す。
#[tauri::command]
pub fn filter_add(
    target: String,
    kind: String,
    pattern: String,
    db: State<'_, Db>,
    poller: State<'_, Arc<crate::chat::ChatPoller>>,
) -> Result<crate::model::Filter, UiError> {
    let target = target.trim().to_string();
    let kind = kind.trim().to_string();
    let pattern = pattern.trim().to_string();
    if !crate::model::FILTER_TARGETS.contains(&target.as_str()) {
        return Err(UiError::invalid_input(format!(
            "target が不正です: {target}"
        )));
    }
    if !crate::model::FILTER_KINDS.contains(&kind.as_str()) {
        return Err(UiError::invalid_input("kind は literal または regex です"));
    }
    if pattern.is_empty() {
        return Err(UiError::invalid_input("pattern が空です"));
    }
    if pattern.len() > 512 {
        return Err(UiError::invalid_input(
            "pattern が長すぎます（512 文字上限）",
        ));
    }
    if kind == "regex" {
        regex::Regex::new(&pattern)
            .map_err(|e| UiError::invalid_input(format!("正規表現が不正です: {e}")))?;
    }
    let f = db.filter_add(&target, &kind, &pattern)?;
    poller.refresh_filters()?;
    Ok(f)
}

/// `filter_remove`。削除して NG 評価器を作り直す。
#[tauri::command]
pub fn filter_remove(
    id: i64,
    db: State<'_, Db>,
    poller: State<'_, Arc<crate::chat::ChatPoller>>,
) -> Result<(), UiError> {
    db.filter_remove(id)?;
    poller.refresh_filters()?;
    Ok(())
}

/// `filter_list`。登録済み NG フィルタの一覧。
#[tauri::command]
pub fn filter_list(db: State<'_, Db>) -> Result<Vec<crate::model::Filter>, UiError> {
    Ok(db.filter_list()?)
}
