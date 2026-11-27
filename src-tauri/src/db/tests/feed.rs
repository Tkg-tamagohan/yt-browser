use super::vref;
use crate::db::*;
use crate::model::FeedFilter;

/// 購読解除後も残る videos 行はフィード一覧に出さない（JOIN で除外）。
/// 未読・すべて両モードで確認する。
#[test]
fn feed_list_hides_unsubscribed_channel_videos() {
    let db = Db::connect_in_memory().unwrap();
    let entries = [NewVideo {
        video_id: "v1",
        channel_id: "UCchan000000000000001",
        channel_title: "C",
        title: "V1",
        thumbnail_url: None,
        published_at: Some("2026-10-01 00:00:00"),
        kind: "video",
    }];
    db.feed_subscribe(&SubscribeArgs {
        channel_id: "UCchan000000000000001",
        title: "C",
        thumbnail_url: None,
        category_id: None,
        entries: &entries,
        etag: None,
        last_modified: None,
    })
    .unwrap();
    let all = FeedFilter {
        unread_only: false,
        category_id: None,
        days: None,
        kind: None,
        cursor: None,
        channel_id: None,
    };
    assert_eq!(
        db.feed_list_filtered(&all, true, FEED_LIST_LIMIT, |_| true)
            .unwrap()
            .len(),
        1
    );
    db.channel_delete("UCchan000000000000001").unwrap();
    assert!(db
        .feed_list_filtered(&all, true, FEED_LIST_LIMIT, |_| true)
        .unwrap()
        .is_empty());
    let unread = FeedFilter {
        unread_only: true,
        category_id: None,
        days: None,
        kind: None,
        cursor: None,
        channel_id: None,
    };
    assert!(db
        .feed_list_filtered(&unread, true, FEED_LIST_LIMIT, |_| true)
        .unwrap()
        .is_empty());
}

/// DB-LD-06: ライブラリ登録で先に作ったプレースホルダ行に、
/// 後着の RSS 投入で投稿日・種別・未読を埋める。既存のフィード行は
/// 既読状態を含めて書き換えない。
#[test]
fn feed_ingest_backfills_library_placeholder() {
    let db = Db::connect_in_memory().unwrap();
    let ch = "UCchan000000000000001";
    // 購読チャンネルと、お気に入り登録で先にできた行（published_at NULL）
    db.feed_subscribe(&SubscribeArgs {
        channel_id: ch,
        title: "テストCH",
        thumbnail_url: None,
        category_id: None,
        entries: &[],
        etag: None,
        last_modified: None,
    })
    .unwrap();
    let mut v = vref("dQw4w9WgXcQ", "動画A");
    v.channel_id = Some(ch.to_string());
    db.favorite_add(&v).unwrap();

    // RSS で同じ動画が届く: published_at / kind が埋まり未読になる
    let entry = NewVideo {
        video_id: "dQw4w9WgXcQ",
        channel_id: ch,
        channel_title: "テストCH",
        title: "動画A",
        thumbnail_url: None,
        published_at: Some("2026-10-07T00:00:00+00:00"),
        kind: "video",
    };
    let out = db
        .feed_ingest(ch, std::slice::from_ref(&entry), None, None)
        .unwrap()
        .unwrap();
    assert_eq!(out.inserted, 1);
    let feed = db
        .feed_list_filtered(&FeedFilter::default(), true, FEED_LIST_LIMIT, |_| true)
        .unwrap();
    let item = feed.iter().find(|i| i.video_id == "dQw4w9WgXcQ").unwrap();
    assert_eq!(
        item.published_at.as_deref(),
        Some("2026-10-07T00:00:00+00:00")
    );
    assert!(!item.is_read);

    // 既読にしても再投入で既読状態は保つ（プレースホルダではないため）
    db.videos_mark_read(&["dQw4w9WgXcQ".to_string()]).unwrap();
    let out = db.feed_ingest(ch, &[entry], None, None).unwrap().unwrap();
    assert_eq!(out.inserted, 0);
    let feed = db
        .feed_list_filtered(&FeedFilter::default(), true, FEED_LIST_LIMIT, |_| true)
        .unwrap();
    let item = feed.iter().find(|i| i.video_id == "dQw4w9WgXcQ").unwrap();
    assert!(item.is_read);

    // 投稿日なしのエントリも最初の投入で ingested=1 となり、
    // 再投入で未読に戻ったり inserted が増えたりしない
    let no_date = NewVideo {
        video_id: "nodate12345",
        channel_id: ch,
        channel_title: "テストCH",
        title: "投稿日なし",
        thumbnail_url: None,
        published_at: None,
        kind: "video",
    };
    let out = db
        .feed_ingest(ch, std::slice::from_ref(&no_date), None, None)
        .unwrap()
        .unwrap();
    assert_eq!(out.inserted, 1);
    db.videos_mark_read(&["nodate12345".to_string()]).unwrap();
    let out = db.feed_ingest(ch, &[no_date], None, None).unwrap().unwrap();
    assert_eq!(out.inserted, 0);
    let feed = db
        .feed_list_filtered(&FeedFilter::default(), true, FEED_LIST_LIMIT, |_| true)
        .unwrap();
    let item = feed.iter().find(|i| i.video_id == "nodate12345").unwrap();
    assert!(item.is_read);
    assert_eq!(item.published_at, None);

    // フィードに到達していないプレースホルダ（ingested=0）はフィードに出ない
    let mut pending = vref("pend1234567", "未到達");
    pending.channel_id = Some(ch.to_string());
    db.favorite_add(&pending).unwrap();
    let feed = db
        .feed_list_filtered(&FeedFilter::default(), true, FEED_LIST_LIMIT, |_| true)
        .unwrap();
    assert!(!feed.iter().any(|i| i.video_id == "pend1234567"));
}

/// 新規投入の video_id が IngestOutcome.new_video_ids に返り、
/// 再投入では空になる（shorts 判定の対象集合、仕様決定 V）。
#[test]
fn feed_ingest_reports_new_video_ids() {
    let db = Db::connect_in_memory().unwrap();
    let ch = "UCchan000000000000001";
    let entries = [NewVideo {
        video_id: "vid00000001",
        channel_id: ch,
        channel_title: "CH",
        title: "V1",
        thumbnail_url: None,
        published_at: Some("2026-10-08T00:00:00+00:00"),
        kind: "video",
    }];
    let out = db
        .feed_subscribe(&SubscribeArgs {
            channel_id: ch,
            title: "CH",
            thumbnail_url: None,
            category_id: None,
            entries: &entries,
            etag: None,
            last_modified: None,
        })
        .unwrap();
    assert_eq!(out.new_video_ids, vec!["vid00000001".to_string()]);

    let out = db.feed_ingest(ch, &entries, None, None).unwrap().unwrap();
    assert_eq!(out.inserted, 0);
    assert!(out.new_video_ids.is_empty());
}

