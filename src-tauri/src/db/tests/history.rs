use crate::db::*;

#[test]
fn history_upsert_then_progress_roundtrip() {
    let db = Db::connect_in_memory().unwrap();
    db.history_upsert("abc123def45").unwrap();
    let h = db.history_get("abc123def45").unwrap().unwrap();
    assert_eq!(h.position_sec, 0);
    assert!(!h.completed);

    db.history_update_progress("abc123def45", "テスト動画", 120.7, Some(300), false)
        .unwrap();
    let h = db.history_get("abc123def45").unwrap().unwrap();
    assert_eq!(h.title, "テスト動画");
    assert_eq!(h.position_sec, 120);
    assert_eq!(h.duration_sec, Some(300));
    assert!(!h.completed);
}

#[test]
fn history_progress_keeps_title_on_empty() {
    let db = Db::connect_in_memory().unwrap();
    db.history_upsert("abc123def45").unwrap();
    db.history_update_progress("abc123def45", "タイトル", 10.0, Some(60), false)
        .unwrap();
    // media-title 未取得の早い保存でタイトルを消さない
    db.history_update_progress("abc123def45", "", 20.0, Some(60), false)
        .unwrap();
    assert_eq!(
        db.history_get("abc123def45").unwrap().unwrap().title,
        "タイトル"
    );
}

#[test]
fn history_completed_resets_position() {
    let db = Db::connect_in_memory().unwrap();
    db.history_upsert("abc123def45").unwrap();
    db.history_update_progress("abc123def45", "t", 290.0, Some(300), false)
        .unwrap();
    db.history_update_progress("abc123def45", "t", 300.0, Some(300), true)
        .unwrap();
    let h = db.history_get("abc123def45").unwrap().unwrap();
    assert!(h.completed);
    assert_eq!(h.position_sec, 0);
}

#[test]
fn history_update_without_upsert_creates_row() {
    let db = Db::connect_in_memory().unwrap();
    // upsert を経由しない直接保存でも行が作られる（upsert 文の両経路を確認）
    db.history_update_progress("new12345678", "v", 5.0, None, false)
        .unwrap();
    assert!(db.history_get("new12345678").unwrap().is_some());
}

/// DB-LD-04: 履歴一覧は新しい順、history_remove で個別削除（FR-7、仕様決定 I）。
#[test]
fn history_list_and_remove() {
    let db = Db::connect_in_memory().unwrap();
    for (i, v) in ["a1", "a2", "a3"].iter().enumerate() {
        db.history_update_progress(v, "t", i as f64, Some(60), false)
            .unwrap();
    }
    let list = db.history_list(None, None, 500).unwrap().items;
    assert_eq!(list.len(), 3);
    // last_watched_at は datetime('now') 同時刻でも rowid 降順で
    // 決定的になる（同一時刻の順序不定を防ぐ）
    assert_eq!(list[0].video_id, "a3");
    assert_eq!(list[1].video_id, "a2");
    assert_eq!(list[2].video_id, "a1");
    assert!(list.iter().all(|h| !h.completed));
    db.history_remove("a2").unwrap();
    let list = db.history_list(None, None, 500).unwrap().items;
    assert_eq!(list.len(), 2);
    assert!(!list.iter().any(|h| h.video_id == "a2"));
    // limit が効く
    assert_eq!(db.history_list(None, None, 1).unwrap().items.len(), 1);
}

