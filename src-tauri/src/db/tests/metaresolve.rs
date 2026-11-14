use crate::db::*;
use crate::model::FeedFilter;

/// 購読チャンネルと動画行をまとめて作る補助。`states` は
/// `(video_id, kind, published_at)` の並びで、そのまま `feed_subscribe`
/// の entries に流す（kind_state/meta_checked は規定値 0/0）。
fn seed(db: &Db, states: &[(&str, &str, Option<&str>)]) {
    let entries: Vec<NewVideo> = states
        .iter()
        .map(|(id, kind, at)| NewVideo {
            video_id: id,
            channel_id: "UCchan000000000000001",
            channel_title: "C",
            title: id,
            thumbnail_url: None,
            published_at: *at,
            kind,
        })
        .collect();
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
}

fn kind_state(db: &Db, video_id: &str) -> i64 {
    db.lock()
        .unwrap()
        .query_row(
            "SELECT kind_state FROM videos WHERE video_id = ?1",
            [video_id],
            |r| r.get(0),
        )
        .unwrap()
}

fn set_state(db: &Db, video_id: &str, kind_state: i64, meta_checked: i64) {
    db.lock()
        .unwrap()
        .execute(
            "UPDATE videos SET kind_state = ?2, meta_checked = ?3
               WHERE video_id = ?1",
            rusqlite::params![video_id, kind_state, meta_checked],
        )
        .unwrap();
}

/// DB-MR-01: 遅延解決の対象選出（FR-53・FR-56・FR-57、仕様決定 CD・CG・CH）。
/// kind 未確定（0）・失敗（2）の行と、投稿日時 NULL でメタ応答未試行の
/// 行が対象。確定済みで投稿日時ありの行・メタ応答試行済みの行は対象外。
/// 対象行は欠けている情報の旗（needs_published / needs_kind）を立てて返す。
#[test]
fn meta_resolve_targets_selects_missing_only() {
    let db = Db::connect_in_memory().unwrap();
    seed(
        &db,
        &[
            ("v_both", "video", None),                              // 未確定 + 日時なし
            ("v_kind", "video", Some("2026-01-01T00:00:00+00:00")), // 未確定のみ
            ("v_pub", "short", None),                               // 確定済み + 日時なし
            ("v_fail", "live", Some("2026-01-01T00:00:00+00:00")),  // 失敗のみ
            ("v_done", "video", Some("2026-01-01T00:00:00+00:00")), // 確定済み + 日時あり
            ("v_checked", "video", None),                           // 確定済み + メタ試行済み
        ],
    );
    set_state(&db, "v_pub", 1, 0); // short 判定で確定済みだが日時は未解決
    set_state(&db, "v_fail", 2, 0);
    set_state(&db, "v_done", 1, 0);
    set_state(&db, "v_checked", 1, 1);

    let targets = db.meta_resolve_targets(50).unwrap();
    let get = |id: &str| targets.iter().find(|t| t.video_id == id);

    // 未確定 + 日時なしは両旗
    let t = get("v_both").expect("v_both は対象");
    assert!(t.needs_published && t.needs_kind);
    // 未確定だけの行は needs_kind のみ
    let t = get("v_kind").expect("v_kind は対象");
    assert!(!t.needs_published && t.needs_kind);
    // 確定済み + 日時なし + 未試行は needs_published のみ
    let t = get("v_pub").expect("v_pub は対象");
    assert!(t.needs_published && !t.needs_kind);
    // 失敗行は再試行対象（needs_kind のみ）
    let t = get("v_fail").expect("v_fail は対象");
    assert!(!t.needs_published && t.needs_kind);
    assert_eq!(t.kind, "live");
    // 確定済み + 日時あり、確定済み + 試行済みは対象外
    assert!(get("v_done").is_none());
    assert!(get("v_checked").is_none());
}

/// DB-MR-02: 対象選出は新しい行から `limit` 件に頭打ちする
/// （FR-53・FR-57、仕様決定 CD の処理量の上限化）。
#[test]
fn meta_resolve_targets_limit_and_order() {
    let db = Db::connect_in_memory().unwrap();
    let rows: Vec<(&str, &str, Option<&str>)> = (0..5)
        .map(|i| {
            (
                match i {
                    0 => "m0",
                    1 => "m1",
                    2 => "m2",
                    3 => "m3",
                    _ => "m4",
                },
                "video",
                None,
            )
        })
        .collect();
    seed(&db, &rows);
    // first_seen_at をずらして新旧を固定する（m4 が最も新しい）
    {
        let conn = db.lock().unwrap();
        for (i, (id, _, _)) in rows.iter().enumerate() {
            conn.execute(
                "UPDATE videos SET first_seen_at = datetime('2026-01-01 00:00:00', '+' || ?2 || ' hours')
                   WHERE video_id = ?1",
                rusqlite::params![id, i as i64],
            )
            .unwrap();
        }
    }
    let targets = db.meta_resolve_targets(3).unwrap();
    let ids: Vec<&str> = targets.iter().map(|t| t.video_id.as_str()).collect();
    assert_eq!(ids, vec!["m4", "m3", "m2"]);
}

