//! 履歴・お気に入り・プレイリスト・あとで見る・横断検索系コマンド（FR-7、FR-59、FR-60）。

use std::sync::Arc;

use super::parse_video_id;
use crate::db::{Db, PlaylistImportItem};
use crate::error::UiError;
use crate::model::{
    is_playlist_source_id, FavoriteEntry, HistoryCursor, HistoryPage, HistoryStats,
    LibrarySearchItem, Playlist, PlaylistEntry, PlaylistSyncOutcome, VideoRef, WatchHistory,
    WatchLaterEntry, MAX_PLAYLIST_NAME_LEN,
};
use crate::yt::{self, YtDlpResolver};
use crate::ytapi::{YtApi, YtApiError};
use tauri::{AppHandle, Emitter, State};

/// 動画 1 件分の視聴履歴。`video_id` は `play_video` と同じ正規化を経る。
#[tauri::command]
pub async fn history_get(
    video_id: String,
    db: State<'_, Db>,
) -> Result<Option<WatchHistory>, UiError> {
    let id = parse_video_id(&video_id)?;
    Ok(db.history_get(&id)?)
}

/// 履歴一覧の既定・最大件数。
const HISTORY_LIST_LIMIT: u32 = 500;
/// 動画タイトル等の上限（`videos.title` の入力値）。
const MAX_VIDEO_FIELD_LEN: usize = 1024;

/// `history_list`（FR-7）。新しく見た順。`limit` 省略時は 500、上限 1000。
/// `cursor` は前ページの `next_cursor` をそのまま渡すページングカーソル
/// （FR-52、仕様決定 CC）。`query` はタイトル・チャンネル名の部分一致
/// 絞り込み（FR-61、仕様決定 CL）。戻り値は `items` と `next_cursor` の
/// ページで、続きが無ければ `next_cursor` は null。
#[tauri::command]
pub async fn history_list(
    cursor: Option<HistoryCursor>,
    query: Option<String>,
    limit: Option<u32>,
    db: State<'_, Db>,
) -> Result<HistoryPage, UiError> {
    let limit = limit.unwrap_or(HISTORY_LIST_LIMIT).min(1000);
    let db = db.inner().clone();
    Ok(tokio::task::spawn_blocking(move || {
        db.history_list(cursor.as_ref(), query.as_deref(), limit)
    })
    .await
    .map_err(|e| UiError::internal(format!("ワーカーが中断しました: {e}")))??)
}

/// `history_remove`（FR-7、仕様決定 I の手動削除）。
/// 削除した行のスナップショットを返す（FR-54、仕様決定 CE の
/// トースト「元に戻す」→ `history_restore` の入力）。対象行が無ければ null。
#[tauri::command]
pub async fn history_remove(
    video_id: String,
    db: State<'_, Db>,
) -> Result<Option<WatchHistory>, UiError> {
    let id = parse_video_id(&video_id)?;
    Ok(db.history_remove(&id)?)
}

/// `history_restore`（FR-54、仕様決定 CE）。
/// `history_remove` が返したスナップショットをそのまま受け取り、
/// 削除前の行（最終視聴日時・再生位置・視聴済みを含む全列）を復元する。
#[tauri::command]
pub async fn history_restore(entry: WatchHistory, db: State<'_, Db>) -> Result<(), UiError> {
    let id = parse_video_id(&entry.video_id)?;
    let mut e = entry;
    e.video_id = id;
    Ok(db.history_restore(&e)?)
}

/// `history_stats`（FR-65、仕様決定 CP）。`watch_history` を集計した
/// 累計視聴時間とチャンネル別・日別の内訳を返す
/// （ライブラリの統計タブが表示用にそのまま使う）。
#[tauri::command]
pub async fn history_stats(db: State<'_, Db>) -> Result<HistoryStats, UiError> {
    let db = db.inner().clone();
    Ok(tokio::task::spawn_blocking(move || db.history_stats())
        .await
        .map_err(|e| UiError::internal(format!("ワーカーが中断しました: {e}")))??)
}

/// `VideoRef` の入力検証と動画 ID 正規化。
/// キューのプレイリスト保存（commands::queue）からも使う
pub(crate) fn validate_video_ref(v: &VideoRef) -> Result<String, UiError> {
    let id = parse_video_id(&v.video_id)?;
    if v.title.chars().count() > MAX_VIDEO_FIELD_LEN {
        return Err(UiError::invalid_input("タイトルが長すぎます"));
    }
    Ok(id)
}

/// `favorite_add`（FR-7）。動画メタは `videos` 台帳へ登録される。
#[tauri::command]
pub async fn favorite_add(video: VideoRef, db: State<'_, Db>) -> Result<(), UiError> {
    let id = validate_video_ref(&video)?;
    let mut v = video;
    v.video_id = id;
    Ok(db.favorite_add(&v)?)
}

/// `favorite_remove`（FR-7）。
/// 削除した行のスナップショットを返す（FR-54、仕様決定 CE の
/// トースト「元に戻す」→ `favorite_restore` の入力）。対象行が無ければ null。
#[tauri::command]
pub async fn favorite_remove(
    video_id: String,
    db: State<'_, Db>,
) -> Result<Option<FavoriteEntry>, UiError> {
    let id = parse_video_id(&video_id)?;
    Ok(db.favorite_remove(&id)?)
}

/// `favorite_restore`（FR-54、仕様決定 CE）。
/// `favorite_remove` が返したスナップショットをそのまま受け取り、
/// 登録日時（`added_at`）も含めて削除前の行を復元する。
#[tauri::command]
pub async fn favorite_restore(entry: FavoriteEntry, db: State<'_, Db>) -> Result<(), UiError> {
    if entry.title.chars().count() > MAX_VIDEO_FIELD_LEN {
        return Err(UiError::invalid_input("タイトルが長すぎます"));
    }
    let id = parse_video_id(&entry.video_id)?;
    let mut e = entry;
    e.video_id = id;
    Ok(db.favorite_restore(&e)?)
}

/// `favorite_list`（FR-7）。追加が新しい順。
#[tauri::command]
pub async fn favorite_list(db: State<'_, Db>) -> Result<Vec<FavoriteEntry>, UiError> {
    Ok(db.favorite_list()?)
}

/// 「あとで見る」の登録上限の扱いは置かない（履歴の剪定対象でもない）。
/// 横断検索の上限（統合後の件数）。
const LIBRARY_SEARCH_LIMIT: u32 = 200;

/// `watch_later_add`（FR-59、仕様決定 CJ）。
/// 動画メタは `videos` 台帳へ登録され、同一動画の再登録は追加日時を
/// 変えない（冪等）。
#[tauri::command]
pub async fn watch_later_add(video: VideoRef, db: State<'_, Db>) -> Result<(), UiError> {
    let id = validate_video_ref(&video)?;
    let mut v = video;
    v.video_id = id;
    Ok(db.watch_later_add(&v)?)
}

/// `watch_later_remove`（FR-59）。未登録でも正常終了（冪等）。
#[tauri::command]
pub async fn watch_later_remove(video_id: String, db: State<'_, Db>) -> Result<(), UiError> {
    let id = parse_video_id(&video_id)?;
    Ok(db.watch_later_remove(&id)?)
}

