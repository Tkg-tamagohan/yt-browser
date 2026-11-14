//! 履歴・お気に入り・プレイリスト・あとで見る・横断検索系コマンド（FR-7、FR-59、FR-60）。

use super::parse_video_id;
use crate::db::Db;
use crate::error::UiError;
use crate::model::{
    FavoriteEntry, HistoryCursor, HistoryPage, LibrarySearchItem, Playlist, PlaylistEntry,
    VideoRef, WatchHistory, WatchLaterEntry,
};
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
/// `cursor` は前ページの `next_cursor` をそのまま渡すページングカーソル
/// （FR-52、仕様決定 CC）。`query` はタイトル・チャンネル名の部分一致
/// 絞り込み（FR-61、仕様決定 CL）。戻り値は `items` と `next_cursor` の
/// ページで、続きが無ければ `next_cursor` は null。
#[tauri::command]
pub fn history_list(
    cursor: Option<HistoryCursor>,
    query: Option<String>,
    limit: Option<u32>,
    db: State<'_, Db>,
) -> Result<HistoryPage, UiError> {
    let limit = limit.unwrap_or(HISTORY_LIST_LIMIT).min(1000);
    Ok(db.history_list(cursor.as_ref(), query.as_deref(), limit)?)
}

/// `history_remove`（FR-7、仕様決定 I の手動削除）。
/// 削除した行のスナップショットを返す（FR-54、仕様決定 CE の
/// トースト「元に戻す」→ `history_restore` の入力）。対象行が無ければ null。
#[tauri::command]
pub fn history_remove(
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
pub fn history_restore(entry: WatchHistory, db: State<'_, Db>) -> Result<(), UiError> {
    let id = parse_video_id(&entry.video_id)?;
    let mut e = entry;
    e.video_id = id;
    Ok(db.history_restore(&e)?)
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
pub fn favorite_add(video: VideoRef, db: State<'_, Db>) -> Result<(), UiError> {
    let id = validate_video_ref(&video)?;
    let mut v = video;
    v.video_id = id;
    Ok(db.favorite_add(&v)?)
}

/// `favorite_remove`（FR-7）。
/// 削除した行のスナップショットを返す（FR-54、仕様決定 CE の
/// トースト「元に戻す」→ `favorite_restore` の入力）。対象行が無ければ null。
#[tauri::command]
pub fn favorite_remove(
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
pub fn favorite_restore(entry: FavoriteEntry, db: State<'_, Db>) -> Result<(), UiError> {
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
pub fn favorite_list(db: State<'_, Db>) -> Result<Vec<FavoriteEntry>, UiError> {
    Ok(db.favorite_list()?)
}

/// 「あとで見る」の登録上限の扱いは置かない（履歴の剪定対象でもない）。
/// 横断検索の上限（統合後の件数）。
const LIBRARY_SEARCH_LIMIT: u32 = 200;

/// `watch_later_add`（FR-59、仕様決定 CJ）。
/// 動画メタは `videos` 台帳へ登録され、同一動画の再登録は追加日時を
/// 変えない（冪等）。
#[tauri::command]
pub fn watch_later_add(video: VideoRef, db: State<'_, Db>) -> Result<(), UiError> {
    let id = validate_video_ref(&video)?;
    let mut v = video;
    v.video_id = id;
    Ok(db.watch_later_add(&v)?)
}

/// `watch_later_remove`（FR-59）。未登録でも正常終了（冪等）。
#[tauri::command]
pub fn watch_later_remove(video_id: String, db: State<'_, Db>) -> Result<(), UiError> {
    let id = parse_video_id(&video_id)?;
    Ok(db.watch_later_remove(&id)?)
}

/// `watch_later_list`（FR-59）。追加が新しい順。
#[tauri::command]
pub fn watch_later_list(db: State<'_, Db>) -> Result<Vec<WatchLaterEntry>, UiError> {
    Ok(db.watch_later_list()?)
}

/// `library_search`（FR-60、仕様決定 CK）。
/// 履歴・お気に入り・プレイリスト項目・あとで見るをタイトルとチャンネル名の
/// 部分一致 LIKE で横断検索し、同一動画は `sources` に区分を持つ 1 件へ
/// まとめる。`query` の空・空白のみは 0 件を返す（暫定解釈）。
#[tauri::command]
pub fn library_search(
    query: String,
    limit: Option<u32>,
    db: State<'_, Db>,
) -> Result<Vec<LibrarySearchItem>, UiError> {
    let limit = limit.unwrap_or(LIBRARY_SEARCH_LIMIT).min(1000);
    Ok(db.library_search(&query, limit)?)
}

/// `playlist_list`（FR-7）。件数集計付き。
#[tauri::command]
pub fn playlist_list(db: State<'_, Db>) -> Result<Vec<Playlist>, UiError> {
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
/// 削除した行のスナップショット（元の `position` 付き）を返す
/// （FR-54、仕様決定 CE のトースト「元に戻す」→ `playlist_item_restore`
/// の入力）。対象行が無ければ null。
#[tauri::command]
pub fn playlist_remove(
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
pub fn playlist_item_restore(
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

/// `playlist_reorder_lists`（FR-35、仕様決定 BF）。
/// プレイリスト一覧の表示順の一括書き換え。渡した `playlist_ids` は現在の
/// プレイリスト全件と同一集合である必要があり、一致しない場合は変更せず
/// エラーとする（`playlist_reorder` と同じ並行編集の誤適用防止）。
#[tauri::command]
pub fn playlist_reorder_lists(playlist_ids: Vec<i64>, db: State<'_, Db>) -> Result<(), UiError> {
    if playlist_ids.iter().any(|&id| id <= 0) {
        return Err(UiError::invalid_input("playlist_id が不正です"));
    }
    // 同一集合の検証は db.playlist_reorder_lists が書き込みトランザクション内で行う
    db.playlist_reorder_lists(&playlist_ids)
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