/// 種別フィルタ（FR-13）。kind='short' の行だけが絞り込まれる。
#[test]
fn feed_list_filtered_by_kind() {
    let db = Db::connect_in_memory().unwrap();
    let ch = "UCchan000000000000001";
    fn mk<'a>(id: &'a str, kind: &'a str, ch: &'a str) -> NewVideo<'a> {
        NewVideo {
            video_id: id,
            channel_id: ch,
            channel_title: "CH",
            title: id,
            thumbnail_url: None,
            published_at: Some("2026-10-08T00:00:00+00:00"),
            kind,
        }
    }
    db.feed_subscribe(&SubscribeArgs {
        channel_id: ch,
        title: "CH",
        thumbnail_url: None,
        category_id: None,
        entries: &[
            mk("video0000001", "video", ch),
            mk("short0000001", "short", ch),
        ],
        etag: None,
        last_modified: None,
    })
    .unwrap();

    let by_kind = |kind: Option<String>| {
        let f = FeedFilter {
            kind,
            ..Default::default()
        };
        db.feed_list_filtered(&f, true, FEED_LIST_LIMIT, |_| true)
            .unwrap()
            .iter()
            .map(|i| i.video_id.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(by_kind(None).len(), 2);
    assert_eq!(by_kind(Some("short".to_string())), vec!["short0000001"]);
    assert_eq!(by_kind(Some("video".to_string())), vec!["video0000001"]);
    assert!(by_kind(Some("live".to_string())).is_empty());
}

/// feed.show_shorts=off 相当（show_shorts=false）の呼び出しでは kind='short' の
/// 行が除かれ、true では含まれる（FR-23、仕様決定 AP。明示の short 選択で
/// true へ回す判定はコマンド側）。
#[test]
fn feed_list_filtered_hides_shorts_when_off() {
    let db = Db::connect_in_memory().unwrap();
    let ch = "UCchan000000000000001";
    fn mk<'a>(id: &'a str, kind: &'a str, ch: &'a str) -> NewVideo<'a> {
        NewVideo {
            video_id: id,
            channel_id: ch,
            channel_title: "CH",
            title: id,
            thumbnail_url: None,
            published_at: Some("2026-10-08T00:00:00+00:00"),
            kind,
        }
    }
    db.feed_subscribe(&SubscribeArgs {
        channel_id: ch,
        title: "CH",
        thumbnail_url: None,
        category_id: None,
        entries: &[
            mk("video0000001", "video", ch),
            mk("short0000001", "short", ch),
        ],
        etag: None,
        last_modified: None,
    })
    .unwrap();

    let ids = |show_shorts: bool| {
        db.feed_list_filtered(&FeedFilter::default(), show_shorts, FEED_LIST_LIMIT, |_| {
            true
        })
        .unwrap()
        .iter()
        .map(|i| i.video_id.clone())
        .collect::<Vec<_>>()
    };
    assert_eq!(ids(true).len(), 2);
    assert_eq!(ids(false), vec!["video0000001"]);
}

/// カーソルページング（FR-25、仕様決定 AR）。
/// 並び順は published_at 降順・同時刻は video_id 昇順・NULL は末尾。
/// カーソルは前ページ末尾行を指し、次ページはその行の次から重複なく返る。
#[test]
fn feed_list_filtered_cursor_paging() {
    use crate::model::FeedCursor;
    let db = Db::connect_in_memory().unwrap();
    let ch = "UCchan000000000000001";
    fn mk<'a>(id: &'a str, published_at: Option<&'a str>, ch: &'a str) -> NewVideo<'a> {
        NewVideo {
            video_id: id,
            channel_id: ch,
            channel_title: "CH",
            title: id,
            thumbnail_url: None,
            published_at,
            kind: "video",
        }
    }
    db.feed_subscribe(&SubscribeArgs {
        channel_id: ch,
        title: "CH",
        thumbnail_url: None,
        category_id: None,
        entries: &[
            mk("aaaa0000001", Some("2026-10-08T00:00:00+00:00"), ch),
            mk("bbbb0000001", Some("2026-10-08T00:00:00+00:00"), ch),
            mk("cccc0000001", Some("2026-10-01T00:00:00+00:00"), ch),
            mk("dddd0000001", None, ch),
        ],
        etag: None,
        last_modified: None,
    })
    .unwrap();

    let page = |cursor: Option<FeedCursor>, limit: usize| {
        let f = FeedFilter {
            cursor,
            ..Default::default()
        };
        db.feed_list_filtered(&f, true, limit, |_| true)
            .unwrap()
            .iter()
            .map(|i| (i.video_id.clone(), i.published_at.clone()))
            .collect::<Vec<_>>()
    };
    let cur = |published_at: Option<&str>, video_id: &str| FeedCursor {
        published_at: published_at.map(str::to_string),
        video_id: video_id.to_string(),
        is_live: false,
    };
    let ids_of =
        |rows: &[(String, Option<String>)]| rows.iter().map(|r| r.0.clone()).collect::<Vec<_>>();

    // 1 ページ目：同時刻は video_id 昇順
    let p1 = page(None, 2);
    assert_eq!(ids_of(&p1), vec!["aaaa0000001", "bbbb0000001"]);

    // 2 ページ目：カーソル行自体は含まれず、NULL 行は末尾
    let p2 = page(
        Some(cur(Some("2026-10-08T00:00:00+00:00"), "bbbb0000001")),
        2,
    );
    assert_eq!(ids_of(&p2), vec!["cccc0000001", "dddd0000001"]);
    assert_eq!(p2[1].1, None);

    // 3 ページ目：NULL 群の中では video_id 昇順で遡る（末尾なら空）
    let p3 = page(Some(cur(None, "dddd0000001")), 2);
    assert!(p3.is_empty());

    // 同時刻の途中行カーソル：次の同時刻行から続く
    let mid = page(
        Some(cur(Some("2026-10-08T00:00:00+00:00"), "aaaa0000001")),
        10,
    );
    assert_eq!(
        ids_of(&mid),
        vec!["bbbb0000001", "cccc0000001", "dddd0000001"]
    );

    // ページをまたいだ全件走査に重複も欠落も無い
    let mut all = ids_of(&p1);
    all.extend(ids_of(&p2));
    let mut sorted = all.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(all.len(), sorted.len());
    assert_eq!(all.len(), 4);
}