/// DB-MR-03: 投稿日時の遅延書き戻し（FR-53、仕様決定 CD）。
/// published_at が NULL の行だけを埋め、既存値は上書きしない。
#[test]
fn videos_set_published_at_fills_null_only() {
    let db = Db::connect_in_memory().unwrap();
    seed(
        &db,
        &[
            ("p_null", "video", None),
            ("p_have", "video", Some("2026-01-01T00:00:00+00:00")),
        ],
    );
    assert!(db
        .videos_set_published_at("p_null", "2026-02-02T03:04:05+00:00")
        .unwrap());
    assert!(!db
        .videos_set_published_at("p_have", "2020-01-01T00:00:00+00:00")
        .unwrap());
    let conn = db.lock().unwrap();
    let got: String = conn
        .query_row(
            "SELECT published_at FROM videos WHERE video_id = 'p_null'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(got, "2026-02-02T03:04:05+00:00");
    let kept: String = conn
        .query_row(
            "SELECT published_at FROM videos WHERE video_id = 'p_have'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(kept, "2026-01-01T00:00:00+00:00");
}

/// DB-MR-04: kind の遅延確定の書き戻し（FR-56、仕様決定 CG）。
/// 確定値（'video' 以外）への置き換えと判定済み印を行う。
/// 'short' の行は shorts 判定の結果として kind を維持しつつ確定だけ立てる。
#[test]
fn videos_resolve_kind_updates_and_preserves_short() {
    let db = Db::connect_in_memory().unwrap();
    seed(
        &db,
        &[
            ("r_v", "video", None),
            ("r_s", "short", None),
            ("r_l", "live", None),
        ],
    );
    set_state(&db, "r_s", 1, 0); // 'short' は判定済みのはずだが復帰パターンも見る

    // 'video' → 'live' は値の置き換え（true）で判定済みへ
    assert!(db.videos_resolve_kind("r_v", "live").unwrap());
    assert_eq!(kind_state(&db, "r_v"), 1);
    {
        let conn = db.lock().unwrap();
        let k: String = conn
            .query_row("SELECT kind FROM videos WHERE video_id = 'r_v'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(k, "live");
    }
    // 'short' の行は kind を維持したまま判定済みに（false: 値は変わらない）
    assert!(!db.videos_resolve_kind("r_s", "live").unwrap());
    {
        let conn = db.lock().unwrap();
        let k: String = conn
            .query_row("SELECT kind FROM videos WHERE video_id = 'r_s'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(k, "short");
    }
    assert_eq!(kind_state(&db, "r_s"), 1);
    // 同値の確定（'live' のまま確定）は false だが判定済みへ進む
    assert!(!db.videos_resolve_kind("r_l", "live").unwrap());
    assert_eq!(kind_state(&db, "r_l"), 1);
}

/// DB-MR-05: kind 判定状態の遷移と失敗行の再選出（FR-57、仕様決定 CH）。
/// 失敗（2）は再試行対象に戻る。確定済み（1）は後から失敗へ降格しない。
#[test]
fn videos_set_kind_state_and_retry_selection() {
    let db = Db::connect_in_memory().unwrap();
    seed(
        &db,
        &[
            ("s_p", "video", Some("2026-01-01T00:00:00+00:00")),
            ("s_r", "video", Some("2026-01-01T00:00:00+00:00")),
        ],
    );
    // 失敗を記録すると再選出対象になる（kind_state<>1 の選出条件）
    assert!(db.videos_set_kind_state("s_p", 2).unwrap());
    assert!(db
        .meta_resolve_targets(10)
        .unwrap()
        .iter()
        .any(|t| t.video_id == "s_p"));
    // 確定済みへ進めると対象から外れる
    assert!(db.videos_set_kind_state("s_r", 1).unwrap());
    assert!(!db
        .meta_resolve_targets(10)
        .unwrap()
        .iter()
        .any(|t| t.video_id == "s_r"));
    // 確定済みは失敗へ降格しない（別経路で確定した結果を守る）
    assert!(!db.videos_set_kind_state("s_r", 2).unwrap());
    assert_eq!(kind_state(&db, "s_r"), 1);
}

/// DB-MR-06: shorts 判定の書き戻し（仕様決定 V・CG）。
/// `videos_set_kind` は 'video' の行だけを更新し、書き込んだ行を
/// 判定済み（kind_state=1）にして遅延解決の対象から外す。
#[test]
fn videos_set_kind_marks_resolved() {
    let db = Db::connect_in_memory().unwrap();
    seed(&db, &[("k_v", "video", None), ("k_l", "live", None)]);
    db.videos_set_kind(&["k_v".to_string(), "k_l".to_string()], "short")
        .unwrap();
    let conn = db.lock().unwrap();
    let (k, s): (String, i64) = conn
        .query_row(
            "SELECT kind, kind_state FROM videos WHERE video_id = 'k_v'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!((k.as_str(), s), ("short", 1));
    let (k, s): (String, i64) = conn
        .query_row(
            "SELECT kind, kind_state FROM videos WHERE video_id = 'k_l'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    // 既に 'live' の行は従来どおり上書きしない
    assert_eq!((k.as_str(), s), ("live", 0));
    drop(conn);
    // 'short' 確定行は選出対象から外れる（kind_state=1。投稿日時は
    // まだ欠けているので needs_published のみの対象としては残る）
    let targets = db.meta_resolve_targets(10).unwrap();
    let t = targets.iter().find(|t| t.video_id == "k_v");
    if let Some(t) = t {
        assert!(!t.needs_kind);
    }
}

/// DB-MR-07: v18 マイグレーション（FR-53・FR-56、仕様決定 CG）。
/// kind の CHECK に 'premiere' を追加し、kind_state をバックフィルする:
/// 'short' は確定済み（1）、それ以外は未確定（0）。meta_checked は 0 始まり。
#[test]
fn migrate_v18_adds_premiere_and_kind_state() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE schema_migrations (
           version INTEGER PRIMARY KEY,
           applied_at TEXT NOT NULL DEFAULT (datetime('now'))
         );",
    )
    .unwrap();
    for m in migrations::MIGRATIONS.iter().filter(|m| m.version <= 17) {
        conn.execute_batch(m.sql).unwrap();
        conn.execute(
            "INSERT INTO schema_migrations (version) VALUES (?1)",
            [m.version],
        )
        .unwrap();
    }
    // v17 時点の videos 行を 3 種類仕込む（kind_state/meta_checked はまだ無い）
    conn.execute(
        "INSERT INTO channels (channel_id, title) VALUES ('UCchan000000000000001', 'C')",
        [],
    )
    .unwrap();
    for (id, kind) in [("mv_v", "video"), ("mv_s", "short"), ("mv_l", "live")] {
        conn.execute(
            "INSERT INTO videos
               (video_id, channel_id, title, kind, is_read, ingested)
             VALUES (?1, 'UCchan000000000000001', 't', ?2, 1, 1)",
            rusqlite::params![id, kind],
        )
        .unwrap();
    }
    let v18 = migrations::MIGRATIONS
        .iter()
        .find(|m| m.version == 18)
        .expect("v18 マイグレーション");
    conn.execute_batch(v18.sql).unwrap();
    conn.execute("INSERT INTO schema_migrations (version) VALUES (18)", [])
        .unwrap();

    // バックフィル: 'short' のみ確定済み、'video'/'live' は未確定へ戻す
    let state = |id: &str| -> (String, i64, i64) {
        conn.query_row(
            "SELECT kind, kind_state, meta_checked FROM videos WHERE video_id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap()
    };
    assert_eq!(state("mv_v"), ("video".to_string(), 0, 0));
    assert_eq!(state("mv_s"), ("short".to_string(), 1, 0));
    assert_eq!(state("mv_l"), ("live".to_string(), 0, 0));
    // 'premiere' が CHECK に追加されている
    conn.execute(
        "INSERT INTO videos
           (video_id, channel_id, title, kind, is_read, ingested)
         VALUES ('mv_p', 'UCchan000000000000001', 't', 'premiere', 1, 1)",
        [],
    )
    .unwrap();
    assert_eq!(state("mv_p").0, "premiere");
    // 値域外は引き続き拒否
    assert!(conn
        .execute(
            "INSERT INTO videos
               (video_id, channel_id, title, kind, is_read, ingested)
             VALUES ('mv_x', 'UCchan000000000000001', 't', 'bogus', 1, 1)",
            [],
        )
        .is_err());
}

/// DB-MR-08: フィードの種別フィルタ 'live' は放送系の傘として
/// live / upcoming / premiere をまとめて拾う（FR-56、仕様決定 CG の暫定解釈）。
#[test]
fn feed_filter_live_covers_broadcast_kinds() {
    let db = Db::connect_in_memory().unwrap();
    seed(
        &db,
        &[
            ("f_v", "video", Some("2026-01-01T00:00:00+00:00")),
            ("f_s", "short", Some("2026-01-01T00:00:00+00:00")),
            ("f_l", "live", Some("2026-01-01T00:00:00+00:00")),
            ("f_u", "upcoming", Some("2026-01-01T00:00:00+00:00")),
            ("f_p", "premiere", Some("2026-01-01T00:00:00+00:00")),
        ],
    );
    let filter = FeedFilter {
        unread_only: false,
        category_id: None,
        days: None,
        kind: Some("live".to_string()),
        cursor: None,
        channel_id: None,
    };
    let mut ids: Vec<String> = db
        .feed_list_filtered(&filter, true, FEED_LIST_LIMIT, |_| true)
        .unwrap()
        .into_iter()
        .map(|i| i.video_id)
        .collect();
    ids.sort();
    assert_eq!(ids, vec!["f_l", "f_p", "f_u"]);
    // 'video' 明示選択は従来どおり 'video' のみ
    let filter = FeedFilter {
        kind: Some("video".to_string()),
        ..filter
    };
    let ids: Vec<String> = db
        .feed_list_filtered(&filter, true, FEED_LIST_LIMIT, |_| true)
        .unwrap()
        .into_iter()
        .map(|i| i.video_id)
        .collect();
    assert_eq!(ids, vec!["f_v"]);
}