/// DB-LD-05: 手動削除した履歴は再生中プレイヤーの進捗保存で復活しない。
/// 明示的な再生開始（history_upsert）で抑止は解除される。
#[test]
fn history_remove_suppresses_inflight_progress() {
    let db = Db::connect_in_memory().unwrap();
    db.history_upsert("abc123def45").unwrap();
    db.history_update_progress("abc123def45", "t", 10.0, Some(60), false)
        .unwrap();
    // 視聴中にライブラリから削除 → 以後の進捗保存は書き込まない
    db.history_remove("abc123def45").unwrap();
    db.history_update_progress("abc123def45", "t", 30.0, Some(60), false)
        .unwrap();
    assert!(db.history_get("abc123def45").unwrap().is_none());
    // 明示的な再生開始で抑止解除 → 履歴が再び残る
    db.history_upsert("abc123def45").unwrap();
    db.history_update_progress("abc123def45", "t", 5.0, Some(60), false)
        .unwrap();
    let h = db.history_get("abc123def45").unwrap().unwrap();
    assert_eq!(h.position_sec, 5);

    // 削除と進捗保存が並行しても、remove 完了後に履歴は復活しない
    // （抑止確認と書き込みが conn ロック内で直列化されていることの検証）
    let db2 = db.clone();
    let writer = std::thread::spawn(move || {
        for _ in 0..50 {
            let _ = db2.history_update_progress("conc1234567", "t", 1.0, None, false);
        }
    });
    db.history_upsert("conc1234567").unwrap();
    db.history_remove("conc1234567").unwrap();
    writer.join().unwrap();
    db.history_update_progress("conc1234567", "t", 1.0, None, false)
        .unwrap();
    assert!(db.history_get("conc1234567").unwrap().is_none());
}

/// DB-LD-10: 保持上限 10000 件を超える履歴は起動時剪定で最古から消える
/// （FR-7、仕様決定 AV）。
#[test]
fn history_prune_keeps_recent_cap() {
    let db = Db::connect_in_memory().unwrap();
    // last_watched_at を古い順にずらして 10001 件を直接仕込む
    {
        let mut conn = db.lock().unwrap();
        let tx = conn.transaction().unwrap();
        for i in 0..10001 {
            tx.execute(
                "INSERT INTO watch_history (video_id, title, last_watched_at)
                 VALUES (?1, 't', datetime('2020-01-01', '+' || ?2 || ' seconds'))",
                rusqlite::params![format!("v{i}"), i],
            )
            .unwrap();
        }
        tx.commit().unwrap();
    }
    db.history_prune().unwrap();
    let list = db.history_list(None, None, 10001).unwrap().items;
    assert_eq!(list.len(), 10000);
    // 最新（v10000）が先頭、境界の v1 が最後に残り、最古の v0 が消える
    assert_eq!(list[0].video_id, "v10000");
    assert_eq!(list[9999].video_id, "v1");
    assert!(db.history_get("v0").unwrap().is_none());
    // 上限以内に戻った後は何も消さない
    db.history_prune().unwrap();
    assert_eq!(
        db.history_list(None, None, 10001).unwrap().items.len(),
        10000
    );
}