/// videos_set_kind は 'video' の行だけ更新し、
/// 判定済みの別種別は上書きしない。
#[test]
fn videos_set_kind_only_upgrades_video() {
    let db = Db::connect_in_memory().unwrap();
    let ch = "UCchan000000000000001";
    fn mk<'a>(id: &'a str, kind: &'a str, ch: &'a str) -> NewVideo<'a> {
        NewVideo {
            video_id: id,
            channel_id: ch,
            channel_title: "CH",
            title: id,
            thumbnail_url: None,
            published_at: Some("2026-10-08T00:00:00+00:00"),
            kind,
        }
    }
    db.feed_subscribe(&SubscribeArgs {
        channel_id: ch,
        title: "CH",
        thumbnail_url: None,
        category_id: None,
        entries: &[
            mk("video0000001", "video", ch),
            mk("live00000001", "live", ch),
        ],
        etag: None,
        last_modified: None,
    })
    .unwrap();

    db.videos_set_kind(
        &["video0000001".to_string(), "live00000001".to_string()],
        "short",
    )
    .unwrap();
    let kind_of = |id: &str| {
        let f = FeedFilter::default();
        db.feed_list_filtered(&f, true, FEED_LIST_LIMIT, |_| true)
            .unwrap()
            .iter()
            .find(|i| i.video_id == id)
            .map(|i| i.kind.clone())
            .unwrap()
    };
    assert_eq!(kind_of("video0000001"), "short");
    assert_eq!(kind_of("live00000001"), "live");
}

/// チャンネル絞り込み（FR-21、仕様決定 AN）。
/// `channel_id` 指定時はそのチャンネルの項目だけが返る。
#[test]
fn feed_list_filtered_by_channel() {
    let db = Db::connect_in_memory().unwrap();
    let ch_a = "UCchan0000000000000a1";
    let ch_b = "UCchan0000000000000b2";
    fn mk<'a>(id: &'a str, ch: &'a str) -> NewVideo<'a> {
        NewVideo {
            video_id: id,
            channel_id: ch,
            channel_title: "CH",
            title: id,
            thumbnail_url: None,
            published_at: Some("2026-10-08T00:00:00+00:00"),
            kind: "video",
        }
    }
    for (ch, id) in [(ch_a, "aaaa0000001"), (ch_b, "bbbb0000001")] {
        db.feed_subscribe(&SubscribeArgs {
            channel_id: ch,
            title: "CH",
            thumbnail_url: None,
            category_id: None,
            entries: &[mk(id, ch)],
            etag: None,
            last_modified: None,
        })
        .unwrap();
    }
    let ids = |f: FeedFilter| {
        db.feed_list_filtered(&f, true, FEED_LIST_LIMIT, |_| true)
            .unwrap()
            .iter()
            .map(|i| i.video_id.clone())
            .collect::<Vec<_>>()
    };
    // 絞り込みなしは両チャンネル
    assert_eq!(ids(FeedFilter::default()).len(), 2);
    // チャンネル指定はそのチャンネルのみ
    let f = FeedFilter {
        channel_id: Some(ch_a.to_string()),
        ..Default::default()
    };
    assert_eq!(ids(f), vec!["aaaa0000001"]);
    // 未読のみと併用しても効く
    db.videos_mark_read(&["aaaa0000001".to_string()]).unwrap();
    let f = FeedFilter {
        channel_id: Some(ch_a.to_string()),
        unread_only: true,
        ..Default::default()
    };
    assert!(ids(f).is_empty());
}

/// バックフィル投入（FR-21、仕様決定 AN）。
/// 新規行は既読・投入済み、`ingested=0` のプレースホルダは
/// 既読のまま確定し、投入済みの既存行は書き換えない。
#[test]
fn feed_backfill_ingest_inserts_read_and_confirms_placeholder() {
    let db = Db::connect_in_memory().unwrap();
    let ch = "UCchan000000000000001";
    db.feed_subscribe(&SubscribeArgs {
        channel_id: ch,
        title: "テストCH",
        thumbnail_url: None,
        category_id: None,
        entries: &[],
        etag: None,
        last_modified: None,
    })
    .unwrap();
    // ライブラリ登録のプレースホルダ（is_read=1・ingested=0）
    let mut v = vref("place1234567", "ライブラリ由来");
    v.channel_id = Some(ch.to_string());
    db.favorite_add(&v).unwrap();
    // RSS で投入済みの行。バックフィル対象外として温存されるべき行
    let rss = NewVideo {
        video_id: "rssvid00001",
        channel_id: ch,
        channel_title: "テストCH",
        title: "RSS 元タイトル",
        thumbnail_url: None,
        published_at: Some("2026-10-07T00:00:00+00:00"),
        kind: "video",
    };
    db.feed_ingest(ch, std::slice::from_ref(&rss), None, None)
        .unwrap();

    fn entry<'a>(id: &'a str, title: &'a str, kind: &'a str, ch: &'a str) -> NewVideo<'a> {
        NewVideo {
            video_id: id,
            channel_id: ch,
            channel_title: "テストCH",
            title,
            thumbnail_url: None,
            published_at: None,
            kind,
        }
    }
    // 新規 2 件 + プレースホルダ 1 件 + 投入済み行への重複 1 件
    let n = db
        .feed_backfill_ingest(
            ch,
            &[
                entry("pastvid0001", "過去動画", "video", ch),
                entry("pastlive001", "アーカイブ", "live", ch),
                entry("place1234567", "タブでの名前", "video", ch),
                entry("rssvid00001", "違う名前", "video", ch),
            ],
        )
        .unwrap()
        .unwrap();
    // 新規 2 + プレースホルダ確定 1。投入済み行の更新はカウント外
    assert_eq!(n, 3);

    let item_of = |id: &str| {
        db.feed_list_filtered(&FeedFilter::default(), true, FEED_LIST_LIMIT, |_| true)
            .unwrap()
            .into_iter()
            .find(|i| i.video_id == id)
    };
    let past = item_of("pastvid0001").unwrap();
    assert!(past.is_read);
    assert_eq!(past.kind, "video");
    assert_eq!(past.published_at, None);
    let live = item_of("pastlive001").unwrap();
    assert_eq!(live.kind, "live");
    // プレースホルダは既読のままメタが埋まって一覧へ出る
    let ph = item_of("place1234567").unwrap();
    assert!(ph.is_read);
    assert_eq!(ph.title, "タブでの名前");
    // 投入済み行はタイトルも投稿日も書き換わらない
    let r = item_of("rssvid00001").unwrap();
    assert_eq!(r.title, "RSS 元タイトル");
    assert_eq!(r.published_at.as_deref(), Some("2026-10-07T00:00:00+00:00"));

    // 未購読チャンネルは None
    assert!(db
        .feed_backfill_ingest("UCabsent00000000000001", &[])
        .unwrap()
        .is_none());
}

/// バックフィル取得済み位置の読み書き（FR-21・FR-55、仕様決定 AN・CF）。
/// `/shorts` タブの位置も `/videos`・`/streams` と独立に保持される。
#[test]
fn channel_backfill_positions_roundtrip() {
    let db = Db::connect_in_memory().unwrap();
    let ch = "UCchan000000000000001";
    db.feed_subscribe(&SubscribeArgs {
        channel_id: ch,
        title: "CH",
        thumbnail_url: None,
        category_id: None,
        entries: &[],
        etag: None,
        last_modified: None,
    })
    .unwrap();
    assert_eq!(db.channel_backfill_positions(ch).unwrap(), (0, 0, 0));
    db.channel_set_backfill_positions(ch, 100, 50, 30).unwrap();
    assert_eq!(db.channel_backfill_positions(ch).unwrap(), (100, 50, 30));
    // 存在しないチャンネルは (0, 0, 0)
    assert_eq!(
        db.channel_backfill_positions("UCabsent00000000000001")
            .unwrap(),
        (0, 0, 0)
    );
}

