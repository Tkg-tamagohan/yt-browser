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

/// バックフィル取得済み位置の読み書き（FR-21、仕様決定 AN）。
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
    assert_eq!(db.channel_backfill_positions(ch).unwrap(), (0, 0));
    db.channel_set_backfill_positions(ch, 100, 50).unwrap();
    assert_eq!(db.channel_backfill_positions(ch).unwrap(), (100, 50));
    // 存在しないチャンネルは (0, 0)
    assert_eq!(
        db.channel_backfill_positions("UCabsent00000000000001")
            .unwrap(),
        (0, 0)
    );
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
