//! 履歴・お気に入り・プレイリスト系コマンド（FR-7）。

use super::parse_video_id;
use crate::db::Db;
use crate::error::UiError;
use crate::model::{FavoriteEntry, Playlist, PlaylistEntry, VideoRef, WatchHistory};
use crate::yt::{self, YtDlpResolver};
use tauri::{AppHandle, Emitter, State};

/// 動画 1 件分の視聴履歴。`video_id` は `play_video` と同じ正規化を経る。
#[tauri::command]
pub fn history_get(video_id: String, db: State<'_, Db>) -> Result<Option<WatchHistory>, UiError> {
    let id = parse_video_id(&video_id)?;
    Ok(db.history_get(&id)?)
}

/// 履歴一覧の既定・最大件数。
const HISTORY_LIST_LIMIT: u32 = 500;
/// プレイリスト名の最大長。
const MAX_PLAYLIST_NAME_LEN: usize = 100;
/// 動画タイトル等の上限（`videos.title` の入力値）。
const MAX_VIDEO_FIELD_LEN: usize = 1024;

/// `history_list`（FR-7）。新しく見た順。`limit` 省略時は 500、上限 1000。
#[tauri::command]
pub fn history_list(limit: Option<u32>, db: State<'_, Db>) -> Result<Vec<WatchHistory>, UiError> {
    let limit = limit.unwrap_or(HISTORY_LIST_LIMIT).min(1000);
    Ok(db.history_list(limit)?)
}

/// `history_remove`（FR-7、仕様決定 I の手動削除）。
#[tauri::command]
pub fn history_remove(video_id: String, db: State<'_, Db>) -> Result<(), UiError> {
    let id = parse_video_id(&video_id)?;
    Ok(db.history_remove(&id)?)
}

/// `VideoRef` の入力検証と動画 ID 正規化。
fn validate_video_ref(v: &VideoRef) -> Result<String, UiError> {
    let id = parse_video_id(&v.video_id)?;
    if v.title.chars().count() > MAX_VIDEO_FIELD_LEN {
        return Err(UiError::invalid_input("タイトルが長すぎます"));
    }
    Ok(id)
}

/// `favorite_add`（FR-7）。動画メタは `videos` 台帳へ登録される。
#[tauri::command]
pub fn favorite_add(video: VideoRef, db: State<'_, Db>) -> Result<(), UiError> {
    let id = validate_video_ref(&video)?;
    let mut v = video;
    v.video_id = id;
    Ok(db.favorite_add(&v)?)
}

/// `favorite_remove`（FR-7）。
#[tauri::command]
pub fn favorite_remove(video_id: String, db: State<'_, Db>) -> Result<(), UiError> {
    let id = parse_video_id(&video_id)?;
    Ok(db.favorite_remove(&id)?)
}

/// `favorite_list`（FR-7）。追加が新しい順。
#[tauri::command]
pub fn favorite_list(db: State<'_, Db>) -> Result<Vec<FavoriteEntry>, UiError> {
    Ok(db.favorite_list()?)
}

/// `playlist_list`（FR-7）。件数集計付き。
#[tauri::command]
pub fn playlist_list(db: State<'_, Db>) -> Result<Vec<Playlist>, UiError> {
    Ok(db.playlist_list()?)
}