/// DB-LD-18: /shorts タブからのバックフィル投入（FR-55、仕様決定 CF）。
/// ショート行は kind='short'・既読・投入済みで積まれる（/videos・/streams と
/// 同じ既読投入で、kind だけ取り込み元タブに対応する）。
#[test]
fn feed_backfill_ingest_stores_shorts_as_short_kind() {
    let db = Db::connect_in_memory().unwrap();
    let ch = "UCchan000000000000001";
    db.feed_subscribe(&SubscribeArgs {
        channel_id: ch,
        title: "テストCH",
        thumbnail_url: None,
        category_id: None,
        entries: &[],
        etag: None,
        last_modified: None,
    })
    .unwrap();
    let n = db
        .feed_backfill_ingest(
            ch,
            &[NewVideo {
                video_id: "short0000001",
                channel_id: ch,
                channel_title: "テストCH",
                title: "過去ショート",
                thumbnail_url: None,
                published_at: None,
                kind: "short",
            }],
        )
        .unwrap()
        .unwrap();
    assert_eq!(n, 1);
    let item = db
        .feed_list_filtered(&FeedFilter::default(), true, FEED_LIST_LIMIT, |_| true)
        .unwrap()
        .into_iter()
        .find(|i| i.video_id == "short0000001")
        .unwrap();
    // 種別フィルタの 'short' で拾える投入となる
    assert_eq!(item.kind, "short");
    assert!(item.is_read);
}

/// 再生中チャンネル解決の第 1 段（FR-12）。
/// 空文字 channel_id の行は「未知」として None を返す。
#[test]
fn video_channel_normalizes_empty() {
    let db = Db::connect_in_memory().unwrap();
    let ch = "UCchan000000000000001";
    db.feed_subscribe(&SubscribeArgs {
        channel_id: ch,
        title: "CH",
        thumbnail_url: None,
        category_id: None,
        entries: &[NewVideo {
            video_id: "video0000001",
            channel_id: ch,
            channel_title: "CH",
            title: "v1",
            thumbnail_url: None,
            published_at: Some("2026-10-08T00:00:00+00:00"),
            kind: "video",
        }],
        etag: None,
        last_modified: None,
    })
    .unwrap();
    // channel_id 空文字の行はお気に入り経由の台帳登録で作る
    db.favorite_add(&crate::model::VideoRef {
        video_id: "video0000002".to_string(),
        title: "v2".to_string(),
        channel_id: None,
        channel_title: Some("手動登録".to_string()),
        thumbnail_url: None,
    })
    .unwrap();

    let (cid, ct) = db.video_channel("video0000001").unwrap();
    assert_eq!(cid.as_deref(), Some(ch));
    assert_eq!(ct.as_deref(), Some("CH"));
    let (cid, ct) = db.video_channel("video0000002").unwrap();
    assert!(cid.is_none());
    assert_eq!(ct.as_deref(), Some("手動登録"));
    let (cid, _) = db.video_channel("missing00001").unwrap();
    assert!(cid.is_none());
}

/// DB-LD-23: 自動既読の「再生済み」ルール（FR-64、仕様決定 CO）。
/// `feed_auto_read(played=true, days=0)` は `watch_history` に行がある
/// 未読の投入済み項目だけ既読にし、履歴の無い項目は未読のまま残す。
/// 既に既読の項目と `ingested=0` のプレースホルダには影響しない。
#[test]
fn feed_auto_read_marks_played_items() {
    let db = Db::connect_in_memory().unwrap();
    let ch = "UCchan000000000000001";
    fn mk<'a>(id: &'a str, ch: &'a str) -> NewVideo<'a> {
        NewVideo {
            video_id: id,
            channel_id: ch,
            channel_title: "CH",
            title: id,
            thumbnail_url: None,
            published_at: Some("2026-10-08T00:00:00+00:00"),
            kind: "video",
        }
    }
    db.feed_subscribe(&SubscribeArgs {
        channel_id: ch,
        title: "CH",
        thumbnail_url: None,
        category_id: None,
        entries: &[
            mk("played000001", ch),
            mk("unplayed0001", ch),
            mk("alreadyread1", ch),
        ],
        etag: None,
        last_modified: None,
    })
    .unwrap();
    // 既読済みの項目と、再生履歴（再生開始で作る行）を持つ未読項目を用意する
    db.videos_mark_read(&["alreadyread1".to_string()]).unwrap();
    db.history_upsert("played000001").unwrap();

    // 再生済みルールだけ有効な適用（日数ルールは無効の 0）
    let n = db.feed_auto_read(true, 0).unwrap();
    assert_eq!(n, 1);

    let is_read = |id: &str| {
        db.feed_list_filtered(&FeedFilter::default(), true, FEED_LIST_LIMIT, |_| true)
            .unwrap()
            .into_iter()
            .find(|i| i.video_id == id)
            .unwrap()
            .is_read
    };
    assert!(is_read("played000001"));
    assert!(!is_read("unplayed0001"));
    assert!(is_read("alreadyread1"));
    // 再適用は冪等（既読しか残っていなければ 0 件）
    assert_eq!(db.feed_auto_read(true, 0).unwrap(), 0);
}

/// DB-LD-24: 自動既読の「N 日経過」ルール（FR-64、仕様決定 CO）。
/// 投稿日時が N 日を経過した未読の投入済み項目だけ既読にし、N 日以内の
/// 項目は未読のまま残す。`published_at` が NULL の項目は `first_seen_at`
/// で判定する（暫定解釈）。`days` が 0 以下のときは適用しない。
#[test]
fn feed_auto_read_marks_aged_items() {
    let db = Db::connect_in_memory().unwrap();
    let ch = "UCchan000000000000001";
    fn mk<'a>(id: &'a str, published_at: Option<&'a str>, ch: &'a str) -> NewVideo<'a> {
        NewVideo {
            video_id: id,
            channel_id: ch,
            channel_title: "CH",
            title: id,
            thumbnail_url: None,
            published_at,
            kind: "video",
        }
    }
    db.feed_subscribe(&SubscribeArgs {
        channel_id: ch,
        title: "CH",
        thumbnail_url: None,
        category_id: None,
        entries: &[
            // 十分昔の投稿日（30 日超は確実に経過）
            mk("agedvid00001", Some("2000-01-01T00:00:00+00:00"), ch),
            // 未来の投稿日（どの時点でも 30 日経過にはならない）
            mk("recent000001", Some("2999-01-01T00:00:00+00:00"), ch),
            // 投稿日なし: first_seen_at（投入時刻）で判定するため経過しない
            mk("nodate000001", None, ch),
        ],
        etag: None,
        last_modified: None,
    })
    .unwrap();

    // days=0（無効）では何も既読にならない
    assert_eq!(db.feed_auto_read(false, 0).unwrap(), 0);

    let n = db.feed_auto_read(false, 30).unwrap();
    assert_eq!(n, 1);

    let is_read = |id: &str| {
        db.feed_list_filtered(&FeedFilter::default(), true, FEED_LIST_LIMIT, |_| true)
            .unwrap()
            .into_iter()
            .find(|i| i.video_id == id)
            .unwrap()
            .is_read
    };
    assert!(is_read("agedvid00001"));
    assert!(!is_read("recent000001"));
    assert!(!is_read("nodate000001"));

    // 経過した項目を含む残りがあっても、同じ日数では新たに既読にならない
    assert_eq!(db.feed_auto_read(false, 30).unwrap(), 0);
}