/// DB-LD-13: 履歴一覧のカーソルページング（FR-52、仕様決定 CC）。
/// ページをまたいで重複・欠落なく全件取れ、末尾到達で next_cursor が
/// None になる。同時刻の並び（rowid 降順）の境界でもカーソルが進む。
#[test]
fn history_list_paginates_with_cursor() {
    let db = Db::connect_in_memory().unwrap();
    {
        let mut conn = db.lock().unwrap();
        let tx = conn.transaction().unwrap();
        // last_watched_at を 1 秒ずつずらして確定順にする（v6 が最新）
        for i in 0..7 {
            tx.execute(
                "INSERT INTO watch_history (video_id, title, last_watched_at)
                 VALUES (?1, 't', datetime('2024-01-01', '+' || ?2 || ' seconds'))",
                rusqlite::params![format!("v{i}"), i],
            )
            .unwrap();
        }
        // 同時刻の行を末尾側（より新しい側）に 3 件足す（s2 が先頭になる）
        for i in 0..3 {
            tx.execute(
                "INSERT INTO watch_history (video_id, title, last_watched_at)
                 VALUES (?1, 't', '2025-01-01 00:00:00')",
                [format!("s{i}")],
            )
            .unwrap();
        }
        tx.commit().unwrap();
    }
    // 3 件ずつ: s2,s1,s0 / v6,v5,v4 / v3,v2,v1 / v0
    let p1 = db.history_list(None, None, 3).unwrap();
    assert_eq!(
        p1.items
            .iter()
            .map(|h| h.video_id.as_str())
            .collect::<Vec<_>>(),
        ["s2", "s1", "s0"]
    );
    assert!(p1.next_cursor.is_some());
    let p2 = db.history_list(p1.next_cursor.as_ref(), None, 3).unwrap();
    assert_eq!(
        p2.items
            .iter()
            .map(|h| h.video_id.as_str())
            .collect::<Vec<_>>(),
        ["v6", "v5", "v4"]
    );
    let p3 = db.history_list(p2.next_cursor.as_ref(), None, 3).unwrap();
    assert_eq!(
        p3.items
            .iter()
            .map(|h| h.video_id.as_str())
            .collect::<Vec<_>>(),
        ["v3", "v2", "v1"]
    );
    let p4 = db.history_list(p3.next_cursor.as_ref(), None, 3).unwrap();
    assert_eq!(
        p4.items
            .iter()
            .map(|h| h.video_id.as_str())
            .collect::<Vec<_>>(),
        ["v0"]
    );
    // 末尾まで達したら next_cursor は None（空の次ページを要求しない）
    assert!(p4.next_cursor.is_none());
    // 全ページをカーソルで繋いでも重複・欠落がない
    let mut ids = Vec::new();
    let mut cursor = None;
    loop {
        let page = db.history_list(cursor.as_ref(), None, 4).unwrap();
        cursor = page.next_cursor;
        let done = cursor.is_none();
        ids.extend(page.items.iter().map(|h| h.video_id.clone()));
        if done {
            break;
        }
    }
    assert_eq!(ids.len(), 10);
    let mut uniq = ids.clone();
    uniq.sort();
    uniq.dedup();
    assert_eq!(uniq.len(), ids.len());
}

/// DB-LD-15: 履歴削除のスナップショット復元（FR-54、仕様決定 CE）。
/// `history_remove` は削除した行を返し、`history_restore` は全列を
/// 削除前の値で書き戻して保存抑止も解除する（以後の進捗保存が再び効く）。
#[test]
fn history_remove_and_restore_snapshot() {
    let db = Db::connect_in_memory().unwrap();
    db.history_upsert("abc123def45").unwrap();
    db.history_update_progress("abc123def45", "テスト動画", 120.7, Some(300), false)
        .unwrap();
    let before = db.history_get("abc123def45").unwrap().unwrap();

    // 削除はスナップショット（削除時点の全列）を返す
    let removed = db.history_remove("abc123def45").unwrap().unwrap();
    assert_eq!(removed.video_id, before.video_id);
    assert_eq!(removed.title, before.title);
    assert_eq!(removed.position_sec, before.position_sec);
    assert_eq!(removed.duration_sec, before.duration_sec);
    assert_eq!(removed.last_watched_at, before.last_watched_at);
    assert_eq!(removed.completed, before.completed);
    assert!(db.history_get("abc123def45").unwrap().is_none());
    // 対象行が無い削除はスナップショット無し
    assert!(db.history_remove("abc123def45").unwrap().is_none());

    // 復元は全列を削除前の値へ戻す
    db.history_restore(&removed).unwrap();
    let restored = db.history_get("abc123def45").unwrap().unwrap();
    assert_eq!(restored.title, before.title);
    assert_eq!(restored.position_sec, 120);
    assert_eq!(restored.duration_sec, Some(300));
    assert_eq!(restored.last_watched_at, before.last_watched_at);
    assert!(!restored.completed);

    // 復元で保存抑止は解除され、以後の進捗保存は通常どおり書き込まれる
    db.history_update_progress("abc123def45", "テスト動画", 200.0, Some(300), false)
        .unwrap();
    assert_eq!(
        db.history_get("abc123def45").unwrap().unwrap().position_sec,
        200
    );

    // 削除→再生成→復元: スナップショットは削除直前の状態を正として
    // 既存行を置き換える（暫定仕様）
    db.history_remove("abc123def45").unwrap();
    db.history_upsert("abc123def45").unwrap();
    db.history_update_progress("abc123def45", "別タイトル", 5.0, Some(60), false)
        .unwrap();
    db.history_restore(&removed).unwrap();
    let after = db.history_get("abc123def45").unwrap().unwrap();
    assert_eq!(after.title, "テスト動画");
    assert_eq!(after.position_sec, 120);
    assert_eq!(after.last_watched_at, before.last_watched_at);
}

