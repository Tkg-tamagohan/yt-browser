//! チャンネル購読・フィード系コマンド（設計書 §3.1）。

use std::sync::Arc;
use std::time::Duration;

use tauri::{AppHandle, Emitter, State};

use crate::db::Db;
use crate::error::UiError;
use crate::feed::{self, FeedPoller};
use crate::innertube::InnerTube;
use crate::model::{
    is_channel_id, parse_channel_ref, BackfillOutcome, Category, Channel, ChannelRef, FeedFilter,
    FeedItem, FeedNewItems, PlayingChannel,
};
use crate::yt::{self, YtDlpResolver};

use super::parse_video_id;

/// `yt::video_channel_ref` フォールバックのタイムアウト上限（仕様決定 DY）。
const VIDEO_CHANNEL_FALLBACK_TIMEOUT: Duration = Duration::from_secs(15);

/// 外部メタ（InnerTube / yt-dlp）による動画チャンネル解決の結果（仕様決定 DY・U）。
pub(crate) struct ResolvedVideoChannel {
    /// `subscribe_channel` へ渡せる入力（`UC...` または `@handle` 等の `parse_channel_ref` 受理形）。
    pub(crate) input: Option<String>,
    /// 正規 `UC...` 形式のチャンネル ID（`watch_history.channel_id` 保存用）。
    pub(crate) channel_id: Option<String>,
    pub(crate) channel_title: Option<String>,
    pub(crate) title: Option<String>,
}

/// 動画のチャンネル情報を `InnerTube::video_meta` → 失敗時 15 秒タイムアウト付き
/// `yt::video_channel_ref` の順で解決する共通ヘルパ（仕様決定 DY・U）。
/// 再生開始時のバックグラウンド履歴補完フックと `playing_channel` 第 3 段の双方が共用する。
pub(crate) async fn resolve_video_channel(
    video_id: &str,
    db: &Db,
    innertube: &InnerTube,
    resolver: &YtDlpResolver,
) -> ResolvedVideoChannel {
    resolve_video_channel_with_timeout(
        video_id,
        db,
        innertube,
        resolver,
        VIDEO_CHANNEL_FALLBACK_TIMEOUT,
    )
    .await
}

async fn resolve_video_channel_with_timeout(
    video_id: &str,
    db: &Db,
    innertube: &InnerTube,
    resolver: &YtDlpResolver,
    fallback_timeout: Duration,
) -> ResolvedVideoChannel {
    let mut channel_title: Option<String> = None;
    let mut title: Option<String> = None;
    match innertube.video_meta(video_id).await {
        Ok(Some(v)) => {
            channel_title = v
                .channel_title
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string);
            let t = v.title.trim();
            if !t.is_empty() {
                title = Some(t.to_string());
            }
            if let Some(cid) = v
                .channel_id
                .as_deref()
                .map(str::trim)
                .filter(|s| is_channel_id(s))
                .map(str::to_string)
            {
                return ResolvedVideoChannel {
                    input: Some(cid.clone()),
                    channel_id: Some(cid),
                    channel_title,
                    title,
                };
            }
        }
        Ok(None) => {}
        Err(e) => {
            tracing::debug!(video_id, error = %e, "InnerTube video_meta によるチャンネル解決に失敗");
        }
    }
    let mut input: Option<String> = None;
    let mut channel_id: Option<String> = None;
    if let Some(path) = resolver.resolve(db).await {
        let po_token = yt::po_token_setting(db);
        match tokio::time::timeout(
            fallback_timeout,
            yt::video_channel_ref(&path, video_id, po_token.as_deref()),
        )
        .await
        {
            Ok(Ok(info)) => {
                // UC ID が取れる候補があれば channel_id / input 双方へ採用し、
                // 無い場合も @handle 等の受理形を playing_channel の input として拾う。
                // InnerTube で取れなかった channel_title / title は同一 yt-dlp JSON で補完する。
                channel_id = info
                    .candidates
                    .iter()
                    .find_map(|c| match parse_channel_ref(c) {
                        Some(ChannelRef::Id(cid)) => Some(cid),
                        _ => None,
                    });
                input = channel_id.clone().or_else(|| {
                    info.candidates
                        .into_iter()
                        .find(|c| parse_channel_ref(c).is_some())
                });
                if channel_title.is_none() {
                    channel_title = info.channel_title;
                }
                if title.is_none() {
                    title = info.title;
                }
            }
            Ok(Err(e)) => {
                tracing::debug!(video_id, error = %e, "yt-dlp video_channel_ref によるチャンネル解決に失敗");
            }
            Err(_) => {
                tracing::warn!(
                    video_id,
                    timeout_ms = fallback_timeout.as_millis() as u64,
                    "yt-dlp video_channel_ref がタイムアウトしました"
                );
            }
        }
    }
    ResolvedVideoChannel {
        input,
        channel_id,
        channel_title,
        title,
    }
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
            yt::channel_id(&path, url.as_str(), yt::po_token_setting(&db).as_deref())
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
            // 初回投入もフィード更新の一形態として自動既読を適用する
            // （FR-64、仕様決定 CO。暫定解釈: 「フィード更新後」に含める）
            poller.apply_auto_read();
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