/// DB-LD-36: `feed_list_filtered` の全 4 分岐 `(unread_only, channel_id.is_some())`
/// について `EXPLAIN QUERY PLAN` を検査し、`SCAN v` 素走査および
/// `USE TEMP B-TREE` が 0 件であること、かつ全分岐で期待どおりの降順結果を返すことを
/// 検証する（F-08）。
#[test]
fn feed_list_filtered_query_plans_avoid_table_scan_and_temp_btree() {
    let db = Db::connect_in_memory().unwrap();
    let cat = db.category_create("技術").unwrap();
    let ch1 = "UCplan000000000000000001";
    let ch2 = "UCplan000000000000000002";
    let mk = |id: &'static str, ch: &'static str, pub_at: &'static str| NewVideo {
        video_id: id,
        channel_id: ch,
        channel_title: "CH",
        title: id,
        thumbnail_url: None,
        published_at: Some(pub_at),
        kind: "video",
    };
    db.feed_subscribe(&SubscribeArgs {
        channel_id: ch1,
        title: "CH1",
        thumbnail_url: None,
        category_id: Some(cat.id),
        entries: &[
            mk("planvid0001", ch1, "2026-11-20T10:00:00+00:00"),
            mk("planvid0002", ch1, "2026-11-19T10:00:00+00:00"),
        ],
        etag: None,
        last_modified: None,
    })
    .unwrap();
    db.feed_subscribe(&SubscribeArgs {
        channel_id: ch2,
        title: "CH2",
        thumbnail_url: None,
        category_id: None,
        entries: &[mk("planvid0003", ch2, "2026-11-21T10:00:00+00:00")],
        etag: None,
        last_modified: None,
    })
    .unwrap();
    db.videos_mark_read(&["planvid0002".to_string()]).unwrap();

    // 4 分岐すべての EXPLAIN QUERY PLAN を検査（カテゴリ・カーソル・期間・種別・shorts 除外の有無を含む）
    let cases = [
        (false, None, "idx_videos_feed_pub"),
        (true, None, "idx_videos_unread"),
        (false, Some(ch1), "idx_videos_channel_pub"),
        (true, Some(ch1), "idx_videos_channel_pub"),
    ];
    let filter_variants = [
        (None, None, None, 1i64, None, None),
        (
            Some(cat.id),
            Some(30i64),
            Some("video"),
            0i64,
            Some("2026-11-20T10:00:00+00:00"),
            Some("planvid0001"),
        ),
    ];
    let assert_plans = |conn: &Connection, stage: &str| {
        for (unread_only, ch, expected_index) in cases {
            for (category_id, days, kind, show_shorts, cur_pub, cur_vid) in filter_variants {
                let sql = format!(
                    "EXPLAIN QUERY PLAN {}",
                    crate::db::feed::feed_list_filtered_sql(unread_only, ch.is_some())
                );
                let mut stmt = conn.prepare(&sql).unwrap();
                let details: Vec<String> = match ch {
                    Some(cid) => stmt
                        .query_map(
                            rusqlite::params![
                                category_id,
                                days,
                                kind,
                                show_shorts,
                                cur_pub,
                                cur_vid,
                                cid,
                            ],
                            |r| r.get::<_, String>(3),
                        )
                        .unwrap()
                        .collect::<Result<_, _>>()
                        .unwrap(),
                    None => stmt
                        .query_map(
                            rusqlite::params![
                                category_id,
                                days,
                                kind,
                                show_shorts,
                                cur_pub,
                                cur_vid,
                            ],
                            |r| r.get::<_, String>(3),
                        )
                        .unwrap()
                        .collect::<Result<_, _>>()
                        .unwrap(),
                };
                let joined = details.join(" | ");
                assert!(
                    !details.iter().any(|d| d.contains("USE TEMP B-TREE")),
                    "[{stage}] unread_only={unread_only}, ch={ch:?}, cat={category_id:?} で USE TEMP B-TREE が発生: {joined}"
                );
                assert!(
                    !details.iter().any(|d| {
                        d == "SCAN v" || (d.starts_with("SCAN v ") && !d.contains("USING "))
                    }),
                    "[{stage}] unread_only={unread_only}, ch={ch:?}, cat={category_id:?} で素の SCAN v が発生: {joined}"
                );
                assert!(
                    details.iter().any(|d| d.contains(expected_index)),
                    "[{stage}] unread_only={unread_only}, ch={ch:?}, cat={category_id:?} で期待索引 {expected_index} が未使用: {joined}"
                );
            }
        }
    };
    {
        let conn = db.lock().unwrap();
        // ANALYZE 前（通常運用時・3 行の小規模データでも索引走査が選ばれる）
        assert_plans(&conn, "before_analyze");
    }

    // 実クエリ結果の整合確認（全 4 分岐＋published_at NULL 混在カーソルページング）
    let ids = |unread_only: bool, channel_id: Option<&str>| -> Vec<String> {
        let f = FeedFilter {
            unread_only,
            channel_id: channel_id.map(str::to_string),
            ..FeedFilter::default()
        };
        db.feed_list_filtered(&f, true, FEED_LIST_LIMIT, |_| true)
            .unwrap()
            .into_iter()
            .map(|it| it.video_id)
            .collect()
    };
    assert_eq!(
        ids(false, None),
        ["planvid0003", "planvid0001", "planvid0002"]
    );
    assert_eq!(ids(true, None), ["planvid0003", "planvid0001"]);
    assert_eq!(ids(false, Some(ch1)), ["planvid0001", "planvid0002"]);
    assert_eq!(ids(true, Some(ch1)), ["planvid0001"]);

    // published_at NULL 混在時のカーソル終端確認（ch1 に NULL 行を 2 件追加）
    db.feed_ingest(
        ch1,
        &[
            NewVideo {
                video_id: "plannull002",
                channel_id: ch1,
                channel_title: "CH1",
                title: "NULL-2",
                thumbnail_url: None,
                published_at: None,
                kind: "video",
            },
            NewVideo {
                video_id: "plannull001",
                channel_id: ch1,
                channel_title: "CH1",
                title: "NULL-1",
                thumbnail_url: None,
                published_at: None,
                kind: "video",
            },
        ],
        None,
        None,
    )
    .unwrap();
    let mut cursor = None;
    let mut paged = Vec::new();
    loop {
        let page = db
            .feed_list_filtered(
                &FeedFilter {
                    unread_only: true,
                    channel_id: Some(ch1.to_string()),
                    cursor: cursor.clone(),
                    ..FeedFilter::default()
                },
                true,
                1,
                |_| true,
            )
            .unwrap();
        if page.is_empty() {
            break;
        }
        let last = page.last().unwrap();
        paged.push(last.video_id.clone());
        cursor = Some(crate::model::FeedCursor {
            published_at: last.published_at.clone(),
            video_id: last.video_id.clone(),
            is_live: last.broadcast_state.as_deref() == Some("live"),
        });
    }
    assert_eq!(paged, ["planvid0001", "plannull001", "plannull002"]);

    // 少数チャンネル（2 チャンネル）＋数百件の videos がある状態で ANALYZE を実行しても、
    // `FROM channels c CROSS JOIN videos v` 型の `SCAN c` 起因 TEMP B-TREE が発生しないことを検証する
    {
        let mut conn = db.lock().unwrap();
        let tx = conn.transaction().unwrap();
        for i in 0..300 {
            let cid = if i < 250 { ch1 } else { ch2 };
            let vid = format!("bulkvid{i:04}");
            let pub_at = format!(
                "2026-{:02}-{:02}T{:02}:00:00+00:00",
                (i % 12) + 1,
                (i % 28) + 1,
                i % 24
            );
            tx.execute(
                "INSERT INTO videos
                   (video_id, channel_id, channel_title, title, published_at, kind, is_read, ingested)
                 VALUES (?1, ?2, 'CH', ?1, ?3, 'video', ?4, 1)",
                rusqlite::params![vid, cid, pub_at, i % 2],
            )
            .unwrap();
        }
        tx.commit().unwrap();
        conn.execute_batch("ANALYZE;").unwrap();
        assert_plans(&conn, "after_analyze");
    }
}