/// DB-LD-14: 履歴一覧の検索絞り込み（FR-61、仕様決定 CL）。
/// タイトル・チャンネル名への部分一致で絞り、空・空白のみの検索語は
/// 絞り込みなしとして扱う。`%`・`_` はワイルドカードではなく文字として探す。
#[test]
fn history_list_search_filters() {
    let db = Db::connect_in_memory().unwrap();
    {
        let mut conn = db.lock().unwrap();
        let tx = conn.transaction().unwrap();
        let rows = [
            ("v1", "Rust 入門", Some("技術チャンネル")),
            ("v2", "料理 カレー", Some("料理チャンネル")),
            ("v3", "100% 果汁", Some("ドリンク")),
            ("v4", "アンダー_スコア", None),
        ];
        for (i, (id, title, ch)) in rows.iter().enumerate() {
            tx.execute(
                "INSERT INTO watch_history
                   (video_id, title, channel_title, last_watched_at)
                 VALUES (?1, ?2, ?3, datetime('2024-01-01', '+' || ?4 || ' seconds'))",
                rusqlite::params![id, title, ch, i],
            )
            .unwrap();
        }
        tx.commit().unwrap();
    }
    let ids = |q: Option<&str>| {
        db.history_list(None, q, 10)
            .unwrap()
            .items
            .iter()
            .map(|h| h.video_id.clone())
            .collect::<Vec<_>>()
    };
    // タイトルの部分一致
    assert_eq!(ids(Some("カレー")), ["v2"]);
    // チャンネル名の部分一致
    assert_eq!(ids(Some("料理")), ["v2"]);
    // channel_title が NULL の行はタイトルだけで拾える
    assert_eq!(ids(Some("スコア")), ["v4"]);
    // `%` と `_` はワイルドカードではなくリテラルとして探す
    assert_eq!(ids(Some("100%")), ["v3"]);
    assert_eq!(ids(Some("アンダー_スコア")), ["v4"]);
    // 空・空白のみの検索語は絞り込みなし
    assert_eq!(ids(Some("   ")).len(), 4);
    assert_eq!(ids(None).len(), 4);
    // 絞り込み中もカーソルページングが効く（ヒット 2 件を 1 件ずつ取る）
    let p1 = db.history_list(None, Some("チャンネル"), 1).unwrap();
    assert_eq!(p1.items.len(), 1);
    assert!(p1.next_cursor.is_some());
    let p2 = db
        .history_list(p1.next_cursor.as_ref(), Some("チャンネル"), 1)
        .unwrap();
    assert_eq!(p2.items.len(), 1);
    assert!(p2.next_cursor.is_none());
    assert_ne!(p1.items[0].video_id, p2.items[0].video_id);
}