/// `watch_later_list`（FR-59）。追加が新しい順。
#[tauri::command]
pub async fn watch_later_list(db: State<'_, Db>) -> Result<Vec<WatchLaterEntry>, UiError> {
    Ok(db.watch_later_list()?)
}

/// `library_search`（FR-60、仕様決定 CK）。
/// 履歴・お気に入り・プレイリスト項目・あとで見るをタイトルとチャンネル名の
/// 部分一致 LIKE で横断検索し、同一動画は `sources` に区分を持つ 1 件へ
/// まとめる。`query` の空・空白のみは 0 件を返す（暫定解釈）。
#[tauri::command]
pub async fn library_search(
    query: String,
    limit: Option<u32>,
    db: State<'_, Db>,
) -> Result<Vec<LibrarySearchItem>, UiError> {
    let limit = limit.unwrap_or(LIBRARY_SEARCH_LIMIT).min(1000);
    let db = db.inner().clone();
    Ok(
        tokio::task::spawn_blocking(move || db.library_search(&query, limit))
            .await
            .map_err(|e| UiError::internal(format!("ワーカーが中断しました: {e}")))??,
    )
}

/// `playlist_list`（FR-7）。件数集計付き。
#[tauri::command]
pub async fn playlist_list(db: State<'_, Db>) -> Result<Vec<Playlist>, UiError> {
    Ok(db.playlist_list()?)
}

/// プレイリスト名の検証（トリム・非空・長さ）。
/// キューのプレイリスト保存（commands::queue）からも使う
pub(crate) fn validate_playlist_name(name: &str) -> Result<String, UiError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(UiError::invalid_input("プレイリスト名が空です"));
    }
    if name.chars().count() > MAX_PLAYLIST_NAME_LEN {
        return Err(UiError::invalid_input(format!(
            "プレイリスト名が長すぎます（{MAX_PLAYLIST_NAME_LEN} 文字上限）"
        )));
    }
    Ok(name.to_string())
}

/// `playlist_create`（FR-7）。
#[tauri::command]
pub async fn playlist_create(name: String, db: State<'_, Db>) -> Result<Playlist, UiError> {
    let name = validate_playlist_name(&name)?;
    Ok(db.playlist_create(&name)?)
}

/// `playlist_rename`（FR-7）。
#[tauri::command]
pub async fn playlist_rename(
    playlist_id: i64,
    name: String,
    db: State<'_, Db>,
) -> Result<(), UiError> {
    if playlist_id <= 0 {
        return Err(UiError::invalid_input("playlist_id が不正です"));
    }
    let name = validate_playlist_name(&name)?;
    Ok(db.playlist_rename(playlist_id, &name)?)
}

/// `playlist_delete`（FR-7）。中身は CASCADE で消える。
#[tauri::command]
pub async fn playlist_delete(playlist_id: i64, db: State<'_, Db>) -> Result<(), UiError> {
    if playlist_id <= 0 {
        return Err(UiError::invalid_input("playlist_id が不正です"));
    }
    Ok(db.playlist_delete(playlist_id)?)
}

/// `playlist_items`（FR-7）。position 昇順。
/// `after_position`・`limit` の双方省略で全件（従来動作）。
/// ページングは末尾行の position を `after_position` に渡して続きを取る
/// （FR-25、仕様決定 AR）。
#[tauri::command]
pub async fn playlist_items(
    playlist_id: i64,
    after_position: Option<i64>,
    limit: Option<u32>,
    db: State<'_, Db>,
) -> Result<Vec<PlaylistEntry>, UiError> {
    if playlist_id <= 0 {
        return Err(UiError::invalid_input("playlist_id が不正です"));
    }
    let db = db.inner().clone();
    Ok(tokio::task::spawn_blocking(move || {
        db.playlist_items_page(playlist_id, after_position, limit)
    })
    .await
    .map_err(|e| UiError::internal(format!("ワーカーが中断しました: {e}")))??)
}

/// `playlist_add`（FR-7）。末尾への追加、重複は位置を維持して無視。
#[tauri::command]
pub async fn playlist_add(
    playlist_id: i64,
    video: VideoRef,
    db: State<'_, Db>,
) -> Result<(), UiError> {
    if playlist_id <= 0 {
        return Err(UiError::invalid_input("playlist_id が不正です"));
    }
    let id = validate_video_ref(&video)?;
    let mut v = video;
    v.video_id = id;
    Ok(db.playlist_add(playlist_id, &v)?)
}

/// `playlist_remove`（FR-7）。
/// 削除した行のスナップショット（元の `position` 付き）を返す
/// （FR-54、仕様決定 CE のトースト「元に戻す」→ `playlist_item_restore`
/// の入力）。対象行が無ければ null。
#[tauri::command]
pub async fn playlist_remove(
    playlist_id: i64,
    video_id: String,
    db: State<'_, Db>,
) -> Result<Option<PlaylistEntry>, UiError> {
    if playlist_id <= 0 {
        return Err(UiError::invalid_input("playlist_id が不正です"));
    }
    let id = parse_video_id(&video_id)?;
    Ok(db.playlist_remove(playlist_id, &id)?)
}

/// `playlist_item_restore`（FR-54、仕様決定 CE）。
/// `playlist_remove` が返したスナップショットをそのまま受け取り、
/// 元の `position` が空いていれば元位置へ、埋まっていれば末尾へ復元する。
/// 戻り値は実際に登録された行（実効 `position` 付き）。
#[tauri::command]
pub async fn playlist_item_restore(
    playlist_id: i64,
    entry: PlaylistEntry,
    db: State<'_, Db>,
) -> Result<PlaylistEntry, UiError> {
    if playlist_id <= 0 {
        return Err(UiError::invalid_input("playlist_id が不正です"));
    }
    if entry.title.chars().count() > MAX_VIDEO_FIELD_LEN {
        return Err(UiError::invalid_input("タイトルが長すぎます"));
    }
    let id = parse_video_id(&entry.video_id)?;
    let v = VideoRef {
        video_id: id,
        title: entry.title.clone(),
        channel_id: entry.channel_id.clone(),
        channel_title: entry.channel_title.clone(),
        thumbnail_url: entry.thumbnail_url.clone(),
    };
    Ok(db.playlist_item_restore(playlist_id, &v, entry.position)?)
}

/// `playlist_import` で使用するプレイリスト名を決める（FR-10、仕様決定 R・DZ）。
/// - `name: Some(n)` のときは明示入力として `validate_playlist_name` を通す（150 文字超はエラー）。
/// - `name: None` のときは `fetched_title` を前後トリムし、空なら「取り込みプレイリスト」、
///   非空なら Unicode スカラ値単位で 150 文字上限へ単純切り詰めした上で検証する。
pub(crate) fn resolve_imported_playlist_name(
    name: Option<&str>,
    fetched_title: Option<&str>,
) -> Result<String, UiError> {
    match name {
        Some(n) => validate_playlist_name(n),
        None => {
            let t = fetched_title.unwrap_or_default().trim();
            if t.is_empty() {
                Ok("取り込みプレイリスト".to_string())
            } else {
                let truncated: String = t.chars().take(MAX_PLAYLIST_NAME_LEN).collect();
                validate_playlist_name(&truncated)
            }
        }
    }
}