/// DB-LD-39: FR-96/EB — `feed_list_broadcast_sql`（Live / Upcoming × 4 分岐）および
/// `feed_list_filtered_sql`（フェーズ 2 × 4 分岐）のすべてで、`ANALYZE` 前後とも
/// 素の `SCAN v` および `USE TEMP B-TREE` が 0 件であることを検証する。
#[test]
fn feed_list_broadcast_query_plans_avoid_table_scan_and_temp_btree() {
    use crate::db::feed::{feed_list_broadcast_sql, BroadcastQueryState};
    let db = Db::connect_in_memory().unwrap();
    let cat = db.category_create("ゲーム").unwrap();
    let ch1 = "UCbplan0000000000000001";
    let ch2 = "UCbplan0000000000000002";
    let mk = |id: &'static str, ch: &'static str, pub_at: &'static str| NewVideo {
        video_id: id,
        channel_id: ch,
        channel_title: "CH",
        title: id,
        thumbnail_url: None,
        published_at: Some(pub_at),
        kind: "live",
    };
    db.feed_subscribe(&SubscribeArgs {
        channel_id: ch1,
        title: "CH1",
        thumbnail_url: None,
        category_id: Some(cat.id),
        entries: &[mk("blive000001", ch1, "2026-11-27T10:00:00+00:00")],
        etag: None,
        last_modified: None,
    })
    .unwrap();
    db.feed_subscribe(&SubscribeArgs {
        channel_id: ch2,
        title: "CH2",
        thumbnail_url: None,
        category_id: None,
        entries: &[mk("bup00000001", ch2, "2026-11-27T11:00:00+00:00")],
        etag: None,
        last_modified: None,
    })
    .unwrap();
    db.videos_apply_broadcast_refresh("blive000001", Some("live"), Some("live"), Some(1_000), 100, false)
        .unwrap();
    db.videos_apply_broadcast_refresh(
        "bup00000001",
        Some("upcoming"),
        Some("upcoming"),
        Some(2_000),
        100,
        false,
    )
    .unwrap();

    let assert_broadcast_plans = |conn: &Connection, stage: &str| {
        for state in [BroadcastQueryState::Live, BroadcastQueryState::Upcoming] {
            let kind_val = if state == BroadcastQueryState::Live {
                Some("live_now")
            } else {
                Some("upcoming")
            };
            for (unread_only, ch) in [
                (false, None),
                (true, None),
                (false, Some(ch1)),
                (true, Some(ch1)),
            ] {
                for (category_id, days, kind, show_shorts, cur_pub, cur_vid) in [
                    (None, None, kind_val, 1i64, None, None),
                    (
                        Some(cat.id),
                        Some(7i64),
                        if state == BroadcastQueryState::Live {
                            None
                        } else {
                            Some("upcoming")
                        },
                        0i64,
                        Some("2026-11-27T10:00:00+00:00"),
                        Some("blive000001"),
                    ),
                ] {
                    let sql = format!(
                        "EXPLAIN QUERY PLAN {}",
                        feed_list_broadcast_sql(state, unread_only, ch.is_some())
                    );
                    let mut stmt = conn.prepare(&sql).unwrap();
                    let details: Vec<String> = match ch {
                        Some(cid) => stmt
                            .query_map(
                                rusqlite::params![
                                    category_id,
                                    days,
                                    kind,
                                    show_shorts,
                                    cur_pub,
                                    cur_vid,
                                    cid,
                                ],
                                |r| r.get::<_, String>(3),
                            )
                            .unwrap()
                            .collect::<Result<_, _>>()
                            .unwrap(),
                        None => stmt
                            .query_map(
                                rusqlite::params![
                                    category_id,
                                    days,
                                    kind,
                                    show_shorts,
                                    cur_pub,
                                    cur_vid,
                                ],
                                |r| r.get::<_, String>(3),
                            )
                            .unwrap()
                            .collect::<Result<_, _>>()
                            .unwrap(),
                    };
                    let joined = details.join(" | ");
                    assert!(
                        !details.iter().any(|d| d.contains("USE TEMP B-TREE")),
                        "[{stage}] {state:?} unread={unread_only} ch={ch:?} で USE TEMP B-TREE: {joined}"
                    );
                    assert!(
                        !details.iter().any(|d| {
                            d == "SCAN v" || (d.starts_with("SCAN v ") && !d.contains("USING "))
                        }),
                        "[{stage}] {state:?} unread={unread_only} ch={ch:?} で素の SCAN v: {joined}"
                    );
                    let expected_idx_ok = if ch.is_some() {
                        details.iter().any(|d| {
                            d.contains("idx_videos_feed_broadcast")
                                || d.contains("idx_videos_channel_pub")
                        })
                    } else {
                        details
                            .iter()
                            .any(|d| d.contains("idx_videos_feed_broadcast"))
                    };
                    assert!(
                        expected_idx_ok,
                        "[{stage}] {state:?} unread={unread_only} ch={ch:?} で期待索引未使用: {joined}"
                    );
                }
            }
        }
    };

    {
        let conn = db.lock().unwrap();
        assert_broadcast_plans(&conn, "before_analyze");
        let tx = conn.unchecked_transaction().unwrap();
        for i in 0..200 {
            let cid = if i % 2 == 0 { ch1 } else { ch2 };
            let vid = format!("bkup{i:07}");
            let pub_at = format!("2026-10-{:02}T10:00:00+00:00", (i % 28) + 1);
            tx.execute(
                "INSERT INTO videos (video_id, channel_id, channel_title, title, published_at, kind, is_read, ingested)
                 VALUES (?1, ?2, 'CH', ?1, ?3, 'video', ?4, 1)",
                rusqlite::params![vid, cid, pub_at, i % 2],
            )
            .unwrap();
        }
        tx.commit().unwrap();
        conn.execute_batch("ANALYZE;").unwrap();
        assert_broadcast_plans(&conn, "after_analyze");
    }
}