/// プレイリスト名の検証（トリム・非空・長さ）。
fn validate_playlist_name(name: &str) -> Result<String, UiError> {
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
pub fn playlist_create(name: String, db: State<'_, Db>) -> Result<Playlist, UiError> {
    let name = validate_playlist_name(&name)?;
    Ok(db.playlist_create(&name)?)
}

/// `playlist_rename`（FR-7）。
#[tauri::command]
pub fn playlist_rename(playlist_id: i64, name: String, db: State<'_, Db>) -> Result<(), UiError> {
    if playlist_id <= 0 {
        return Err(UiError::invalid_input("playlist_id が不正です"));
    }
    let name = validate_playlist_name(&name)?;
    Ok(db.playlist_rename(playlist_id, &name)?)
}

/// `playlist_delete`（FR-7）。中身は CASCADE で消える。
#[tauri::command]
pub fn playlist_delete(playlist_id: i64, db: State<'_, Db>) -> Result<(), UiError> {
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
pub fn playlist_items(
    playlist_id: i64,
    after_position: Option<i64>,
    limit: Option<u32>,
    db: State<'_, Db>,
) -> Result<Vec<PlaylistEntry>, UiError> {
    if playlist_id <= 0 {
        return Err(UiError::invalid_input("playlist_id が不正です"));
    }
    Ok(db.playlist_items_page(playlist_id, after_position, limit)?)
}

/// `playlist_add`（FR-7）。末尾への追加、重複は位置を維持して無視。
#[tauri::command]
pub fn playlist_add(playlist_id: i64, video: VideoRef, db: State<'_, Db>) -> Result<(), UiError> {
    if playlist_id <= 0 {
        return Err(UiError::invalid_input("playlist_id が不正です"));
    }
    let id = validate_video_ref(&video)?;
    let mut v = video;
    v.video_id = id;
    Ok(db.playlist_add(playlist_id, &v)?)
}

/// `playlist_remove`（FR-7）。
#[tauri::command]
pub fn playlist_remove(
    playlist_id: i64,
    video_id: String,
    db: State<'_, Db>,
) -> Result<(), UiError> {
    if playlist_id <= 0 {
        return Err(UiError::invalid_input("playlist_id が不正です"));
    }
    let id = parse_video_id(&video_id)?;
    Ok(db.playlist_remove(playlist_id, &id)?)
}

/// `playlist_import`（FR-10、仕様決定 R）。
/// YouTube プレイリストを yt-dlp の `--flat-playlist` で取り込み、
/// ローカルのプレイリストへ登録する。各項目は `video_upsert` 経由で
/// `videos` 台帳に集約し、`published_at` は未取得のまま（ソート時は末尾）。
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
    resolver: State<'_, YtDlpResolver>,
    db: State<'_, Db>,
    app: AppHandle,
) -> Result<Playlist, UiError> {
    // 取り込み対象は YouTube の URL のみ（yt-dlp の任意 extractor 呼び出しを防ぐ）
    let parsed =
        url::Url::parse(&url).map_err(|_| UiError::invalid_input("URL を認識できませんでした"))?;
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
    let path = resolver
        .resolve(&db)
        .await
        .ok_or_else(|| UiError::from(yt::YtError::NotFound))?;
    let (fetched_title, items) = yt::playlist_meta(&path, &url).await?;
    if items.is_empty() {
        return Err(UiError::invalid_input(
            "プレイリストから動画を取得できませんでした",
        ));
    }
    let name = match name {
        Some(n) => validate_playlist_name(&n)?,
        None => {
            let t = fetched_title.unwrap_or_default();
            let t = t.trim();
            if t.is_empty() {
                "取り込みプレイリスト".to_string()
            } else {
                validate_playlist_name(t)?
            }
        }
    };
    let mut playlist = db.playlist_create(&name)?;
    if let Err(e) = db.playlist_add_many(playlist.id, &items) {
        // 項目登録の失敗で空の一覧を残さない
        let _ = db.playlist_delete(playlist.id);
        return Err(e.into());
    }
    // INSERT OR IGNORE で既存項目は位置維持されるため実件数を取り直す
    playlist.item_count = db.playlist_items(playlist.id)?.len() as i64;
    let _ = app.emit("library://playlists_changed", &playlist);
    Ok(playlist)
}

/// `playlist_reorder`（FR-11、仕様決定 T）。
/// 項目順の一括書き換え。渡した `video_ids` は現在の項目と同一集合である
/// 必要があり、一致しない場合は変更せずエラーとする（並行編集の誤適用防止）。
#[tauri::command]
pub fn playlist_reorder(
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
    // 同一集合の検証は db.playlist_reorder が書き込みトランザクション内で行う
    db.playlist_reorder(playlist_id, &ids).map_err(|e| match e {
        crate::db::DbError::MismatchedItems => {
            UiError::invalid_input("並べ替え後の項目が現在のプレイリスト内容と一致しません")
        }
        e => e.into(),
    })
}

/// `playlist_sort`（FR-11、仕様決定 T）。
/// 投稿日時の昇順で一括ソートする（one-shot）。`published_at` の無い項目は
/// 末尾に寄せ、同キー内は現在順を保つ。
#[tauri::command]
pub fn playlist_sort(playlist_id: i64, db: State<'_, Db>) -> Result<(), UiError> {
    if playlist_id <= 0 {
        return Err(UiError::invalid_input("playlist_id が不正です"));
    }
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
}

/// `playlist_reverse`（FR-11、仕様決定 Z）。
/// 項目順を一括で反転する（one-shot）。YouTube が新しい順で返す
/// プレイリストを投稿日時の昇順へ変える用途に使う。
#[tauri::command]
pub fn playlist_reverse(playlist_id: i64, db: State<'_, Db>) -> Result<(), UiError> {
    if playlist_id <= 0 {
        return Err(UiError::invalid_input("playlist_id が不正です"));
    }
    Ok(db.playlist_reverse(playlist_id)?)
}