/// DB-LD-34: `history_upsert` / `history_update_progress` / `history_set_channel`
/// による視聴履歴のチャンネル補完（`videos` → `live_waits` → `queue_items` の
/// 優先順、既存非空値の保持、逆順到達時の順序非依存性、手動削除抑止の尊重）と、
/// 下流の `history_stats`・`history_list` チャンネル名検索・`video_meta_lookup` の
/// 連動動作を検証する（仕様決定 DY）。
#[test]
fn history_channel_fill_and_downstream_queries() {
    let db = Db::connect_in_memory().unwrap();
    let ch_a = "UCchanA00000000000000001";
    let ch_b = "UCchanB00000000000000002";
    let ch_c = "UCchanC00000000000000003";

    // 1) videos 台帳にある動画: history_upsert / history_update_progress が自動補完
    db.feed_subscribe(&SubscribeArgs {
        channel_id: ch_a,
        title: "チャンネルAlpha",
        thumbnail_url: None,
        category_id: None,
        entries: &[NewVideo {
            video_id: "vid_from_v1",
            channel_id: ch_a,
            channel_title: "チャンネルAlpha",
            title: "購読動画1",
            thumbnail_url: None,
            published_at: Some("2026-11-20T00:00:00+00:00"),
            kind: "video",
        }],
        etag: None,
        last_modified: None,
    })
    .unwrap();
    db.history_upsert("vid_from_v1").unwrap();
    db.history_update_progress("vid_from_v1", "購読動画1", 120.0, Some(600), false)
        .unwrap();
    let h1 = db.history_get("vid_from_v1").unwrap().unwrap();
    assert_eq!(h1.channel_id.as_deref(), Some(ch_a));
    assert_eq!(h1.channel_title.as_deref(), Some("チャンネルAlpha"));

    // 2) videos になく live_waits にある動画: live_waits から補完
    db.live_wait_insert(&crate::model::LiveWait {
        video_id: "vid_from_lw".to_string(),
        title: "待機配信".to_string(),
        channel_id: Some(ch_b.to_string()),
        channel_title: Some("チャンネルBeta".to_string()),
        thumbnail_url: None,
        scheduled_start_ms: None,
        registered_ms: 1_700_000_000_000,
    })
    .unwrap();
    db.history_upsert("vid_from_lw").unwrap();
    db.history_update_progress("vid_from_lw", "待機配信", 0.0, Some(300), true)
        .unwrap();
    let h2 = db.history_get("vid_from_lw").unwrap().unwrap();
    assert_eq!(h2.channel_id.as_deref(), Some(ch_b));
    assert_eq!(h2.channel_title.as_deref(), Some("チャンネルBeta"));

    // 3) queue_items のみにある動画: channel_title だけ補完（channel_id は None）
    db.queue_save(
        &[crate::model::QueueItem {
            uid: 1,
            video_id: "vid_from_q1".to_string(),
            title: "キュー動画".to_string(),
            channel_title: Some("キューCH".to_string()),
            thumbnail_url: None,
        }],
        0,
    )
    .unwrap();
    db.history_update_progress("vid_from_q1", "キュー動画", 45.0, Some(100), false)
        .unwrap();
    let h3 = db.history_get("vid_from_q1").unwrap().unwrap();
    assert_eq!(h3.channel_id, None);
    assert_eq!(h3.channel_title.as_deref(), Some("キューCH"));

    // 4) 逆順テスト: history_set_channel が history_upsert / history_update_progress より
    // 先に到着しても、後続で非空 channel_id / channel_title が保持され、進捗と実タイトルが反映される
    db.history_set_channel(
        "vid_unsub01",
        Some(ch_c),
        Some("未購読Gamma"),
        Some("フォールバック題名"),
    )
    .unwrap();
    db.history_upsert("vid_unsub01").unwrap();
    let before_prog = db.history_get("vid_unsub01").unwrap().unwrap();
    assert_eq!(before_prog.title, "フォールバック題名");
    assert_eq!(before_prog.channel_id.as_deref(), Some(ch_c));
    assert_eq!(before_prog.channel_title.as_deref(), Some("未購読Gamma"));
    // 空タイトルの進捗保存ではフォールバック題名を維持し、非空タイトルで上書きする
    db.history_update_progress("vid_unsub01", "", 60.0, Some(240), false)
        .unwrap();
    assert_eq!(
        db.history_get("vid_unsub01").unwrap().unwrap().title,
        "フォールバック題名"
    );
    db.history_update_progress("vid_unsub01", "未購読動画の実題名", 90.0, Some(240), false)
        .unwrap();
    let h4 = db.history_get("vid_unsub01").unwrap().unwrap();
    assert_eq!(h4.title, "未購読動画の実題名");
    assert_eq!(h4.channel_id.as_deref(), Some(ch_c));
    assert_eq!(h4.channel_title.as_deref(), Some("未購読Gamma"));
    assert_eq!(h4.position_sec, 90);

    // 5) 手動削除後の history_set_channel は抑止されて履歴を復活させない
    db.history_remove("vid_from_q1").unwrap();
    db.history_set_channel("vid_from_q1", Some(ch_c), Some("復活NG"), Some("NG"))
        .unwrap();
    assert!(db.history_get("vid_from_q1").unwrap().is_none());

    // 5b) channel_id=NULL かつ channel_title のみの行と完全不明行が混在する場合:
    // history_stats は channel_id=None 1 グループへ合算し channel_title は None にする
    // （他動画の視聴時間が特定チャンネル名へ誤帰属するのを防ぐ）一方、
    // history_list のチャンネル名検索では名前を保持してヒットする
    db.queue_save(
        &[crate::model::QueueItem {
            uid: 2,
            video_id: "vid_from_q2".to_string(),
            title: "キュー動画2".to_string(),
            channel_title: Some("キューCH".to_string()),
            thumbnail_url: None,
        }],
        0,
    )
    .unwrap();
    db.history_update_progress("vid_from_q2", "キュー動画2", 25.0, Some(100), false)
        .unwrap();
    db.history_update_progress("vid_no_info", "完全不明動画", 40.0, Some(100), false)
        .unwrap();

    // 5c) live_waits 行が自動再生前に削除された後でも、LiveWait 由来のヒントを
    // history_upsert → history_set_channel の順で引き継げばチャンネル情報が保存される
    db.history_upsert("vid_from_lw_hint").unwrap();
    db.history_set_channel(
        "vid_from_lw_hint",
        Some(ch_b),
        Some("チャンネルBeta"),
        Some("自動再生タイトル"),
    )
    .unwrap();
    let h_hint = db.history_get("vid_from_lw_hint").unwrap().unwrap();
    assert_eq!(h_hint.channel_id.as_deref(), Some(ch_b));
    assert_eq!(h_hint.channel_title.as_deref(), Some("チャンネルBeta"));
    assert_eq!(h_hint.title, "自動再生タイトル");

    // 6) 下流連携: history_stats（チャンネル別集計が UC ID ごと＋不明 1 グループに分かれる）
    let stats = db.history_stats().unwrap();
    assert_eq!(stats.entry_count, 6);
    assert_eq!(stats.total_sec, 300 + 120 + 90 + 25 + 40);
    assert_eq!(stats.by_channel.len(), 4);
    assert_eq!(stats.by_channel[0].channel_id.as_deref(), Some(ch_b));
    assert_eq!(stats.by_channel[0].total_sec, 300);
    assert_eq!(stats.by_channel[0].entry_count, 2);
    assert_eq!(stats.by_channel[1].channel_id.as_deref(), Some(ch_a));
    assert_eq!(stats.by_channel[1].total_sec, 120);
    assert_eq!(stats.by_channel[2].channel_id.as_deref(), Some(ch_c));
    assert_eq!(stats.by_channel[2].total_sec, 90);
    assert_eq!(stats.by_channel[3].channel_id, None);
    assert_eq!(stats.by_channel[3].channel_title, None);
    assert_eq!(stats.by_channel[3].total_sec, 65);
    assert_eq!(stats.by_channel[3].entry_count, 2);

    // 7) 下流連携: history_list のチャンネル名検索で未購読動画および名前のみ行がヒットする
    let found = db.history_list(None, Some("未購読Gamma"), 10).unwrap();
    assert_eq!(found.items.len(), 1);
    assert_eq!(found.items[0].video_id, "vid_unsub01");
    let found_q = db.history_list(None, Some("キューCH"), 10).unwrap();
    assert_eq!(found_q.items.len(), 1);
    assert_eq!(found_q.items[0].video_id, "vid_from_q2");

    // 8) 下流連携: videos に無い未購読動画でも video_meta_lookup が履歴からチャンネルを返す
    let meta = db.video_meta_lookup("vid_unsub01").unwrap().unwrap();
    assert_eq!(meta.title, "未購読動画の実題名");
    assert_eq!(meta.channel_id.as_deref(), Some(ch_c));
    assert_eq!(meta.channel_title.as_deref(), Some("未購読Gamma"));
}