/// DB-LD-40: FR-96/EB — ライブ中先頭配置、2 段階カーソル跨ぎ（NULL 含む）、
/// および 6 種フィルタ × show_shorts の 12 通り判定を検証する。
#[test]
fn feed_list_filtered_live_priority_cursor_and_12_matrix() {
    use crate::model::FeedCursor;
    let db = Db::connect_in_memory().unwrap();
    let ch = "UCmat000000000000000001";
    let entry = |id: &'static str, pub_at: Option<&'static str>, kind: &'static str| NewVideo {
        video_id: id,
        channel_id: ch,
        channel_title: "CH",
        title: id,
        thumbnail_url: None,
        published_at: pub_at,
        kind,
    };
    db.feed_subscribe(&SubscribeArgs {
        channel_id: ch,
        title: "CH",
        thumbnail_url: None,
        category_id: None,
        entries: &[
            entry("vod_newest_", Some("2026-11-27T12:00:00+00:00"), "video"),
            entry("live_older_", Some("2026-11-25T10:00:00+00:00"), "live"),
            entry("live_null__", None, "premiere"),
            entry("up_next____", Some("2026-11-27T11:00:00+00:00"), "upcoming"),
            entry("arch_live__", Some("2026-11-27T10:00:00+00:00"), "live"),
            entry("short_vod__", Some("2026-11-27T09:00:00+00:00"), "short"),
            entry("short_live_", Some("2026-11-24T09:00:00+00:00"), "short"),
            entry("short_up___", Some("2026-11-27T08:00:00+00:00"), "short"),
        ],
        etag: None,
        last_modified: None,
    })
    .unwrap();
    db.videos_apply_broadcast_refresh("live_older_", Some("live"), Some("live"), Some(1_000), 100, false)
        .unwrap();
    db.videos_apply_broadcast_refresh(
        "live_null__",
        Some("premiere"),
        Some("live"),
        Some(1_500),
        100,
        false,
    )
    .unwrap();
    db.videos_apply_broadcast_refresh(
        "up_next____",
        Some("upcoming"),
        Some("upcoming"),
        Some(2_000),
        100,
        false,
    )
    .unwrap();
    db.videos_apply_broadcast_refresh(
        "arch_live__",
        Some("live"),
        Some("ended"),
        Some(500),
        100,
        false,
    )
    .unwrap();
    db.videos_apply_broadcast_refresh("short_live_", None, Some("live"), Some(3_000), 100, false)
        .unwrap();
    db.videos_apply_broadcast_refresh(
        "short_up___",
        None,
        Some("upcoming"),
        Some(4_000),
        100,
        false,
    )
    .unwrap();

    // 1) 全件カーソルページング（limit=2 ずつ、ライブ中先頭＋NULL 末尾→非ライブ先頭へ跨ぐ）
    let mut cur: Option<FeedCursor> = None;
    let mut paged = Vec::new();
    loop {
        let rows = db
            .feed_list_filtered(
                &FeedFilter {
                    cursor: cur.clone(),
                    ..Default::default()
                },
                true,
                2,
                |_| true,
            )
            .unwrap();
        if rows.is_empty() {
            break;
        }
        for r in &rows {
            paged.push(r.video_id.clone());
        }
        let last = rows.last().unwrap();
        cur = Some(FeedCursor {
            published_at: last.published_at.clone(),
            video_id: last.video_id.clone(),
            is_live: last.broadcast_state.as_deref() == Some("live"),
        });
    }
    assert_eq!(
        paged,
        [
            "live_older_", // live & pub 11-25
            "short_live_", // live & pub 11-24
            "live_null__", // live & pub NULL（ライブ中内で末尾）
            "vod_newest_", // 非ライブ先頭 11-27 12:00
            "up_next____", // 11-27 11:00
            "arch_live__", // 11-27 10:00
            "short_vod__", // 11-27 09:00
            "short_up___", // 11-27 08:00
        ]
    );

    // 2) 6 種別フィルタ × show_shorts (true / false) の 12 通り検証
    let ids = |kind: Option<&str>, show_shorts: bool| -> Vec<String> {
        db.feed_list_filtered(
            &FeedFilter {
                kind: kind.map(str::to_string),
                ..Default::default()
            },
            show_shorts,
            FEED_LIST_LIMIT,
            |_| true,
        )
        .unwrap()
        .into_iter()
        .map(|i| i.video_id)
        .collect()
    };

    assert_eq!(
        ids(None, false),
        ["live_older_", "live_null__", "vod_newest_", "up_next____", "arch_live__"]
    );
    assert_eq!(
        ids(Some("video"), true),
        ["vod_newest_"]
    );
    assert_eq!(
        ids(Some("video"), false),
        ["vod_newest_"]
    );
    // Shorts 明示は show_shorts=false でも表示し、short_live_ を先頭に置く
    assert_eq!(
        ids(Some("short"), false),
        ["short_live_", "short_vod__", "short_up___"]
    );
    assert_eq!(
        ids(Some("short"), true),
        ["short_live_", "short_vod__", "short_up___"]
    );
    // 傘「ライブ」は kind IN ('live','upcoming','premiere') でライブ中が先頭
    assert_eq!(
        ids(Some("live"), true),
        ["live_older_", "live_null__", "up_next____", "arch_live__"]
    );
    assert_eq!(
        ids(Some("live"), false),
        ["live_older_", "live_null__", "up_next____", "arch_live__"]
    );
    // 「ライブ中」
    assert_eq!(
        ids(Some("live_now"), true),
        ["live_older_", "short_live_", "live_null__"]
    );
    assert_eq!(
        ids(Some("live_now"), false),
        ["live_older_", "live_null__"]
    );
    // 「配信予定」
    assert_eq!(
        ids(Some("upcoming"), true),
        ["up_next____", "short_up___"]
    );
    assert_eq!(
        ids(Some("upcoming"), false),
        ["up_next____"]
    );
}