/// 再生中動画のチャンネル解決（FR-12、仕様決定 U・DY）。
/// `videos.channel_id` → `watch_history.channel_id` → 外部メタ
/// （`InnerTube::video_meta` → 失敗時 15 秒タイムアウト付き `yt::video_channel_ref`）
/// の順に `subscribe_channel` へ渡せる入力を決めて返す。各段の値が空文字・NULL の
/// 場合は「未知」として次段へ進む。解決不能は `input: null`。
#[tauri::command]
pub async fn playing_channel(
    video_id: String,
    db: State<'_, Db>,
    innertube: State<'_, Arc<InnerTube>>,
    resolver: State<'_, YtDlpResolver>,
) -> Result<PlayingChannel, UiError> {
    let id = parse_video_id(&video_id)?;
    // 1) videos 台帳（フィード・お気に入り・プレイリスト経由で既知情報がある）
    let (mut input, raw_title) = db.video_channel(&id)?;
    let mut title = raw_title
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let mut history_exists = false;
    // 2) watch_history（台帳に無い動画も履歴の再視聴で拾う）
    if input.is_none() {
        if let Some(h) = db.history_get(&id)? {
            history_exists = true;
            input = h
                .channel_id
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
            if title.is_none() {
                title = h
                    .channel_title
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty());
            }
        }
    }
    // 3) 外部メタ（InnerTube → yt-dlp、DB に知識が無いときだけ。失敗は解決不能扱い）
    if input.is_none() {
        let resolved = resolve_video_channel(&id, &db, &innertube, &resolver).await;
        input = resolved.input;
        if title.is_none() {
            title = resolved.channel_title.clone();
        }
        // `playing_channel` が `history_upsert` より先に到達した競合でも、
        // 外部解決完了時点で履歴行が存在すれば書き戻す（未視聴動画への幽霊行作成は防ぐ）
        if (resolved.channel_id.is_some() || resolved.channel_title.is_some())
            && (history_exists || db.history_get(&id)?.is_some())
        {
            let _ = db.history_set_channel(
                &id,
                resolved.channel_id.as_deref(),
                resolved.channel_title.as_deref(),
                resolved.title.as_deref(),
            );
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
pub async fn unsubscribe_channel(channel_id: String, db: State<'_, Db>) -> Result<(), UiError> {
    db.channel_delete(&channel_id)?;
    Ok(())
}

/// 購読チャンネル一覧。
#[tauri::command]
pub async fn list_channels(db: State<'_, Db>) -> Result<Vec<Channel>, UiError> {
    Ok(db.channel_list()?)
}

/// チャンネルのカテゴリ割り当て（null で未分類に戻す）。
#[tauri::command]
pub async fn set_channel_category(
    channel_id: String,
    category_id: Option<i64>,
    db: State<'_, Db>,
) -> Result<(), UiError> {
    db.channel_set_category(&channel_id, category_id)?;
    Ok(())
}

#[tauri::command]
pub async fn list_categories(db: State<'_, Db>) -> Result<Vec<Category>, UiError> {
    Ok(db.category_list()?)
}

#[tauri::command]
pub async fn create_category(name: String, db: State<'_, Db>) -> Result<Category, UiError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(UiError::invalid_input("カテゴリ名が空です"));
    }
    Ok(db.category_create(name)?)
}

/// `category_reorder`（FR-63、仕様決定 CN）。カテゴリの並べ替え。
/// `category_ids` が現在のカテゴリ全件と同一集合でなければ変更しない
/// （`playlist_reorder_lists` と同じ検証規則）。
#[tauri::command]
pub async fn category_reorder(category_ids: Vec<i64>, db: State<'_, Db>) -> Result<(), UiError> {
    db.category_reorder(&category_ids)?;
    Ok(())
}

/// `list_feed`（設計書 §3.1）。NG フィルタの動画系 target もここで適用する（FR-9）。
#[tauri::command]
pub async fn list_feed(
    filter: FeedFilter,
    db: State<'_, Db>,
    ng: State<'_, Arc<crate::filter::NgMatcher>>,
) -> Result<Vec<FeedItem>, UiError> {
    let db = db.inner().clone();
    let matcher = ng.get();
    // 最大 FEED_LIST_LIMIT 件の走査と NG 評価をメインスレッド外へ逃がす
    tokio::task::spawn_blocking(move || -> Result<Vec<FeedItem>, UiError> {
        // feed.show_shorts が off のとき「すべて」「video」選択は short を除く
        // （FR-23、仕様決定 AP）。kind で short を明示した選択は設定に関わらず表示する
        let show_shorts = crate::feed::feed_show_shorts_enabled(
            db.setting_get(crate::feed::SETTING_FEED_SHOW_SHORTS)?,
        ) || filter.kind.as_deref() == Some("short");
        // 述語適合が FEED_LIST_LIMIT 件に達するまで走査するため、
        // 先頭が NG で抜けても後続の適合行を拾える。
        // 説明文は遅延解決済みの行だけが持つ（FR-85、仕様決定 DJ）。
        // 未解決（NULL）の行は video_desc の評価対象にしない暫定解釈。
        Ok(
            db.feed_list_filtered(&filter, show_shorts, crate::db::FEED_LIST_LIMIT, |i| {
                !matcher.is_video_ng(
                    &i.title,
                    i.channel_title.as_deref(),
                    Some(&i.channel_id),
                    i.description.as_deref(),
                )
            })?,
        )
    })
    .await
    .map_err(|e| UiError::internal(format!("ワーカーが中断しました: {e}")))?
}

/// `mark_read`（設計書 §3.1）。`all: true` で一括既読、それ以外は `video_ids` を個別既読。
#[tauri::command]
pub async fn mark_read(
    video_ids: Option<Vec<String>>,
    all: Option<bool>,
    db: State<'_, Db>,
) -> Result<u64, UiError> {
    let db = db.inner().clone();
    // `all: true` の一括既読は複数行の書き込みになるためブロッキング側へ逃がす
    tokio::task::spawn_blocking(move || -> Result<u64, UiError> {
        if all == Some(true) {
            return Ok(db.videos_mark_all_read()?);
        }
        let ids = video_ids.unwrap_or_default();
        db.videos_mark_read(&ids)?;
        Ok(ids.len() as u64)
    })
    .await
    .map_err(|e| UiError::internal(format!("ワーカーが中断しました: {e}")))?
}

/// 手動の即時更新。全チャンネル（または指定チャンネル）の次回予定を現在に戻し、
/// メタ遅延解決・ライブ状態再確認ワーカーも手動トリガーで起床させる（FR-96）。
#[tauri::command]
pub fn feed_refresh(
    channel_id: Option<String>,
    poller: State<'_, Arc<FeedPoller>>,
    meta_resolver: State<'_, Arc<crate::metaresolve::MetaResolver>>,
) -> Result<(), UiError> {
    poller.force_refresh(channel_id.as_deref());
    meta_resolver.wake_manual();
    Ok(())
}

/// バックフィルの 1 回あたりのタブ別取得件数（仕様決定 AN の暫定値）。
const BACKFILL_BATCH: u32 = 100;

/// バックフィルの取り込み対象タブ `(タブ名, videos.kind)` の一覧
/// （FR-21、仕様決定 AN。`/shorts` は FR-55・仕様決定 CF で追加）。
/// 並びは `channel_backfill_positions` の戻り値 `(videos, streams, shorts)`
/// と同じ順で、位置の更新はインデックスで対応付ける。
const BACKFILL_TABS: &[(&str, &str)] = &[
    ("videos", "video"),
    ("streams", "live"),
    ("shorts", "short"),
];

/// `feed_backfill`（FR-21・FR-55、仕様決定 AN・CF）。購読チャンネルの
/// `/videos`・`/streams`・`/shorts` タブを yt-dlp flat-playlist で遡り、
/// 各タブ 100 件ずつ既読・投入済みで積む。
/// 取得済み位置は `channels` のタブ別カラムに保持し、再呼び出しで続きを遡る。
/// 一部のタブの取得失敗は `errors` に乗せて他タブの結果を維持する。
#[tauri::command]
pub async fn feed_backfill(
    channel_id: String,
    db: State<'_, Db>,
    resolver: State<'_, YtDlpResolver>,
    poller: State<'_, Arc<FeedPoller>>,
) -> Result<BackfillOutcome, UiError> {
    let channel_id = channel_id.trim().to_string();
    let ch = db
        .channel_get(&channel_id)?
        .ok_or_else(|| UiError::invalid_input("購読していないチャンネルです"))?;
    let path = resolver.resolve(&db).await.ok_or(yt::YtError::NotFound)?;
    let (videos_pos, streams_pos, shorts_pos) = db.channel_backfill_positions(&channel_id)?;
    let mut positions = [videos_pos, streams_pos, shorts_pos];
    // 取得中と失敗は feed://status で通知する（仕様決定 AN。
    // トーストはフロント側の feed://status 購読と BackfillOutcome 応答が担う）
    poller.emit_status(
        Some(&channel_id),
        "info",
        format!("{} の過去動画を取得しています", ch.title),
    );
    let mut inserted = 0usize;
    let mut errors = Vec::new();
    for (i, (tab, kind)) in BACKFILL_TABS.iter().enumerate() {
        let pos = positions[i];
        // 位置は DB の i64。実際に u32 を超える蓄積はないが丸めで安全側に倒す
        let start = (pos + 1).min(u32::MAX as i64) as u32;
        let end = (pos + BACKFILL_BATCH as i64).min(u32::MAX as i64) as u32;
        match yt::channel_tab_entries(
            &path,
            &channel_id,
            tab,
            start,
            end,
            yt::po_token_setting(&db).as_deref(),
        )
        .await
        {
            Ok(entries) => {
                let items: Vec<crate::db::NewVideo> = entries
                    .iter()
                    .map(|(id, title, thumb)| crate::db::NewVideo {
                        video_id: id,
                        channel_id: &channel_id,
                        channel_title: &ch.title,
                        title,
                        thumbnail_url: thumb.as_deref(),
                        published_at: None,
                        kind,
                    })
                    .collect();
                let fetched = items.len() as i64;
                if let Some(n) = db.feed_backfill_ingest(&channel_id, &items)? {
                    inserted += n;
                }
                positions[i] = pos + fetched;
            }
            Err(e) => {
                let msg = format!("{tab} タブ: {e}");
                poller.emit_status(Some(&channel_id), "warn", msg.clone());
                errors.push(msg);
            }
        }
    }
    db.channel_set_backfill_positions(&channel_id, positions[0], positions[1], positions[2])?;
    Ok(BackfillOutcome {
        inserted,
        videos_pos: positions[0],
        streams_pos: positions[1],
        shorts_pos: positions[2],
        errors,
    })
}

#[cfg(test)]
mod tests {
    use super::{resolve_video_channel_with_timeout, BACKFILL_TABS};
    use crate::db::Db;
    use crate::innertube::InnerTube;
    use crate::yt::{YtDlpResolver, SETTING_YTDLP_PATH};
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    /// バックフィルの取り込み対象タブ（FR-21・FR-55、仕様決定 AN・CF）。
    /// `/shorts` タブを含み、kind の対応は videos→video・streams→live・
    /// shorts→short とする。並びは `channel_backfill_positions` の
    /// 戻り値 (videos, streams, shorts) と同じ順でなければならない。
    #[test]
    fn backfill_tabs_include_shorts() {
        let tabs: Vec<(&str, &str)> = BACKFILL_TABS.to_vec();
        assert_eq!(
            tabs,
            vec![
                ("videos", "video"),
                ("streams", "live"),
                ("shorts", "short")
            ]
        );
        // positions 配列の添字対応: videos=0, streams=1, shorts=2
        assert_eq!(BACKFILL_TABS[2].0, "shorts");
    }

    struct TempScript(PathBuf);
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
        let path = std::env::temp_dir().join(format!("ytb_fake_ytdlp_{tag}_{pid}_{nonce}.cmd"));
        #[cfg(not(windows))]
        let path = std::env::temp_dir().join(format!("ytb_fake_ytdlp_{tag}_{pid}_{nonce}.sh"));

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

    async fn spawn_mock_innertube(
        player_status: &'static str,
        player_body: &'static str,
    ) -> (InnerTube, tokio::task::JoinHandle<()>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let srv = tokio::spawn(async move {
            let ytcfg_html = r#"<script>ytcfg.set({"INNERTUBE_API_KEY":"k1","INNERTUBE_CONTEXT_CLIENT_VERSION":"v1","VISITOR_DATA":"vd1"});</script>"#;
            while let Ok((mut stream, _)) = listener.accept().await {
                let mut buf = [0u8; 2048];
                let n = stream.read(&mut buf).await.unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]);
                let resp = if req.starts_with("GET /watch") {
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        ytcfg_html.len(),
                        ytcfg_html
                    )
                } else {
                    format!(
                        "HTTP/1.1 {player_status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        player_body.len(),
                        player_body
                    )
                };
                let _ = stream.write_all(resp.as_bytes()).await;
            }
        });
        let tube = InnerTube::with_test_urls(
            format!("http://{addr}/watch"),
            format!("http://{addr}/api"),
            Duration::from_secs(5),
        );
        (tube, srv)
    }

    #[tokio::test]
    async fn resolve_video_channel_prefers_innertube_and_skips_ytdlp() {
        let db = Db::connect_in_memory().unwrap();
        // 存在しない yt-dlp パスを入れても InnerTube 成功時は到達しない
        db.setting_set(SETTING_YTDLP_PATH, "/nonexistent/yt-dlp-should-not-run")
            .unwrap();
        let (tube, srv) = spawn_mock_innertube(
            "200 OK",
            r#"{"videoDetails":{"videoId":"vid00000001","title":"動画A","channelId":"UCtube000000000000000001","author":"TubeCH"}}"#,
        )
        .await;
        let resolver = YtDlpResolver::new(None);
        let res = resolve_video_channel_with_timeout(
            "vid00000001",
            &db,
            &tube,
            &resolver,
            Duration::from_secs(5),
        )
        .await;
        assert_eq!(res.channel_id.as_deref(), Some("UCtube000000000000000001"));
        assert_eq!(res.input.as_deref(), Some("UCtube000000000000000001"));
        assert_eq!(res.channel_title.as_deref(), Some("TubeCH"));
        assert_eq!(res.title.as_deref(), Some("動画A"));
        srv.abort();
    }

    #[tokio::test]
    async fn resolve_video_channel_falls_back_to_ytdlp_for_uc_and_handle() {
        // InnerTube が 500 を返したとき yt-dlp フォールバックで UC ID とチャンネル名・題名を同時取得する
        let script_uc = write_fake_ytdlp(
            "uc",
            "@echo off\r\necho {\"channel_id\":\"UCfall000000000000000001\",\"channel\":\"MountainWalk\",\"title\":\"FallbackVideo\"}\r\n",
            "#!/bin/sh\nprintf '%s\\n' '{\"channel_id\":\"UCfall000000000000000001\",\"channel\":\"MountainWalk\",\"title\":\"FallbackVideo\"}'\n",
        );
        let db = Db::connect_in_memory().unwrap();
        db.setting_set(SETTING_YTDLP_PATH, script_uc.0.to_str().unwrap())
            .unwrap();
        let (tube, srv) = spawn_mock_innertube("500 Internal Server Error", "{}").await;
        let resolver = YtDlpResolver::new(None);
        let res = resolve_video_channel_with_timeout(
            "vid00000002",
            &db,
            &tube,
            &resolver,
            Duration::from_secs(5),
        )
        .await;
        assert_eq!(res.channel_id.as_deref(), Some("UCfall000000000000000001"));
        assert_eq!(res.input.as_deref(), Some("UCfall000000000000000001"));
        assert_eq!(res.channel_title.as_deref(), Some("MountainWalk"));
        assert_eq!(res.title.as_deref(), Some("FallbackVideo"));
        srv.abort();

        // InnerTube が channelId 欠落（author/title のみ）で yt-dlp が @handle と別名返したとき、
        // input は @handle を採りつつ channel_id は非 UC で汚さず None に保ち、
        // 既に InnerTube で取れた channel_title / title は維持する
        let script_handle = write_fake_ytdlp(
            "handle",
            "@echo off\r\necho {\"uploader_id\":\"@sample_creator\",\"channel\":\"IgnoredFallback\",\"title\":\"IgnoredTitle\"}\r\n",
            "#!/bin/sh\nprintf '%s\\n' '{\"uploader_id\":\"@sample_creator\",\"channel\":\"IgnoredFallback\",\"title\":\"IgnoredTitle\"}'\n",
        );
        db.setting_set(SETTING_YTDLP_PATH, script_handle.0.to_str().unwrap())
            .unwrap();
        let (tube2, srv2) = spawn_mock_innertube(
            "200 OK",
            r#"{"videoDetails":{"videoId":"vid00000003","title":"動画C","author":"CreatorC"}}"#,
        )
        .await;
        let res2 = resolve_video_channel_with_timeout(
            "vid00000003",
            &db,
            &tube2,
            &resolver,
            Duration::from_secs(5),
        )
        .await;
        assert_eq!(res2.channel_id, None);
        assert_eq!(res2.input.as_deref(), Some("@sample_creator"));
        assert_eq!(res2.channel_title.as_deref(), Some("CreatorC"));
        assert_eq!(res2.title.as_deref(), Some("動画C"));
        srv2.abort();

        // InnerTube が 500 かつ yt-dlp が参照候補なし・チャンネル名のみ返したとき、
        // channel_id / input は None のまま履歴検索用 channel_title / title を返す
        let script_name_only = write_fake_ytdlp(
            "nameonly",
            "@echo off\r\necho {\"channel\":\"NameOnlyCH\",\"title\":\"NameOnlyVideo\"}\r\n",
            "#!/bin/sh\nprintf '%s\\n' '{\"channel\":\"NameOnlyCH\",\"title\":\"NameOnlyVideo\"}'\n",
        );
        db.setting_set(SETTING_YTDLP_PATH, script_name_only.0.to_str().unwrap())
            .unwrap();
        let (tube3, srv3) = spawn_mock_innertube("500 Internal Server Error", "{}").await;
        let res3 = resolve_video_channel_with_timeout(
            "vid00000005",
            &db,
            &tube3,
            &resolver,
            Duration::from_secs(5),
        )
        .await;
        assert_eq!(res3.channel_id, None);
        assert_eq!(res3.input, None);
        assert_eq!(res3.channel_title.as_deref(), Some("NameOnlyCH"));
        assert_eq!(res3.title.as_deref(), Some("NameOnlyVideo"));
        srv3.abort();
    }

    #[tokio::test]
    async fn resolve_video_channel_enforces_fallback_timeout() {
        let script_hang = write_fake_ytdlp(
            "hang",
            "@echo off\r\nping -n 4 127.0.0.1 >nul\r\n",
            "#!/bin/sh\nexec sleep 3\n",
        );
        let db = Db::connect_in_memory().unwrap();
        db.setting_set(SETTING_YTDLP_PATH, script_hang.0.to_str().unwrap())
            .unwrap();
        let (tube, srv) = spawn_mock_innertube("500 Internal Server Error", "{}").await;
        let resolver = YtDlpResolver::new(None);
        let started = Instant::now();
        let res = resolve_video_channel_with_timeout(
            "vid00000004",
            &db,
            &tube,
            &resolver,
            Duration::from_millis(200),
        )
        .await;
        let elapsed = started.elapsed();
        assert!(res.channel_id.is_none());
        assert!(res.input.is_none());
        assert!(
            elapsed < Duration::from_secs(2),
            "フォールバックタイムアウトが効いていない: {elapsed:?}"
        );
        srv.abort();
    }
}