/// URL のクエリパラメータ `list` からプレイリスト ID を抽出する（FR-99）。
pub(crate) fn extract_list_id(url: &url::Url) -> Option<String> {
    url.query_pairs()
        .find(|(k, _)| k == "list")
        .map(|(_, v)| v.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// YouTube Data API v3 で取得できない個人用・自動生成プレイリスト ID かを判定する（FR-99）。
/// `WL`（後で見る）、`HL`（履歴）、`LM`（高評価した音楽）、`LL*`（高く評価した動画）、
/// `RD*`（ミックスリスト/ラジオ）は `yt-dlp` 経路へ直行させる。
fn is_api_unsupported_playlist_id(list_id: &str) -> bool {
    let upper = list_id.trim().to_ascii_uppercase();
    upper == "WL"
        || upper == "HL"
        || upper == "LM"
        || upper.starts_with("LL")
        || upper.starts_with("RD")
}

fn now_utc_iso() -> String {
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    crate::util::iso_utc(ms)
}

async fn fetch_remote_playlist(
    url: &str,
    list_id: Option<&str>,
    fetch_title: bool,
    ytapi: &YtApi,
    resolver: &YtDlpResolver,
    db: &Db,
) -> Result<(Option<String>, Vec<PlaylistImportItem>), UiError> {
    if let Some(id) = list_id.filter(|id| !is_api_unsupported_playlist_id(id)) {
        match ytapi.playlist_items(id, fetch_title).await {
            Ok(yt_pl) => {
                let items = yt_pl
                    .items
                    .into_iter()
                    .map(|it| PlaylistImportItem {
                        video: it.video_ref,
                        published_at: it.published_at,
                    })
                    .collect();
                return Ok((yt_pl.title, items));
            }
            Err(YtApiError::NotFound) => {
                return Err(UiError::invalid_input(
                    "プレイリストが見つからないか非公開です",
                ));
            }
            Err(e) => {
                let _ = e.should_fallback();
                tracing::debug!(
                    list_id = %id,
                    error = %e,
                    "YtApi プレイリスト取得に失敗したため yt-dlp へフォールバック"
                );
            }
        }
    }

    let path = resolver
        .resolve(db)
        .await
        .ok_or_else(|| UiError::from(yt::YtError::NotFound))?;
    let (title, refs) = yt::playlist_meta(&path, url, yt::po_token_setting(db).as_deref()).await?;
    let items = refs
        .into_iter()
        .map(|video| PlaylistImportItem {
            video,
            published_at: None,
        })
        .collect();
    Ok((title, items))
}

async fn import_playlist_with(
    url: &str,
    name: Option<&str>,
    ytapi: &YtApi,
    resolver: &YtDlpResolver,
    db: &Db,
) -> Result<Playlist, UiError> {
    // 取り込み対象は YouTube の URL のみ（yt-dlp の任意 extractor 呼び出しを防ぐ）
    let parsed =
        url::Url::parse(url).map_err(|_| UiError::invalid_input("URL を認識できませんでした"))?;
    let host_ok = matches!(parsed.scheme(), "https" | "http")
        && matches!(
            parsed.host_str(),
            Some("youtube.com")
                | Some("youtu.be")
                | Some("music.youtube.com")
                | Some("www.youtube.com")
        );
    if !host_ok {
        return Err(UiError::invalid_input(
            "YouTube のプレイリスト URL を入力してください",
        ));
    }
    if let Some(n) = name {
        validate_playlist_name(n)?;
    }

    let list_id = extract_list_id(&parsed).filter(|id| is_playlist_source_id(id));
    let (fetched_title, items) =
        fetch_remote_playlist(url, list_id.as_deref(), name.is_none(), ytapi, resolver, db).await?;
    if items.is_empty() {
        return Err(UiError::invalid_input(
            "プレイリストから動画を取得できませんでした",
        ));
    }

    let resolved_name = resolve_imported_playlist_name(name, fetched_title.as_deref())?;
    let synced_at = list_id.as_ref().map(|_| now_utc_iso());
    let db = db.clone();
    tokio::task::spawn_blocking(move || -> Result<Playlist, UiError> {
        let mut playlist = db.playlist_create(&resolved_name)?;
        if let Err(e) = db.playlist_add_many_with_published(playlist.id, &items) {
            // 項目登録の失敗で空の一覧を残さない
            let _ = db.playlist_delete(playlist.id);
            return Err(e.into());
        }
        if let (Some(src_id), Some(ts)) = (list_id.as_deref(), synced_at.as_deref()) {
            match db.playlist_set_sync_meta(playlist.id, src_id, ts) {
                Ok(updated) => playlist = updated,
                Err(e) => {
                    let _ = db.playlist_delete(playlist.id);
                    return Err(e.into());
                }
            }
        } else {
            // INSERT OR IGNORE で既存項目は位置維持されるため実件数を取り直す
            playlist.item_count = db.playlist_items(playlist.id)?.len() as i64;
        }
        Ok(playlist)
    })
    .await
    .map_err(|e| UiError::internal(format!("ワーカーが中断しました: {e}")))?
}

fn build_playlist_sync_url(source_list_id: &str) -> String {
    let id = source_list_id.trim();
    let upper = id.to_ascii_uppercase();
    if upper.starts_with("RD") {
        // 動画 ID 起点のミックス（RD<11文字> / RDAMVM<11文字>）は /playlist?list= だと
        // 再生起点 v= が欠落するため、視聴 URL 形式で同期する
        let seed = if id.len() == 13 && crate::model::is_video_id(&id[2..]) {
            Some(&id[2..])
        } else if upper.starts_with("RDAMVM")
            && id.len() == 17
            && crate::model::is_video_id(&id[6..])
        {
            Some(&id[6..])
        } else {
            None
        };
        if let Some(vid) = seed {
            return format!("https://www.youtube.com/watch?v={vid}&list={id}");
        }
    }
    format!("https://www.youtube.com/playlist?list={id}")
}

async fn sync_playlist_with(
    playlist_id: i64,
    ytapi: &YtApi,
    resolver: &YtDlpResolver,
    db: &Db,
) -> Result<PlaylistSyncOutcome, UiError> {
    if playlist_id <= 0 {
        return Err(UiError::invalid_input("playlist_id が不正です"));
    }
    let db_get = db.clone();
    let (playlist, import_epoch) =
        tokio::task::spawn_blocking(move || db_get.playlist_get_with_epoch(playlist_id))
            .await
            .map_err(|e| UiError::internal(format!("ワーカーが中断しました: {e}")))??
            .ok_or(crate::db::DbError::NotFound)?;
    let source_list_id = playlist
        .source_list_id
        .as_deref()
        .map(str::trim)
        .filter(|id| is_playlist_source_id(id))
        .ok_or_else(|| UiError::invalid_input("このプレイリストには同期元 ID がありません"))?
        .to_string();

    let url = build_playlist_sync_url(&source_list_id);
    let (_title, items) =
        fetch_remote_playlist(&url, Some(&source_list_id), false, ytapi, resolver, db).await?;

    let synced_at = now_utc_iso();
    let db_apply = db.clone();
    let (updated_playlist, added_count) = tokio::task::spawn_blocking(move || {
        db_apply.playlist_sync_apply(
            playlist_id,
            &source_list_id,
            import_epoch,
            &items,
            &synced_at,
        )
    })
    .await
    .map_err(|e| UiError::internal(format!("ワーカーが中断しました: {e}")))??;

    Ok(PlaylistSyncOutcome {
        playlist: updated_playlist,
        added_count,
    })
}

/// `playlist_sync`（FR-101、仕様決定 EG）。
/// 取り込み済みプレイリストの `source_list_id` を用い、Phase 94 の取得経路（`fetch_title = false`）で
/// リモート項目を再取得し、未登録の動画のみを末尾へ非破壊追記する。
/// 手動作成リスト（`source_list_id == None`）はネットワークや `yt-dlp` を呼ばずに `invalid_input` を返す。
#[tauri::command]
pub async fn playlist_sync(
    playlist_id: i64,
    ytapi: State<'_, Arc<YtApi>>,
    resolver: State<'_, YtDlpResolver>,
    db: State<'_, Db>,
) -> Result<PlaylistSyncOutcome, UiError> {
    sync_playlist_with(playlist_id, ytapi.inner(), resolver.inner(), db.inner()).await
}

/// `playlist_import`（FR-10・FR-99、仕様決定 R・DZ・EE）。
/// `list` クエリがある公開/限定公開プレイリストは YouTube Data API v3 を優先し、
/// 未設定・クォータ上限・一時障害時や `WL`/`HL`/`LM`/`LL*`/`RD*` 等は
/// yt-dlp の `--flat-playlist` へフォールバックする。
/// `name` 省略時は取り込んだプレイリストのタイトルを使い、
/// それも取れないときは「取り込みプレイリスト」とする（暫定）。
/// 戻り値は作成したプレイリスト（件数集計済み）。
/// 成功時は `library://playlists_changed` で作成したプレイリストを通知し、
/// deep link など画面外からの取り込みを表示中の /library が検知できるようにする
/// （設計書 §3.2）。
#[tauri::command]
pub async fn playlist_import(
    url: String,
    name: Option<String>,
    ytapi: State<'_, Arc<YtApi>>,
    resolver: State<'_, YtDlpResolver>,
    db: State<'_, Db>,
    app: AppHandle,
) -> Result<Playlist, UiError> {
    let playlist = import_playlist_with(
        &url,
        name.as_deref(),
        ytapi.inner(),
        resolver.inner(),
        db.inner(),
    )
    .await?;
    let _ = app.emit("library://playlists_changed", &playlist);
    Ok(playlist)
}

/// `playlist_reorder`（FR-11、仕様決定 T）。
/// 項目順の一括書き換え。渡した `video_ids` は現在の項目と同一集合である
/// 必要があり、一致しない場合は変更せずエラーとする（並行編集の誤適用防止）。
#[tauri::command]
pub async fn playlist_reorder(
    playlist_id: i64,
    video_ids: Vec<String>,
    db: State<'_, Db>,
) -> Result<(), UiError> {
    if playlist_id <= 0 {
        return Err(UiError::invalid_input("playlist_id が不正です"));
    }
    let ids: Vec<String> = video_ids
        .iter()
        .map(|v| parse_video_id(v))
        .collect::<Result<_, _>>()?;
    let db = db.inner().clone();
    // 同一集合の検証は db.playlist_reorder が書き込みトランザクション内で行う
    tokio::task::spawn_blocking(move || db.playlist_reorder(playlist_id, &ids))
        .await
        .map_err(|e| UiError::internal(format!("ワーカーが中断しました: {e}")))?
        .map_err(|e| match e {
            crate::db::DbError::MismatchedItems => {
                UiError::invalid_input("並べ替え後の項目が現在のプレイリスト内容と一致しません")
            }
            e => e.into(),
        })
}

/// `playlist_reorder_lists`（FR-35、仕様決定 BF）。
/// プレイリスト一覧の表示順の一括書き換え。渡した `playlist_ids` は現在の
/// プレイリスト全件と同一集合である必要があり、一致しない場合は変更せず
/// エラーとする（`playlist_reorder` と同じ並行編集の誤適用防止）。
#[tauri::command]
pub async fn playlist_reorder_lists(
    playlist_ids: Vec<i64>,
    db: State<'_, Db>,
) -> Result<(), UiError> {
    if playlist_ids.iter().any(|&id| id <= 0) {
        return Err(UiError::invalid_input("playlist_id が不正です"));
    }
    let db = db.inner().clone();
    // 同一集合の検証は db.playlist_reorder_lists が書き込みトランザクション内で行う
    tokio::task::spawn_blocking(move || db.playlist_reorder_lists(&playlist_ids))
        .await
        .map_err(|e| UiError::internal(format!("ワーカーが中断しました: {e}")))?
        .map_err(|e| match e {
            crate::db::DbError::MismatchedItems => {
                UiError::invalid_input("並べ替え後のプレイリストが現在の一覧と一致しません")
            }
            e => e.into(),
        })
}

/// `playlist_sort`（FR-11、仕様決定 T）。
/// 投稿日時の昇順で一括ソートする（one-shot）。`published_at` の無い項目は
/// 末尾に寄せ、同キー内は現在順を保つ。
#[tauri::command]
pub async fn playlist_sort(playlist_id: i64, db: State<'_, Db>) -> Result<(), UiError> {
    if playlist_id <= 0 {
        return Err(UiError::invalid_input("playlist_id が不正です"));
    }
    let db = db.inner().clone();
    // 全件読み出し→ソート→一括書き換えのためブロッキング側へ逃がす
    tokio::task::spawn_blocking(move || -> Result<(), UiError> {
        let mut items = db.playlist_items(playlist_id)?;
        items.sort_by(|a, b| match (&a.published_at, &b.published_at) {
            (Some(x), Some(y)) => x.cmp(y),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        });
        let ids: Vec<String> = items.into_iter().map(|i| i.video_id).collect();
        db.playlist_reorder(playlist_id, &ids).map_err(|e| match e {
            crate::db::DbError::MismatchedItems => {
                UiError::invalid_input("プレイリストが他の操作で変更されました。再度お試しください")
            }
            e => e.into(),
        })
    })
    .await
    .map_err(|e| UiError::internal(format!("ワーカーが中断しました: {e}")))?
}

/// `playlist_reverse`（FR-11、仕様決定 Z）。
/// 項目順を一括で反転する（one-shot）。YouTube が新しい順で返す
/// プレイリストを投稿日時の昇順へ変える用途に使う。
#[tauri::command]
pub async fn playlist_reverse(playlist_id: i64, db: State<'_, Db>) -> Result<(), UiError> {
    if playlist_id <= 0 {
        return Err(UiError::invalid_input("playlist_id が不正です"));
    }
    let db = db.inner().clone();
    Ok(
        tokio::task::spawn_blocking(move || db.playlist_reverse(playlist_id))
            .await
            .map_err(|e| UiError::internal(format!("ワーカーが中断しました: {e}")))??,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// DB-LD-37: プレイリスト名上限 150 文字（Unicode スカラ値単位）の境界検証と、
    /// `playlist_import` の `name: None` 分岐における 160 文字→150 文字単純切り詰めを
    /// 検証する（FR-7・FR-10、仕様決定 R・DZ）。
    #[test]
    fn playlist_name_limit_150_and_import_truncation() {
        let ok_150 = "あ".repeat(150);
        let over_151 = "あ".repeat(151);
        let fetched_160 = "あ".repeat(160);

        // 明示指定: 150 文字（マルチバイト）は受理、151 文字は拒否
        assert_eq!(validate_playlist_name(&ok_150).unwrap(), ok_150);
        assert!(validate_playlist_name(&over_151).is_err());
        assert_eq!(
            resolve_imported_playlist_name(Some(&ok_150), None).unwrap(),
            ok_150
        );
        assert!(resolve_imported_playlist_name(Some(&over_151), None).is_err());

        // name: None 分岐: 160 文字の取得タイトルは省略記号なしで先頭 150 文字へ切り詰め
        let truncated = resolve_imported_playlist_name(None, Some(&fetched_160)).unwrap();
        assert_eq!(truncated.chars().count(), 150);
        assert_eq!(truncated, ok_150);

        // 前後空白のトリム後に 150 文字ちょうどなら欠損なくそのまま通る
        let padded = format!("  {ok_150}  ");
        assert_eq!(
            resolve_imported_playlist_name(None, Some(&padded)).unwrap(),
            ok_150
        );

        // 空白のみ・None は既定名へフォールバック
        assert_eq!(
            resolve_imported_playlist_name(None, Some("   ")).unwrap(),
            "取り込みプレイリスト"
        );
        assert_eq!(
            resolve_imported_playlist_name(None, None).unwrap(),
            "取り込みプレイリスト"
        );
    }

    struct TempScript(std::path::PathBuf);
    impl Drop for TempScript {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    fn write_fake_ytdlp(tag: &str, windows_body: &str, unix_body: &str) -> TempScript {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let pid = std::process::id();
        #[cfg(windows)]
        let path = std::env::temp_dir().join(format!("ytb_fake_lib_{tag}_{pid}_{nonce}.cmd"));
        #[cfg(not(windows))]
        let path = std::env::temp_dir().join(format!("ytb_fake_lib_{tag}_{pid}_{nonce}.sh"));

        #[cfg(windows)]
        {
            let _ = unix_body;
            std::fs::write(&path, windows_body).unwrap();
        }
        #[cfg(not(windows))]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = windows_body;
            std::fs::write(&path, unix_body).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        TempScript(path)
    }

    /// DB-LD-43: `playlist_import`（`import_playlist_with`）で Data API v3 優先取り込み
    /// （`yt-dlp` 不在でも成功、非公開除外、`videoOwnerChannelId` 抽出、`published_at` `+00:00` 保存と
    /// 既存非 NULL `published_at` 保持、`name: Some` で `playlists.list` スキップ）、全件非公開/空/404 の
    /// `invalid_input`（`yt-dlp` 非フォールバック）、`WL`/`HL`/`LL`/`LM`/`RD...` のバイパス、および
    /// API 障害時（`500`/`403 quotaExceeded`）の `yt-dlp` フォールバックを検証する（FR-99、仕様決定 EE）。
    #[tokio::test]
    async fn playlist_import_prefers_ytapi_and_falls_back() {
        use std::collections::VecDeque;
        use std::sync::{Arc as StdArc, Mutex as StdMutex};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        assert!(is_api_unsupported_playlist_id("WL"));
        assert!(is_api_unsupported_playlist_id("hl"));
        assert!(is_api_unsupported_playlist_id("lm"));
        assert!(is_api_unsupported_playlist_id("LLxyz"));
        assert!(is_api_unsupported_playlist_id("RDMMabc"));
        assert!(!is_api_unsupported_playlist_id("PL12345"));

        let db = Db::connect_in_memory().unwrap();
        let resolver = YtDlpResolver::new(None);

        // 既存動画を 2 件投入: 1 件は非 NULL published_at あり、もう 1 件は NULL
        let seed_pl = db.playlist_create("Seed").unwrap();
        db.playlist_add_many_with_published(
            seed_pl.id,
            &[
                PlaylistImportItem {
                    video: VideoRef {
                        video_id: "existPub001".to_string(),
                        title: "Existing Pub".to_string(),
                        channel_id: None,
                        channel_title: None,
                        thumbnail_url: None,
                    },
                    published_at: Some("2024-01-01T00:00:00+00:00".to_string()),
                },
                PlaylistImportItem {
                    video: VideoRef {
                        video_id: "nullPub0002".to_string(),
                        title: "Null Pub".to_string(),
                        channel_id: None,
                        channel_title: None,
                        thumbnail_url: None,
                    },
                    published_at: None,
                },
            ],
        )
        .unwrap();

        let recorded: StdArc<StdMutex<Vec<String>>> = StdArc::new(StdMutex::new(Vec::new()));
        let rec_clone = StdArc::clone(&recorded);
        let replies: StdArc<StdMutex<VecDeque<(u16, String)>>> =
            StdArc::new(StdMutex::new(VecDeque::from(vec![
                // 1) name: None 取り込み: playlists.list
                (
                    200,
                    r#"{"items":[{"snippet":{"title":"  API Playlist  "}}]}"#.to_string(),
                ),
                // 2) playlistItems.list (existPub001 + nullPub0002 + private 1件)
                (
                    200,
                    r#"{"items":[
                        {"snippet":{"title":"Updated 1","channelId":"UCowner00000000000000001","videoOwnerChannelId":"UCvideo00000000000000001","videoOwnerChannelTitle":"Video CH 1","resourceId":{"videoId":"existPub001"}},"contentDetails":{"videoId":"existPub001","videoPublishedAt":"2026-05-01T00:00:00Z"},"status":{"privacyStatus":"public"}},
                        {"snippet":{"title":"Updated 2","videoOwnerChannelId":"UCvideo00000000000000002","videoOwnerChannelTitle":"Video CH 2","resourceId":{"videoId":"nullPub0002"}},"contentDetails":{"videoId":"nullPub0002","videoPublishedAt":"2026-05-02T12:00:00Z"},"status":{"privacyStatus":"unlisted"}},
                        {"snippet":{"title":"Hidden","resourceId":{"videoId":"privVid0003"}},"status":{"privacyStatus":"private"}}
                    ]}"#
                        .to_string(),
                ),
                // 3) name: Some 取り込み: playlists.list スキップされ直接 playlistItems.list
                (
                    200,
                    r#"{"items":[{"snippet":{"title":"Named V","resourceId":{"videoId":"namedVid004"}},"contentDetails":{"videoPublishedAt":"2026-05-03T00:00:00Z"},"status":{"privacyStatus":"public"}}]}"#
                        .to_string(),
                ),
                // 4) 全件 private (name: Some) -> empty items -> invalid_input
                (
                    200,
                    r#"{"items":[{"snippet":{"title":"Priv","resourceId":{"videoId":"privOnly005"}},"status":{"privacyStatus":"private"}}]}"#
                        .to_string(),
                ),
                // 5) playlists.list 空 items -> NotFound -> invalid_input
                (200, r#"{"items":[]}"#.to_string()),
                // 6) playlistItems.list 404 -> NotFound -> invalid_input
                (404, r#"{"error":{"code":404}}"#.to_string()),
                // 7) 500 -> yt-dlp フォールバック
                (500, r#"{"error":{"code":500}}"#.to_string()),
                // 8) 403 quotaExceeded -> yt-dlp フォールバック
                (
                    403,
                    r#"{"error":{"errors":[{"reason":"quotaExceeded"}]}}"#.to_string(),
                ),
            ])));
        let rep_clone = StdArc::clone(&replies);

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let srv = tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let mut buf = [0u8; 2048];
                let n = stream.read(&mut buf).await.unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]);
                let line = req.lines().next().unwrap_or_default().to_string();
                rec_clone.lock().unwrap().push(line);
                let (status, body) = rep_clone
                    .lock()
                    .unwrap()
                    .pop_front()
                    .unwrap_or((200, r#"{"items":[]}"#.to_string()));
                let reason = match status {
                    200 => "OK",
                    403 => "Forbidden",
                    404 => "Not Found",
                    500 => "Internal Server Error",
                    _ => "Error",
                };
                let resp = format!(
                    "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(resp.as_bytes()).await;
            }
        });
        let ytapi =
            YtApi::with_test_base_url(Some("AIzaSyLD_43".to_string()), format!("http://{addr}"));

        // 1 & 2) yt-dlp 失敗スクリプト下でも API 経由取り込みが成功し、COALESCE が効く
        let fail_script = write_fake_ytdlp(
            "ld43_fail",
            "@echo off\r\nexit /b 1\r\n",
            "#!/bin/sh\nexit 1\n",
        );
        db.setting_set(
            crate::yt::SETTING_YTDLP_PATH,
            fail_script.0.to_str().unwrap(),
        )
        .unwrap();

        let pl1 = import_playlist_with(
            "https://www.youtube.com/playlist?list=PLapi001",
            None,
            &ytapi,
            &resolver,
            &db,
        )
        .await
        .unwrap();
        assert_eq!(pl1.name, "API Playlist");
        assert_eq!(pl1.item_count, 2);
        let entries1 = db.playlist_items(pl1.id).unwrap();
        assert_eq!(entries1[0].video_id, "existPub001");
        assert_eq!(
            entries1[0].channel_id.as_deref(),
            Some("UCvideo00000000000000001")
        );
        // 既存の非 NULL published_at が保持されること
        assert_eq!(
            entries1[0].published_at.as_deref(),
            Some("2024-01-01T00:00:00+00:00")
        );
        // 元々 NULL だった行には API からの +00:00 が補完されること
        assert_eq!(entries1[1].video_id, "nullPub0002");
        assert_eq!(
            entries1[1].published_at.as_deref(),
            Some("2026-05-02T12:00:00+00:00")
        );

        // name: Some 指定時は playlists.list をスキップする
        let pl2 = import_playlist_with(
            "https://www.youtube.com/playlist?list=PLapi002",
            Some("Explicit Name"),
            &ytapi,
            &resolver,
            &db,
        )
        .await
        .unwrap();
        assert_eq!(pl2.name, "Explicit Name");
        assert_eq!(pl2.item_count, 1);
        {
            let reqs = recorded.lock().unwrap();
            assert_eq!(reqs.len(), 3);
            assert!(reqs[0].contains("/playlists?"));
            assert!(reqs[1].contains("/playlistItems?"));
            assert!(reqs[2].contains("/playlistItems?"));
        }

        // 以降の異常系・バイパス・フォールバック検証のため成功する偽 yt-dlp に差し替える
        let ok_script = write_fake_ytdlp(
            "ld43_ok",
            "@echo off\r\necho {\"title\":\"Fallback PL\",\"entries\":[{\"id\":\"fbVid000001\",\"title\":\"FB Video\",\"channel_id\":\"UCfb00000000000000000001\",\"channel\":\"FB CH\"}]}\r\n",
            "#!/bin/sh\nprintf '%s\\n' '{\"title\":\"Fallback PL\",\"entries\":[{\"id\":\"fbVid000001\",\"title\":\"FB Video\",\"channel_id\":\"UCfb00000000000000000001\",\"channel\":\"FB CH\"}]}'\n",
        );
        db.setting_set(crate::yt::SETTING_YTDLP_PATH, ok_script.0.to_str().unwrap())
            .unwrap();

        // 全件 private / playlists.list 空 / 404 は yt-dlp へフォールバックせず invalid_input
        let e_priv = import_playlist_with(
            "https://www.youtube.com/playlist?list=PLprivOnly",
            Some("PrivOnly"),
            &ytapi,
            &resolver,
            &db,
        )
        .await
        .unwrap_err();
        assert_eq!(e_priv.code, "invalid_input");

        let e_empty = import_playlist_with(
            "https://www.youtube.com/playlist?list=PLemptyMeta",
            None,
            &ytapi,
            &resolver,
            &db,
        )
        .await
        .unwrap_err();
        assert_eq!(e_empty.code, "invalid_input");

        let e_404 = import_playlist_with(
            "https://www.youtube.com/playlist?list=PLnotFound404",
            Some("NF404"),
            &ytapi,
            &resolver,
            &db,
        )
        .await
        .unwrap_err();
        assert_eq!(e_404.code, "invalid_input");

        // WL / HL / LL / LM / RD... は ytapi を呼ばず yt-dlp へ直行する
        let before_bypass_reqs = recorded.lock().unwrap().len();
        let pl_wl = import_playlist_with(
            "https://www.youtube.com/playlist?list=WL",
            None,
            &ytapi,
            &resolver,
            &db,
        )
        .await
        .unwrap();
        assert_eq!(pl_wl.name, "Fallback PL");
        let pl_rd = import_playlist_with(
            "https://www.youtube.com/watch?v=fbVid000001&list=RDfbVid000001",
            Some("Radio Mix"),
            &ytapi,
            &resolver,
            &db,
        )
        .await
        .unwrap();
        assert_eq!(pl_rd.name, "Radio Mix");
        assert_eq!(recorded.lock().unwrap().len(), before_bypass_reqs);

        // 500（Cooldown 入り）で yt-dlp フォールバック
        let pl_500 = import_playlist_with(
            "https://www.youtube.com/playlist?list=PLerr500",
            None,
            &ytapi,
            &resolver,
            &db,
        )
        .await
        .unwrap();
        assert_eq!(pl_500.name, "Fallback PL");

        // 403 quotaExceeded（別インスタンスで検証）で yt-dlp フォールバック
        let ytapi_q =
            YtApi::with_test_base_url(Some("AIzaSyLD_43_Q".to_string()), format!("http://{addr}"));
        let pl_q = import_playlist_with(
            "https://www.youtube.com/playlist?list=PLerrQuota",
            Some("Quota FB"),
            &ytapi_q,
            &resolver,
            &db,
        )
        .await
        .unwrap();
        assert_eq!(pl_q.name, "Quota FB");

        srv.abort();
    }

    /// DB-LD-47: `import_playlist_with` による `source_list_id` / `+00:00` `synced_at` 永続化と、
    /// `sync_playlist_with` による差分追記同期（`/playlists` スキップ・新規のみ追記・`added_count` 返却・
    /// `FR-11` 並びと `queue_state.current_index` 維持・手動リスト即時拒否・HTTP 404 非フォールバック・
    /// HTTP 200 空 0 件非破壊更新・`WL`/`RD...` と 500/403 時 `yt-dlp` フォールバック）を検証する
    /// （FR-101、仕様決定 EG）。
    #[tokio::test]
    async fn playlist_import_persists_sync_meta_and_playlist_sync_appends_diff() {
        use std::collections::VecDeque;
        use std::sync::{Arc as StdArc, Mutex as StdMutex};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let db = Db::connect_in_memory().unwrap();
        let resolver = YtDlpResolver::new(None);

        let recorded: StdArc<StdMutex<Vec<String>>> = StdArc::new(StdMutex::new(Vec::new()));
        let rec_clone = StdArc::clone(&recorded);
        let replies: StdArc<StdMutex<VecDeque<(u16, String)>>> =
            StdArc::new(StdMutex::new(VecDeque::from(vec![
                // 1) import_playlist_with (name: None): playlists.list
                (
                    200,
                    r#"{"items":[{"snippet":{"title":"Sync PL"}}]}"#.to_string(),
                ),
                // 2) import_playlist_with: playlistItems.list (v1, v2)
                (
                    200,
                    r#"{"items":[
                        {"snippet":{"title":"V1","videoOwnerChannelId":"UCsync000000000000000001","resourceId":{"videoId":"syncVid0001"}},"contentDetails":{"videoPublishedAt":"2026-05-01T00:00:00Z"},"status":{"privacyStatus":"public"}},
                        {"snippet":{"title":"V2","videoOwnerChannelId":"UCsync000000000000000002","resourceId":{"videoId":"syncVid0002"}},"contentDetails":{"videoPublishedAt":"2026-05-02T00:00:00Z"},"status":{"privacyStatus":"unlisted"}}
                    ]}"#
                        .to_string(),
                ),
                // 3) sync_playlist_with #1: /playlists スキップされ /playlistItems のみ (v2 既存 + v3 新規 + private)
                (
                    200,
                    r#"{"items":[
                        {"snippet":{"title":"V2 Renamed","resourceId":{"videoId":"syncVid0002"}},"status":{"privacyStatus":"public"}},
                        {"snippet":{"title":"V3 New","videoOwnerChannelId":"UCsync000000000000000003","resourceId":{"videoId":"syncVid0003"}},"contentDetails":{"videoPublishedAt":"2026-05-03T00:00:00Z"},"status":{"privacyStatus":"public"}},
                        {"snippet":{"title":"Priv","resourceId":{"videoId":"syncPriv004"}},"status":{"privacyStatus":"private"}}
                    ]}"#
                        .to_string(),
                ),
                // 4) sync_playlist_with #2 (HTTP 200 全件 private -> 0 件非破壊更新)
                (
                    200,
                    r#"{"items":[{"snippet":{"title":"Priv Only","resourceId":{"videoId":"syncPriv005"}},"status":{"privacyStatus":"private"}}]}"#
                        .to_string(),
                ),
                // 5) sync_playlist_with #3 (HTTP 404 -> invalid_input、yt-dlp 非フォールバック)
                (404, r#"{"error":{"code":404}}"#.to_string()),
                // 6) sync_playlist_with #4 (HTTP 403 playlistItemsNotAccessible -> yt-dlp フォールバック)
                (
                    403,
                    r#"{"error":{"errors":[{"reason":"playlistItemsNotAccessible"}]}}"#.to_string(),
                ),
                // 7) sync_playlist_with #5 (HTTP 500 -> yt-dlp フォールバック)
                (500, r#"{"error":{"code":500}}"#.to_string()),
            ])));
        let rep_clone = StdArc::clone(&replies);

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let srv = tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let mut buf = [0u8; 2048];
                let n = stream.read(&mut buf).await.unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]);
                let line = req.lines().next().unwrap_or_default().to_string();
                rec_clone.lock().unwrap().push(line);
                let (status, body) = rep_clone
                    .lock()
                    .unwrap()
                    .pop_front()
                    .unwrap_or((200, r#"{"items":[]}"#.to_string()));
                let reason = match status {
                    200 => "OK",
                    403 => "Forbidden",
                    404 => "Not Found",
                    500 => "Internal Server Error",
                    _ => "Error",
                };
                let resp = format!(
                    "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(resp.as_bytes()).await;
            }
        });

        let ytapi =
            YtApi::with_test_base_url(Some("AIzaSyLD_47".to_string()), format!("http://{addr}"));
        let fail_script = write_fake_ytdlp(
            "ld47_fail",
            "@echo off\r\nexit /b 1\r\n",
            "#!/bin/sh\nexit 1\n",
        );
        db.setting_set(
            crate::yt::SETTING_YTDLP_PATH,
            fail_script.0.to_str().unwrap(),
        )
        .unwrap();

        // 1) import_playlist_with が source_list_id と +00:00 synced_at を保存する
        let imported = import_playlist_with(
            "https://www.youtube.com/playlist?list=PLsyncTarget01",
            None,
            &ytapi,
            &resolver,
            &db,
        )
        .await
        .unwrap();
        assert_eq!(imported.name, "Sync PL");
        assert_eq!(imported.item_count, 2);
        assert_eq!(imported.source_list_id.as_deref(), Some("PLsyncTarget01"));
        let imported_ts = imported.synced_at.as_deref().unwrap();
        assert!(imported_ts.ends_with("+00:00"), "ts={imported_ts}");
        assert!(crate::util::iso8601_ms(imported_ts).is_some());

        // 手動追加 + FR-11 並べ替え + queue_state.current_index 保存
        db.playlist_add(
            imported.id,
            &VideoRef {
                video_id: "manualVid99".to_string(),
                title: "Manual".to_string(),
                channel_id: None,
                channel_title: None,
                thumbnail_url: None,
            },
        )
        .unwrap();
        db.playlist_reorder(
            imported.id,
            &[
                "manualVid99".to_string(),
                "syncVid0002".to_string(),
                "syncVid0001".to_string(),
            ],
        )
        .unwrap();
        db.queue_playlist_save(imported.id, 1).unwrap();

        // 2) sync_playlist_with: /playlists を呼ばず /playlistItems のみ、新規 1 件のみ末尾追加
        let reqs_before_sync = recorded.lock().unwrap().len();
        let outcome1 = sync_playlist_with(imported.id, &ytapi, &resolver, &db)
            .await
            .unwrap();
        assert_eq!(outcome1.added_count, 1);
        assert_eq!(outcome1.playlist.item_count, 4);
        assert!(outcome1
            .playlist
            .synced_at
            .as_deref()
            .unwrap()
            .ends_with("+00:00"));
        {
            let reqs = recorded.lock().unwrap();
            assert_eq!(reqs.len() - reqs_before_sync, 1);
            assert!(reqs[reqs_before_sync].contains("/playlistItems?"));
        }
        let entries = db.playlist_items(imported.id).unwrap();
        assert_eq!(
            entries
                .iter()
                .map(|e| e.video_id.as_str())
                .collect::<Vec<_>>(),
            ["manualVid99", "syncVid0002", "syncVid0001", "syncVid0003"]
        );
        assert_eq!(db.queue_playlist_load().unwrap()[0].current_index, 1);

        // 3) HTTP 200 有効 0 件でも非破壊・added_count=0 で成功
        let outcome_zero = sync_playlist_with(imported.id, &ytapi, &resolver, &db)
            .await
            .unwrap();
        assert_eq!(outcome_zero.added_count, 0);
        assert_eq!(outcome_zero.playlist.item_count, 4);

        // 4) 手動作成リスト (source_list_id == None) は通信せず即座に invalid_input
        let manual_pl = db.playlist_create("Manual Only").unwrap();
        let reqs_before_manual = recorded.lock().unwrap().len();
        let err_manual = sync_playlist_with(manual_pl.id, &ytapi, &resolver, &db)
            .await
            .unwrap_err();
        assert_eq!(err_manual.code, "invalid_input");
        assert_eq!(recorded.lock().unwrap().len(), reqs_before_manual);

        // 不正 ID / 不在 ID
        let err_bad_id = sync_playlist_with(0, &ytapi, &resolver, &db)
            .await
            .unwrap_err();
        assert_eq!(err_bad_id.code, "invalid_input");
        let err_missing = sync_playlist_with(99999, &ytapi, &resolver, &db)
            .await
            .unwrap_err();
        assert_eq!(err_missing.code, "db");

        // 5) 成功する偽 yt-dlp に差し替えても 404 はフォールバックせず invalid_input
        let ok_script = write_fake_ytdlp(
            "ld47_ok",
            "@echo off\r\necho {\"title\":\"FB\",\"entries\":[{\"id\":\"fbSync00001\",\"title\":\"FB Sync\",\"channel_id\":\"UCfb00000000000000000001\",\"channel\":\"FB CH\"}]}\r\n",
            "#!/bin/sh\nprintf '%s\\n' '{\"title\":\"FB\",\"entries\":[{\"id\":\"fbSync00001\",\"title\":\"FB Sync\",\"channel_id\":\"UCfb00000000000000000001\",\"channel\":\"FB CH\"}]}'\n",
        );
        db.setting_set(crate::yt::SETTING_YTDLP_PATH, ok_script.0.to_str().unwrap())
            .unwrap();

        let err_404 = sync_playlist_with(imported.id, &ytapi, &resolver, &db)
            .await
            .unwrap_err();
        assert_eq!(err_404.code, "invalid_input");
        // 404 で yt-dlp の fbSync00001 が混入していないこと
        assert_eq!(db.playlist_items(imported.id).unwrap().len(), 4);

        // 6) 403 playlistItemsNotAccessible -> yt-dlp フォールバックで fbSync00001 を追記
        let outcome_403 = sync_playlist_with(imported.id, &ytapi, &resolver, &db)
            .await
            .unwrap();
        assert_eq!(outcome_403.added_count, 1);
        assert_eq!(outcome_403.playlist.item_count, 5);

        // 7) 500 -> yt-dlp フォールバック（fbSync00001 は登録済みなので added_count=0）
        let outcome_500 = sync_playlist_with(imported.id, &ytapi, &resolver, &db)
            .await
            .unwrap();
        assert_eq!(outcome_500.added_count, 0);

        // 8) WL / RD... 取り込み時は source_list_id 保存、同期時も ytapi バイパスで yt-dlp 直行
        //    動画 ID 起点の RD... は watch?v=<id>&list=RD<id> を組み立てて渡すこと
        assert_eq!(
            build_playlist_sync_url("WL"),
            "https://www.youtube.com/playlist?list=WL"
        );
        assert_eq!(
            build_playlist_sync_url("RDfbVid000001"),
            "https://www.youtube.com/watch?v=fbVid000001&list=RDfbVid000001"
        );
        assert_eq!(
            build_playlist_sync_url("RDAMVMfbVid000001"),
            "https://www.youtube.com/watch?v=fbVid000001&list=RDAMVMfbVid000001"
        );
        assert_eq!(
            build_playlist_sync_url("RDCLAK5uy_abc123"),
            "https://www.youtube.com/playlist?list=RDCLAK5uy_abc123"
        );

        let reqs_before_wl = recorded.lock().unwrap().len();
        let wl_pl = import_playlist_with(
            "https://www.youtube.com/playlist?list=WL",
            Some("Watch Later"),
            &ytapi,
            &resolver,
            &db,
        )
        .await
        .unwrap();
        assert_eq!(wl_pl.source_list_id.as_deref(), Some("WL"));
        assert!(wl_pl.synced_at.as_deref().unwrap().ends_with("+00:00"));
        let wl_sync = sync_playlist_with(wl_pl.id, &ytapi, &resolver, &db)
            .await
            .unwrap();
        assert_eq!(wl_sync.added_count, 0);
        assert_eq!(recorded.lock().unwrap().len(), reqs_before_wl);

        // 動画 ID を含む RD 系ミックス URL の取り込みと同期（引数 URL 検証付き偽 yt-dlp）
        let rd_script = write_fake_ytdlp(
            "ld47_rd_url",
            "@echo off\r\nif \"%~1\"==\"https://www.youtube.com/watch?v=fbVid000001&list=RDfbVid000001\" (\r\n  echo {\"title\":\"RD\",\"entries\":[{\"id\":\"rdMixVid001\",\"title\":\"RD Mix\",\"channel_id\":\"UCfb00000000000000000001\",\"channel\":\"RD CH\"}]}\r\n  exit /b 0\r\n)\r\nexit /b 1\r\n",
            "#!/bin/sh\nif [ \"$1\" = \"https://www.youtube.com/watch?v=fbVid000001&list=RDfbVid000001\" ]; then\n  printf '%s\\n' '{\"title\":\"RD\",\"entries\":[{\"id\":\"rdMixVid001\",\"title\":\"RD Mix\",\"channel_id\":\"UCfb00000000000000000001\",\"channel\":\"RD CH\"}]}'\n  exit 0\nfi\nexit 1\n",
        );
        db.setting_set(crate::yt::SETTING_YTDLP_PATH, rd_script.0.to_str().unwrap())
            .unwrap();
        let rd_pl = import_playlist_with(
            "https://www.youtube.com/watch?v=fbVid000001&list=RDfbVid000001",
            Some("Radio Mix Sync"),
            &ytapi,
            &resolver,
            &db,
        )
        .await
        .unwrap();
        assert_eq!(rd_pl.source_list_id.as_deref(), Some("RDfbVid000001"));
        let rd_sync = sync_playlist_with(rd_pl.id, &ytapi, &resolver, &db)
            .await
            .unwrap();
        assert_eq!(rd_sync.added_count, 0);
        assert_eq!(recorded.lock().unwrap().len(), reqs_before_wl);

        srv.abort();
    }
}