/// DB-LD-41: FR-96/EB — 7 日超過打ち切り、緊急度 8:2 選出・間隔別フィルタ・手動強制、
/// 単調時刻ガード、ingested_only、short 保護、touch_broadcast_checked を検証する。
#[test]
fn broadcast_refresh_targets_expiry_and_monotonic_guard() {
    let db = Db::connect_in_memory().unwrap();
    let ch = "UCref000000000000000001";
    let day = 86_400_000i64;
    let min = 60_000i64;
    let now = 2_000_000_000_000i64;

    let mut entries = Vec::new();
    let ids_owned: Vec<String> = (0..14).map(|i| format!("refvid{i:05}")).collect();
    for id in &ids_owned {
        entries.push(NewVideo {
            video_id: id,
            channel_id: ch,
            channel_title: "CH",
            title: id,
            thumbnail_url: None,
            published_at: Some("2026-11-27T00:00:00+00:00"),
            kind: "upcoming",
        });
    }
    db.feed_subscribe(&SubscribeArgs {
        channel_id: ch,
        title: "CH",
        thumbnail_url: None,
        category_id: None,
        entries: &entries,
        etag: None,
        last_modified: None,
    })
    .unwrap();

    // 階層 1 を 9 件（live 8 件 + 直前 upcoming 1 件）、階層 2（遠い upcoming）を 3 件用意し、8:2 枠割当を確認
    for (i, id) in ids_owned.iter().enumerate().take(9) {
        let state = if i < 8 { "live" } else { "upcoming" };
        let start = if i < 8 { now - 30 * min } else { now + 5 * min };
        db.videos_apply_broadcast_refresh(
            id,
            Some(state),
            Some(state),
            Some(start),
            now - 10 * min,
            false,
        )
        .unwrap();
    }
    for id in &ids_owned[9..12] {
        db.videos_apply_broadcast_refresh(
            id,
            Some("upcoming"),
            Some("upcoming"),
            Some(now + 2 * day),
            now - 20 * min,
            false,
        )
        .unwrap();
    }
    // 期限未到達の live (checked 1 分前) と、12 時間超の長時間 live (checked 5 分前、10 分未満なので未到達)
    db.videos_apply_broadcast_refresh(
        &ids_owned[12],
        Some("live"),
        Some("live"),
        Some(now - 2 * min),
        now - min,
        false,
    )
    .unwrap();
    db.videos_apply_broadcast_refresh(
        &ids_owned[13],
        Some("live"),
        Some("live"),
        Some(now - 13 * 3_600_000),
        now - 5 * min,
        false,
    )
    .unwrap();

    // 通常選出（limit=10）は階層 1 が 8 件、階層 2 が 2 件選ばれ、未到達 2 件は入らない
    let picked = db.broadcast_refresh_targets(now, false, 10).unwrap();
    assert_eq!(picked.len(), 10);
    let tier1_count = picked
        .iter()
        .filter(|t| t.broadcast_state == "live" || t.video_id == ids_owned[8])
        .count();
    let tier2_count = picked
        .iter()
        .filter(|t| t.video_id >= ids_owned[9] && t.video_id <= ids_owned[11])
        .count();
    assert_eq!((tier1_count, tier2_count), (8, 2));
    assert!(!picked.iter().any(|t| t.video_id == ids_owned[12]));
    assert!(!picked.iter().any(|t| t.video_id == ids_owned[13]));

    // force_manual=true 時は間隔未到達の id[12]/id[13] も候補に入る
    let manual = db.broadcast_refresh_targets(now, true, 14).unwrap();
    assert_eq!(manual.len(), 14);

    // 7 日超過打ち切り：checked_ms == 0 の未確認行は残し、checked_ms > 0 かつ 7 日超のみ NULL 解除
    db.videos_apply_broadcast_refresh(
        &ids_owned[9],
        Some("upcoming"),
        Some("upcoming"),
        Some(now - 8 * day),
        now - 10 * min,
        false,
    )
    .unwrap();
    {
        let conn = db.lock().unwrap();
        // ingested=0 行は打ち切り対象外、checked=0 行も打ち切り対象外
        conn.execute(
            "INSERT INTO videos (video_id, channel_id, title, kind, ingested, broadcast_state, broadcast_start_ms, broadcast_checked_at_ms)
             VALUES ('un_exp_0001', ?1, 'u', 'upcoming', 0, 'upcoming', ?2, ?3),
                    ('unchk_00001', ?1, 'u2', 'upcoming', 1, 'upcoming', ?2, 0)",
            rusqlite::params![ch, now - 8 * day, now - 10 * min],
        )
        .unwrap();
    }
    assert_eq!(db.videos_expire_stale_upcoming(now).unwrap(), 1);
    // ingested_only=true は ingested=0 行を更新しない
    assert!(!db
        .videos_apply_broadcast_refresh("un_exp_0001", Some("live"), Some("live"), Some(now), now, true)
        .unwrap());

    // 単調時刻ガード：古い now_ms では更新されず false
    assert!(!db
        .videos_apply_broadcast_refresh(&ids_owned[0], Some("live"), Some("ended"), Some(now), now - 20 * min, false)
        .unwrap());

    // touch_broadcast_checked は状態を維持して時刻だけ進める
    db.videos_touch_broadcast_checked(&ids_owned[0], now).unwrap();
    let item0 = db
        .feed_list_filtered(&FeedFilter::default(), true, FEED_LIST_LIMIT, |_| true)
        .unwrap()
        .into_iter()
        .find(|i| i.video_id == ids_owned[0])
        .unwrap();
    assert_eq!(item0.broadcast_state.as_deref(), Some("live"));

    // kind='short' は live へ昇格せず保護される
    {
        let conn = db.lock().unwrap();
        conn.execute(
            "INSERT INTO videos (video_id, channel_id, title, kind, kind_state, ingested)
             VALUES ('shortprot01', ?1, 's', 'short', 1, 1)",
            [ch],
        )
        .unwrap();
    }
    assert!(db
        .videos_apply_broadcast_refresh("shortprot01", Some("live"), Some("live"), Some(now), now, false)
        .unwrap());
    let conn = db.lock().unwrap();
    let (k, bs): (String, Option<String>) = conn
        .query_row(
            "SELECT kind, broadcast_state FROM videos WHERE video_id = 'shortprot01'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!((k.as_str(), bs.as_deref()), ("short", Some("live")));
}
