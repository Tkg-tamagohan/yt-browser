use super::*;
use crate::model::{FeedFilter, PlaylistEntry};

#[test]
fn migrate_applies_all_and_is_idempotent() {
    let db = Db::connect_in_memory().unwrap();
    let v = db.schema_version().unwrap();
    assert_eq!(v, migrations::MIGRATIONS.last().unwrap().version);
    // 2 回目の migrate は何も適用しない
    db.migrate().unwrap();
    assert_eq!(db.schema_version().unwrap(), v);
}

#[test]
fn settings_roundtrip() {
    let db = Db::connect_in_memory().unwrap();
    assert_eq!(db.setting_get("missing").unwrap(), None);
    db.setting_set("k", "v1").unwrap();
    assert_eq!(db.setting_get("k").unwrap().as_deref(), Some("v1"));
    db.setting_set("k", "v2").unwrap();
    assert_eq!(db.setting_get("k").unwrap().as_deref(), Some("v2"));
}

#[test]
fn reopen_preserves_data() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    {
        let db = Db::connect(&path).unwrap();
        db.setting_set("persist", "yes").unwrap();
    }
    let db = Db::connect(&path).unwrap();
    assert_eq!(db.setting_get("persist").unwrap().as_deref(), Some("yes"));
}

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

/// v4 マイグレーション: UC プレフィックスなしで保存された channel_id の修復。
/// v3 スキーマまで適用した DB に UC 無しのデータを埋めてから v4 を適用する。
#[test]
fn migrate_v4_normalizes_uc_less_channel_ids() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE schema_migrations (
           version INTEGER PRIMARY KEY,
           applied_at TEXT NOT NULL DEFAULT (datetime('now'))
         );",
    )
    .unwrap();
    for m in migrations::MIGRATIONS.iter().filter(|m| m.version <= 3) {
        conn.execute_batch(m.sql).unwrap();
        conn.execute(
            "INSERT INTO schema_migrations (version) VALUES (?1)",
            [m.version],
        )
        .unwrap();
    }
    // UC 無しの購読・ブロック・動画・履歴と、UC 付きの重複チャンネルを仕込む。
    // 旧行はカテゴリ・古い購読日時を持つ（衝突統合で引き継がれるべき値）
    conn.execute("INSERT INTO categories (id, name) VALUES (7, 'tech')", [])
        .unwrap();
    conn.execute(
        "INSERT INTO channels (channel_id, title, category_id, subscribed_at)
         VALUES ('XuqSBlHAE6Xw-yeJA0Tunw', 'LTT', 7, '2025-01-01 00:00:00')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO channels (channel_id, title, subscribed_at)
         VALUES ('UCXuqSBlHAE6Xw-yeJA0Tunw', 'LTT-uc', '2026-10-01 00:00:00')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO channels (channel_id, title) VALUES ('shortOnlyChannel000001', 'OnlyOld')",
        [],
    )
    .unwrap();
    // 先頭が UC で始まる 22 文字の旧形式 id（NOT LIKE 判定では取りこぼされる）
    conn.execute(
        "INSERT INTO videos (video_id, channel_id, title)
         VALUES ('v1', 'XuqSBlHAE6Xw-yeJA0Tunw', 'V1'),
                ('v2', 'UCXuqSBlHAE6Xw-yeJA0Tunw', 'V2'),
                ('v3', 'UCzzzzzzzzzzzzzzzzzzzz', 'V3')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO blocked_channels (channel_id, title) VALUES ('badChannelX00000000000', 'B')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO blocked_channels (channel_id, title) VALUES ('ucBlockedChannel000000', 'B2')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO watch_history (video_id, title, channel_id)
         VALUES ('v1', 'V1', 'XuqSBlHAE6Xw-yeJA0Tunw')",
        [],
    )
    .unwrap();
    let db = Db {
        conn: Arc::new(Mutex::new(conn)),
        history_suppressed: Arc::new(Mutex::new(HashSet::new())),
    };
    db.migrate().unwrap();

    // UC 付きが既にあるチャンネルは UC 無し行が消え、
    // カテゴリと最古の購読日時は UC 付き行へ引き継がれる
    assert!(db.channel_get("XuqSBlHAE6Xw-yeJA0Tunw").unwrap().is_none());
    let merged = db.channel_get("UCXuqSBlHAE6Xw-yeJA0Tunw").unwrap().unwrap();
    assert_eq!(merged.category_id, Some(7));
    assert_eq!(merged.subscribed_at, "2025-01-01 00:00:00");
    // UC 付きが無いチャンネルはリネームされる
    assert!(db
        .channel_get("UCshortOnlyChannel000001")
        .unwrap()
        .is_some());
    // UC 始まりの 22 文字旧 id も正規化される
    assert!(db
        .channel_get("UCUCzzzzzzzzzzzzzzzzzzzz")
        .unwrap()
        .is_none()); // v3 は channels 行を持たないので videos 側だけ
    let conn = db.lock().unwrap();
    let v3_channel: String = conn
        .query_row(
            "SELECT channel_id FROM videos WHERE video_id = 'v3'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(v3_channel, "UCUCzzzzzzzzzzzzzzzzzzzz");
    // blocked_channels は 22 文字の旧 id が UC 付きに正規化される
    for (raw, expected) in [
        ("badChannelX00000000000", "UCbadChannelX00000000000"),
        ("ucBlockedChannel000000", "UCucBlockedChannel000000"),
    ] {
        let n: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM blocked_channels WHERE channel_id = ?1",
                [expected],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1, "{raw} の正規化先が無い");
    }
    // videos / watch_history / blocked_channels も UC 付きに揃う
    let stale: i64 = conn
        .query_row(
            "SELECT (SELECT COUNT(*) FROM channels WHERE length(channel_id) = 22)
                  + (SELECT COUNT(*) FROM videos WHERE length(channel_id) = 22)
                  + (SELECT COUNT(*) FROM blocked_channels WHERE length(channel_id) = 22)
                  + (SELECT COUNT(*) FROM watch_history WHERE length(channel_id) = 22)",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(stale, 0);
    drop(conn);
    // v4 自体も冪等（再適用は schema_migrations で抑止される）
    db.migrate().unwrap();
}

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

/// DB-CH-01: チャットのバッチ保存と FTS5 検索（FR-6）。
/// 本文・投稿者のどちらにもヒットし、video_id で絞り込める。
#[test]
fn chat_insert_and_search() {
    use crate::model::{ChatEvent, ChatKind};
    let db = Db::connect_in_memory().unwrap();
    let ev = |id: i64, vid: &str, author: &str, msg: &str| ChatEvent {
        item_id: String::new(),
        video_id: vid.to_string(),
        posted_at_usec: id,
        author_channel_id: None,
        author_name: Some(author.to_string()),
        kind: ChatKind::Text,
        message: msg.to_string(),
        amount_display: None,
        ng: false,
        video_offset_ms: None,
        raw_json: "{}".to_string(),
    };
    let n = db
        .chat_insert_batch(&[
            ev(1, "v1", "@alice", "こんにちは世界"),
            ev(2, "v1", "@bob", "another line"),
            ev(3, "v2", "@alice", "アルプスの発言"),
        ])
        .unwrap();
    assert_eq!(n, 3);

    // 本文検索（trigram: 部分文字列でもヒットする）
    let hits = db.chat_search(None, "こんにちは", 10).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].message, "こんにちは世界");
    // 投稿者名でもヒットする
    let hits = db.chat_search(None, "alice", 10).unwrap();
    assert_eq!(hits.len(), 2);
    // AND 検索（投稿者＋本文の両方を含む行のみ）
    // 各検索語は trigram の最小語長である 3 文字以上にする
    let hits = db.chat_search(None, "alice アルプ", 10).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].video_id, "v2");
    // video_id 絞り込み
    let hits = db.chat_search(Some("v1"), "alice", 10).unwrap();
    assert_eq!(hits.len(), 1);
    // FTS5 構文を含む入力でも落ちない
    let hits = db.chat_search(None, "NEAR 'こんにちは'", 10).unwrap();
    assert_eq!(hits.len(), 0);
}

/// DB-CH-02: NG フィルタの登録・一覧・削除（FR-9）。
#[test]
fn filter_roundtrip() {
    let db = Db::connect_in_memory().unwrap();
    let f = db.filter_add("chat_text", "literal", "売り込み").unwrap();
    assert_eq!(f.target, "chat_text");
    assert!(f.enabled);
    assert!(!f.created_at.is_empty());
    assert_eq!(db.filter_list().unwrap().len(), 1);
    assert!(db.filter_remove(f.id).unwrap());
    assert!(db.filter_list().unwrap().is_empty());
    // 存在しない ID の削除は false
    assert!(!db.filter_remove(f.id).unwrap());
}

/// DB-CH-03: chat_logs の item_id 一意制約による冪等化（migration v9）。
/// 同じ item_id のイベントを再投入しても無視され、件数は増えない。
/// item_id の無いイベント（NULL）は制約対象外なので重複して入る。
#[test]
fn chat_insert_dedup_by_item_id() {
    use crate::model::{ChatEvent, ChatKind};
    let db = Db::connect_in_memory().unwrap();
    let ev = |id: &str, msg: &str| ChatEvent {
        item_id: id.to_string(),
        video_id: "v1".to_string(),
        posted_at_usec: 1,
        author_channel_id: None,
        author_name: None,
        kind: ChatKind::Text,
        message: msg.to_string(),
        amount_display: None,
        ng: false,
        video_offset_ms: None,
        raw_json: "{}".to_string(),
    };
    assert_eq!(
        db.chat_insert_batch(&[ev("i1", "a"), ev("i2", "b")])
            .unwrap(),
        2
    );
    // バックログ再取得を模して同じイベントを再投入: 全部無視される
    assert_eq!(
        db.chat_insert_batch(&[ev("i1", "a"), ev("i2", "b")])
            .unwrap(),
        0
    );
    // 一部だけ新規: 新規分だけ入る
    assert_eq!(
        db.chat_insert_batch(&[ev("i2", "b"), ev("i3", "c")])
            .unwrap(),
        1
    );
    // 別動画の同じ item_id は別行として入る
    let mut other = ev("i1", "a");
    other.video_id = "v2".to_string();
    assert_eq!(db.chat_insert_batch(&[other]).unwrap(), 1);
    // item_id 空（NULL）は制約対象外で重複して入る
    assert_eq!(
        db.chat_insert_batch(&[ev("", "x"), ev("", "x")]).unwrap(),
        2
    );
    let total: i64 = db
        .lock()
        .unwrap()
        .query_row("SELECT COUNT(*) FROM chat_logs", [], |r| r.get(0))
        .unwrap();
    assert_eq!(total, 6);
}

/// v9 マイグレーション: raw_json ラッパの内側から item_id を復元し、
/// バックフィル由来の重複を最古行だけ残して掃除する。
#[test]
fn migrate_v9_backfills_item_id_and_dedups() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE schema_migrations (
           version INTEGER PRIMARY KEY,
           applied_at TEXT NOT NULL DEFAULT (datetime('now'))
         );",
    )
    .unwrap();
    for m in migrations::MIGRATIONS.iter().filter(|m| m.version < 9) {
        conn.execute_batch(m.sql).unwrap();
        conn.execute(
            "INSERT INTO schema_migrations (version) VALUES (?1)",
            [m.version],
        )
        .unwrap();
    }
    // v8 までの DB を模す: 同じイベントが再保存された重複行を含める
    let ins = |raw: &str| {
        conn.execute(
            "INSERT INTO chat_logs
               (video_id, posted_at_usec, kind, message, raw_json)
             VALUES ('v1', 1, 'text', 'm', ?1)",
            [raw],
        )
        .unwrap();
    };
    ins(r#"{"liveChatTextMessageRenderer":{"id":"item-1"}}"#);
    ins(r#"{"liveChatTextMessageRenderer":{"id":"item-1"}}"#);
    // 削除イベントはラッパなしのフラット形（deleted_to_event が
    // 内側オブジェクトをそのまま raw_json に保存する）
    ins(r#"{"targetItemId":"t-9","deletedStateMessage":{"runs":[{"text":"deleted"}]}}"#);
    ins("{}"); // id を持たない行は NULL のまま
    for m in migrations::MIGRATIONS.iter().filter(|m| m.version == 9) {
        conn.execute_batch(m.sql).unwrap();
    }
    let mut stmt = conn
        .prepare("SELECT item_id, COUNT(*) FROM chat_logs GROUP BY item_id ORDER BY item_id")
        .unwrap();
    let rows: Vec<(Option<String>, i64)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    // 重複は最古の 1 行に掃除され、del: 合成 ID も復元される
    assert_eq!(
        rows,
        vec![
            (None, 1),
            (Some("del:t-9".to_string()), 1),
            (Some("item-1".to_string()), 1),
        ]
    );
    // 一意索引が効いている（復元された item_id と同じ値は再投入できない）
    assert!(conn
        .execute(
            "INSERT INTO chat_logs
               (video_id, posted_at_usec, kind, message, raw_json, item_id)
             VALUES ('v1', 2, 'text', 'm2', '{}', 'item-1')",
            [],
        )
        .is_err());
}

fn vref(video_id: &str, title: &str) -> crate::model::VideoRef {
    crate::model::VideoRef {
        video_id: video_id.to_string(),
        title: title.to_string(),
        channel_id: Some("UCchan000000000000001".to_string()),
        channel_title: Some("テストCH".to_string()),
        thumbnail_url: Some("https://i.ytimg.com/vi/x.jpg".to_string()),
    }
}

/// v7 マイグレーション: ingested のバックフィルは投稿日を持つ行のみ 1 にする。
/// v6 状態の DB に「投稿日ありのフィード行」と「投稿日なしの行」
/// （日付を欠いたフィード行とライブラリ由来プレースホルダは区別不能）を
/// 仕込んでから v7 を適用する。
#[test]
fn migrate_v7_backfills_ingested_by_provenance() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE schema_migrations (
           version INTEGER PRIMARY KEY,
           applied_at TEXT NOT NULL DEFAULT (datetime('now'))
         );",
    )
    .unwrap();
    for m in migrations::MIGRATIONS.iter().filter(|m| m.version <= 6) {
        conn.execute_batch(m.sql).unwrap();
        conn.execute(
            "INSERT INTO schema_migrations (version) VALUES (?1)",
            [m.version],
        )
        .unwrap();
    }
    // 既存行はすべてフィード由来と確定できるため一律 1。
    // フィード由来であり得ない行（お気に入り・プレイリスト参照のある
    // 日付なし行、チャンネルなしの日付なし行＝この PR の開発ビルドで
    // 作られたプレースホルダに限る）だけが 0 に戻る
    conn.execute(
        "INSERT INTO channels (channel_id, title) VALUES ('UCfeedchan00000000001', 'CH')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO videos (video_id, channel_id, title, published_at, is_read)
         VALUES ('dated_ref', 'UCfeedchan00000000001', 'DR', '2026-10-01T00:00:00+00:00', 1),
                ('dated_unref', 'UCfeedchan00000000001', 'DU', '2026-10-01T00:00:00+00:00', 0),
                ('undated_ref', 'UCfeedchan00000000001', 'UR', NULL, 1),
                ('undated_unref', 'UCfeedchan00000000001', 'UU', NULL, 1),
                ('undated_noch', '', 'UN', NULL, 1)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO favorites (video_id) VALUES ('dated_ref'), ('undated_ref')",
        [],
    )
    .unwrap();
    for m in migrations::MIGRATIONS.iter().filter(|m| m.version == 7) {
        conn.execute_batch(m.sql).unwrap();
        conn.execute(
            "INSERT INTO schema_migrations (version) VALUES (?1)",
            [m.version],
        )
        .unwrap();
    }
    let mut stmt = conn
        .prepare("SELECT video_id, ingested FROM videos ORDER BY video_id")
        .unwrap();
    let rows: Vec<(String, i64)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    assert_eq!(
        rows,
        vec![
            ("dated_ref".to_string(), 1),
            ("dated_unref".to_string(), 1),
            ("undated_noch".to_string(), 0),
            // 参照のある日付なし行はプレースホルダ（開発ビルド由来）として 0
            ("undated_ref".to_string(), 0),
            // 未参照の日付なし行はリリース済み DB ではフィード由来しか
            // あり得ないため 1 に保持する
            ("undated_unref".to_string(), 1)
        ]
    );
}

/// v8: この PR の開発途中の 3 状態版 v7 が書き込んだ ingested=2 行を
/// 投入済みに昇格する（リリース済み DB では空操作）。
#[test]
fn migrate_v8_promotes_intermediate_state2() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE schema_migrations (
           version INTEGER PRIMARY KEY,
           applied_at TEXT NOT NULL DEFAULT (datetime('now'))
         );",
    )
    .unwrap();
    // v7 までを適用した状態（3 状態版 v7 を通った開発 DB を模す）
    for m in migrations::MIGRATIONS.iter().filter(|m| m.version < 8) {
        conn.execute_batch(m.sql).unwrap();
        conn.execute(
            "INSERT INTO schema_migrations (version) VALUES (?1)",
            [m.version],
        )
        .unwrap();
    }
    conn.execute(
        "INSERT INTO channels (channel_id, title) VALUES ('UCfeedchan00000000001', 'CH')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO videos (video_id, channel_id, title, published_at, is_read, ingested)
         VALUES ('dev_state2', 'UCfeedchan00000000001', 'S2', NULL, 1, 2),
                ('normal', 'UCfeedchan00000000001', 'N1', '2026-10-01T00:00:00+00:00', 1, 1),
                ('placeholder', 'UCfeedchan00000000001', 'P0', NULL, 1, 0)",
        [],
    )
    .unwrap();
    // 3 状態版の v7 を既に適用済みとした DB に v8 だけを後追い適用する
    for m in migrations::MIGRATIONS.iter().filter(|m| m.version == 8) {
        conn.execute_batch(m.sql).unwrap();
    }
    let mut stmt = conn
        .prepare("SELECT video_id, ingested FROM videos ORDER BY video_id")
        .unwrap();
    let rows: Vec<(String, i64)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    assert_eq!(
        rows,
        vec![
            ("dev_state2".to_string(), 1),
            ("normal".to_string(), 1),
            ("placeholder".to_string(), 0)
        ]
    );
}

/// DB-LD-01: お気に入りの追加・一覧・削除（FR-7）。
/// 動画メタは videos 台帳から JOIN で取り、重複登録は新しい日時に更新しない。
#[test]
fn favorite_roundtrip() {
    let db = Db::connect_in_memory().unwrap();
    db.favorite_add(&vref("dQw4w9WgXcQ", "動画A")).unwrap();
    let list = db.favorite_list().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].title, "動画A");
    assert_eq!(list[0].channel_id.as_deref(), Some("UCchan000000000000001"));
    db.favorite_add(&vref("dQw4w9WgXcQ", "動画A")).unwrap();
    assert_eq!(db.favorite_list().unwrap().len(), 1);
    db.favorite_remove("dQw4w9WgXcQ").unwrap();
    assert!(db.favorite_list().unwrap().is_empty());
}

/// DB-LD-02: video_upsert は非空値で既存メタを補強し、空値は上書きしない。
/// channel_id が取れない入力は台帳では空文字になり、一覧では NULL で返す。
#[test]
fn video_upsert_merges_metadata() {
    let db = Db::connect_in_memory().unwrap();
    db.favorite_add(&vref("dQw4w9WgXcQ", "元タイトル")).unwrap();
    // 空タイトル・channel_id なしで再登録しても既存値を保つ
    let mut sparse = vref("dQw4w9WgXcQ", "");
    sparse.channel_id = None;
    db.favorite_add(&sparse).unwrap();
    let list = db.favorite_list().unwrap();
    assert_eq!(list[0].title, "元タイトル");
    assert_eq!(list[0].channel_id.as_deref(), Some("UCchan000000000000001"));
    // channel_id を一切持たない動画は NULLIF で None に見える
    let mut no_ch = vref("nochan12345", "無名");
    no_ch.channel_id = None;
    no_ch.channel_title = None;
    db.favorite_add(&no_ch).unwrap();
    let e = db
        .favorite_list()
        .unwrap()
        .into_iter()
        .find(|x| x.video_id == "nochan12345")
        .unwrap();
    assert_eq!(e.channel_id, None);
}

/// DB-LD-03: プレイリストの CRUD とアイテム順序（FR-7）。
/// position は末尾追加で連番、重複追加は位置を維持して無視される。
#[test]
fn playlist_crud_and_order() {
    let db = Db::connect_in_memory().unwrap();
    let pl = db.playlist_create("夜の選曲").unwrap();
    assert_eq!(pl.item_count, 0);
    db.playlist_add(pl.id, &vref("aaaaaaaaaa1", "A")).unwrap();
    db.playlist_add(pl.id, &vref("bbbbbbbbbb2", "B")).unwrap();
    // 重複追加は位置を維持して無視される
    db.playlist_add(pl.id, &vref("aaaaaaaaaa1", "A")).unwrap();
    let items = db.playlist_items(pl.id).unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].video_id, "aaaaaaaaaa1");
    assert_eq!(items[1].video_id, "bbbbbbbbbb2");
    assert_eq!(items[0].position, 0);
    // 一覧の item_count も 2
    assert_eq!(db.playlist_list().unwrap()[0].item_count, 2);
    // 途中削除でも残りの順序は変わらない
    db.playlist_remove(pl.id, "aaaaaaaaaa1").unwrap();
    let items = db.playlist_items(pl.id).unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].video_id, "bbbbbbbbbb2");
    // リネームと削除
    db.playlist_rename(pl.id, "朝の選曲").unwrap();
    assert_eq!(db.playlist_list().unwrap()[0].name, "朝の選曲");
    db.playlist_delete(pl.id).unwrap();
    assert!(db.playlist_items(pl.id).unwrap().is_empty());
    assert!(db.playlist_list().unwrap().is_empty());
    // 存在しない id への rename / add は NotFound
    assert!(matches!(
        db.playlist_rename(999, "x"),
        Err(DbError::NotFound)
    ));
    assert!(matches!(
        db.playlist_add(999, &vref("cccccccccc3", "C")),
        Err(DbError::NotFound)
    ));
}

/// `playlist_items_page` の分割取得（FR-25、仕様決定 AR）。
/// `after_position` より後ろを `limit` 件返し、両省略で全件。
#[test]
fn playlist_items_page_ranges() {
    let db = Db::connect_in_memory().unwrap();
    let pl = db.playlist_create("分割").unwrap();
    db.playlist_add_many(
        pl.id,
        &[
            vref("aaaaaaaaaa1", "A"),
            vref("bbbbbbbbbb2", "B"),
            vref("cccccccccc3", "C"),
            vref("dddddddddd4", "D"),
        ],
    )
    .unwrap();
    let ids = |rows: &[PlaylistEntry]| {
        rows.iter()
            .map(|i| i.video_id.clone())
            .collect::<Vec<_>>()
    };
    // 先頭 2 件
    let p1 = db.playlist_items_page(pl.id, None, Some(2)).unwrap();
    assert_eq!(ids(&p1), ["aaaaaaaaaa1", "bbbbbbbbbb2"]);
    // 末尾行の position から続き
    let p2 = db
        .playlist_items_page(pl.id, Some(p1[1].position), Some(2))
        .unwrap();
    assert_eq!(ids(&p2), ["cccccccccc3", "dddddddddd4"]);
    // 末尾以降は空、重複も無い
    let p3 = db
        .playlist_items_page(pl.id, Some(p2[1].position), Some(2))
        .unwrap();
    assert!(p3.is_empty());
    // 引数省略は従来の全件取得と同じ
    assert_eq!(db.playlist_items(pl.id).unwrap().len(), 4);
    let tail = db.playlist_items_page(pl.id, Some(1), None).unwrap();
    assert_eq!(ids(&tail), ["cccccccccc3", "dddddddddd4"]);
}

/// DB-LD-07: 項目順の一括書き換え（FR-11、仕様決定 T）。
/// 渡した順に position が振り直され、未登録プレイリストは NotFound。
/// 同一集合の検証はこの書き込みトランザクション内で行い、集合不一致・
/// 重複・件数違いは `MismatchedItems` で変更を破棄する。
#[test]
fn playlist_reorder_renumbers_positions() {
    let db = Db::connect_in_memory().unwrap();
    let pl = db.playlist_create("並べ替え").unwrap();
    db.playlist_add_many(
        pl.id,
        &[
            vref("aaaaaaaaaa1", "A"),
            vref("bbbbbbbbbb2", "B"),
            vref("cccccccccc3", "C"),
        ],
    )
    .unwrap();
    db.playlist_reorder(
        pl.id,
        &[
            "cccccccccc3".to_string(),
            "aaaaaaaaaa1".to_string(),
            "bbbbbbbbbb2".to_string(),
        ],
    )
    .unwrap();
    let items = db.playlist_items(pl.id).unwrap();
    assert_eq!(
        items
            .iter()
            .map(|i| i.video_id.as_str())
            .collect::<Vec<_>>(),
        ["cccccccccc3", "aaaaaaaaaa1", "bbbbbbbbbb2"]
    );
    assert_eq!(
        items.iter().map(|i| i.position).collect::<Vec<_>>(),
        [0, 1, 2]
    );
    assert!(matches!(
        db.playlist_reorder(999, &[]),
        Err(DbError::NotFound)
    ));
    // 集合不一致・重複・件数違いは MismatchedItems で既存順を破壊しない
    for bad in [
        vec!["aaaaaaaaaa1", "bbbbbbbbbb2", "dddddddddd4"], // 未登録項目
        vec!["aaaaaaaaaa1", "bbbbbbbbbb2", "bbbbbbbbbb2"], // 重複
        vec!["aaaaaaaaaa1", "bbbbbbbbbb2"],                // 件数不足
    ] {
        let ids: Vec<String> = bad.iter().map(|s| s.to_string()).collect();
        assert!(matches!(
            db.playlist_reorder(pl.id, &ids),
            Err(DbError::MismatchedItems)
        ));
    }
    assert_eq!(
        db.playlist_items(pl.id)
            .unwrap()
            .iter()
            .map(|i| i.video_id.as_str())
            .collect::<Vec<_>>(),
        ["cccccccccc3", "aaaaaaaaaa1", "bbbbbbbbbb2"]
    );
}

/// DB-LD-08: 取り込み用の一括追加（FR-10、仕様決定 R）。
/// 1 トランザクションで末尾追加し、重複は位置を維持して無視。
/// `published_at` は取り込み時点では未取得のまま（ソート時は末尾扱い）。
#[test]
fn playlist_add_many_appends_and_dedupes() {
    let db = Db::connect_in_memory().unwrap();
    let pl = db.playlist_create("取り込み").unwrap();
    db.playlist_add_many(
        pl.id,
        &[
            vref("aaaaaaaaaa1", "A"),
            vref("bbbbbbbbbb2", "B"),
            vref("cccccccccc3", "C"),
        ],
    )
    .unwrap();
    assert_eq!(db.playlist_items(pl.id).unwrap().len(), 3);
    // 途中に既存項目が混ざった再投入: 新規分だけ末尾追加、既存は位置維持
    db.playlist_add_many(pl.id, &[vref("bbbbbbbbbb2", "B"), vref("dddddddddd4", "D")])
        .unwrap();
    let items = db.playlist_items(pl.id).unwrap();
    assert_eq!(items.len(), 4);
    assert_eq!(
        items
            .iter()
            .map(|i| i.video_id.as_str())
            .collect::<Vec<_>>(),
        ["aaaaaaaaaa1", "bbbbbbbbbb2", "cccccccccc3", "dddddddddd4"]
    );
    // published_at は未投入のまま None
    assert!(items.iter().all(|i| i.published_at.is_none()));
    assert_eq!(db.playlist_list().unwrap()[0].item_count, 4);
    assert!(matches!(
        db.playlist_add_many(999, &[]),
        Err(DbError::NotFound)
    ));
}

/// DB-LD-09: 項目順の一括反転（FR-11、仕様決定 Z）。
/// 現在の項目が逆順で保存され、未登録プレイリストは NotFound。
/// 反転は冪等で、2 回適用すると元の順に戻る。
#[test]
fn playlist_reverse_flips_positions() {
    let db = Db::connect_in_memory().unwrap();
    let pl = db.playlist_create("反転").unwrap();
    db.playlist_add_many(
        pl.id,
        &[
            vref("aaaaaaaaaa1", "A"),
            vref("bbbbbbbbbb2", "B"),
            vref("cccccccccc3", "C"),
        ],
    )
    .unwrap();
    db.playlist_reverse(pl.id).unwrap();
    let items = db.playlist_items(pl.id).unwrap();
    assert_eq!(
        items
            .iter()
            .map(|i| i.video_id.as_str())
            .collect::<Vec<_>>(),
        ["cccccccccc3", "bbbbbbbbbb2", "aaaaaaaaaa1"]
    );
    assert_eq!(
        items.iter().map(|i| i.position).collect::<Vec<_>>(),
        [0, 1, 2]
    );
    db.playlist_reverse(pl.id).unwrap();
    assert_eq!(
        db.playlist_items(pl.id)
            .unwrap()
            .iter()
            .map(|i| i.video_id.as_str())
            .collect::<Vec<_>>(),
        ["aaaaaaaaaa1", "bbbbbbbbbb2", "cccccccccc3"]
    );
    // 空のプレイリストは成功のまま何もしない
    let empty = db.playlist_create("空").unwrap();
    db.playlist_reverse(empty.id).unwrap();
    assert!(db.playlist_items(empty.id).unwrap().is_empty());
    assert!(matches!(db.playlist_reverse(999), Err(DbError::NotFound)));
}

/// DB-LD-04: 履歴一覧は新しい順、history_remove で個別削除（FR-7、仕様決定 I）。
#[test]
fn history_list_and_remove() {
    let db = Db::connect_in_memory().unwrap();
    for (i, v) in ["a1", "a2", "a3"].iter().enumerate() {
        db.history_update_progress(v, "t", i as f64, Some(60), false)
            .unwrap();
    }
    let list = db.history_list(500).unwrap();
    assert_eq!(list.len(), 3);
    // last_watched_at は datetime('now') 同時刻でも rowid 降順で
    // 決定的になる（同一時刻の順序不定を防ぐ）
    assert_eq!(list[0].video_id, "a3");
    assert_eq!(list[1].video_id, "a2");
    assert_eq!(list[2].video_id, "a1");
    assert!(list.iter().all(|h| !h.completed));
    db.history_remove("a2").unwrap();
    let list = db.history_list(500).unwrap();
    assert_eq!(list.len(), 2);
    assert!(!list.iter().any(|h| h.video_id == "a2"));
    // limit が効く
    assert_eq!(db.history_list(1).unwrap().len(), 1);
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
        db.feed_list_filtered(&FeedFilter::default(), show_shorts, FEED_LIST_LIMIT, |_| true)
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
    let ids_of = |rows: &[(String, Option<String>)]| {
        rows.iter().map(|r| r.0.clone()).collect::<Vec<_>>()
    };

    // 1 ページ目：同時刻は video_id 昇順
    let p1 = page(None, 2);
    assert_eq!(ids_of(&p1), vec!["aaaa0000001", "bbbb0000001"]);

    // 2 ページ目：カーソル行自体は含まれず、NULL 行は末尾
    let p2 = page(Some(cur(Some("2026-10-08T00:00:00+00:00"), "bbbb0000001")), 2);
    assert_eq!(ids_of(&p2), vec!["cccc0000001", "dddd0000001"]);
    assert_eq!(p2[1].1, None);

    // 3 ページ目：NULL 群の中では video_id 昇順で遡る（末尾なら空）
    let p3 = page(Some(cur(None, "dddd0000001")), 2);
    assert!(p3.is_empty());

    // 同時刻の途中行カーソル：次の同時刻行から続く
    let mid = page(Some(cur(Some("2026-10-08T00:00:00+00:00"), "aaaa0000001")), 10);
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
    assert_eq!(
        r.published_at.as_deref(),
        Some("2026-10-07T00:00:00+00:00")
    );

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

// ---------------------------------------------------------------------------
// docs↔コード整合の機械チェック: §8 DDL と適用後スキーマの照合
// （Python 側は scripts/check_docs_consistency.py、計画書 CL-1a）。
// ---------------------------------------------------------------------------

/// `--` 行コメントを文字列リテラル外だけ取り除く。
fn strip_sql_comments(sql: &str) -> String {
    let mut out = String::with_capacity(sql.len());
    let mut chars = sql.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\'' {
            out.push(c);
            while let Some(inner) = chars.next() {
                out.push(inner);
                if inner == '\'' {
                    // '' はリテラル内のエスケープなので読み続ける
                    if chars.peek() == Some(&'\'') {
                        out.push(chars.next().unwrap());
                        continue;
                    }
                    break;
                }
            }
            continue;
        }
        if c == '-' && chars.peek() == Some(&'-') {
            for inner in chars.by_ref() {
                if inner == '\n' {
                    out.push(inner);
                    break;
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

/// 比較用の正規化: コメント除去、引用符外の小文字化、空白畳み込み、
/// `if not exists` 除去、`()` `,` `;` 前後の空白除去、末尾 `;` 除去。
fn normalize_sql(sql: &str) -> String {
    let stripped = strip_sql_comments(sql);
    let mut out = String::with_capacity(stripped.len());
    let mut prev_ws = false;
    let mut chars = stripped.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\'' {
            // 文字列リテラルは大小・空白ともそのまま残す
            out.push(c);
            while let Some(inner) = chars.next() {
                out.push(inner);
                if inner == '\'' {
                    if chars.peek() == Some(&'\'') {
                        out.push(chars.next().unwrap());
                        continue;
                    }
                    break;
                }
            }
            prev_ws = false;
            continue;
        }
        if c.is_whitespace() {
            prev_ws = true;
            continue;
        }
        if prev_ws {
            // 開き `(` の直後と `()` `,` `;` の直前の空白は出さない
            if !"(),;".contains(c) && !out.ends_with('(') {
                out.push(' ');
            }
            prev_ws = false;
        }
        out.extend(c.to_lowercase());
    }
    // sqlite は格納時に if not exists を落とすが、文書側に書かれた場合にも耐える
    let out = out.replace("if not exists ", "");
    out.trim_end_matches(';').trim_end().to_string()
}

/// 正規化済み SQL の `(...)` 本体を最上位カンマで分割する。
/// `PRIMARY KEY (a, b)` のような括弧内と '...' 内のカンマは区切りにしない。
fn split_top_level_commas(body: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut cur = String::new();
    let mut depth = 0i32;
    let mut in_quote = false;
    let mut chars = body.chars().peekable();
    while let Some(c) = chars.next() {
        if in_quote {
            cur.push(c);
            if c == '\'' {
                if chars.peek() == Some(&'\'') {
                    cur.push(chars.next().unwrap());
                    continue;
                }
                in_quote = false;
            }
            continue;
        }
        match c {
            '\'' => {
                in_quote = true;
                cur.push(c);
            }
            '(' => {
                depth += 1;
                cur.push(c);
            }
            ')' => {
                depth -= 1;
                cur.push(c);
            }
            ',' if depth == 0 => {
                items.push(cur.trim().to_string());
                cur.clear();
            }
            _ => cur.push(c),
        }
    }
    if !cur.trim().is_empty() {
        items.push(cur.trim().to_string());
    }
    items
}

/// オブジェクト単位の比較用文字列に変換する。
/// `CREATE [VIRTUAL] TABLE` はヘッダ＋列定義の集合として比較し、
/// それ以外（INDEX / TRIGGER など）は正規化文全体を比較する。
fn normalize_for_compare(sql: &str) -> String {
    let n = normalize_sql(sql);
    if n.starts_with("create table ") || n.starts_with("create virtual table ") {
        if let Some(open) = n.find('(') {
            let head = n[..open].trim_end();
            let close = n.rfind(')').unwrap_or(n.len());
            let mut items = split_top_level_commas(&n[open + 1..close]);
            items.sort();
            return format!("{head}({})", items.join(","));
        }
    }
    n
}

/// CREATE 文からオブジェクト名を取る。
fn ddl_object_name(stmt: &str) -> Option<String> {
    let re = regex::Regex::new(
        r#"(?i)^\s*create\s+(?:virtual\s+)?(?:table|index|unique\s+index|trigger|view)\s+(?:if\s+not\s+exists\s+)?[\"'`]?(\w+)"#,
    )
    .unwrap();
    re.captures(stmt).map(|c| c[1].to_string())
}

/// 文書側の SQL 断片から CREATE 文を (名前, 文) で切り出す。
/// トリガーの BEGIN...END 内の `;` は文の区切りにしない。
fn create_statements(sql: &str) -> Vec<(String, String)> {
    // トークン化（空白で区切られる語、'...' リテラル、() ; , の区切り文字）
    let mut spans: Vec<(usize, usize)> = Vec::new();
    let mut chars = sql.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if c.is_whitespace() {
            continue;
        }
        if c == '\'' {
            let mut end = i + c.len_utf8();
            while let Some((j, d)) = chars.next() {
                end = j + d.len_utf8();
                if d == '\'' {
                    if chars.peek().map(|&(_, p)| p) == Some('\'') {
                        let (j2, d2) = chars.next().unwrap();
                        end = j2 + d2.len_utf8();
                        continue;
                    }
                    break;
                }
            }
            spans.push((i, end));
            continue;
        }
        if "();,".contains(c) {
            spans.push((i, i + c.len_utf8()));
            continue;
        }
        let mut end = i + c.len_utf8();
        while let Some(&(j, d)) = chars.peek() {
            if d.is_whitespace() || "();,'".contains(d) {
                break;
            }
            chars.next();
            end = j + d.len_utf8();
        }
        spans.push((i, end));
    }

    let mut out = Vec::new();
    let mut i = 0;
    while i < spans.len() {
        let (s, e) = spans[i];
        if !sql[s..e].eq_ignore_ascii_case("create") {
            i += 1;
            continue;
        }
        // `;`（括弧・BEGIN...END の外側に限る）までを 1 文とする
        let mut depth = 0i32;
        let mut begin_depth = 0i32;
        let mut end = e;
        let mut j = i;
        while j < spans.len() {
            let (s2, e2) = spans[j];
            end = e2;
            let t = &sql[s2..e2];
            if t == "(" {
                depth += 1;
            } else if t == ")" {
                depth -= 1;
            } else if depth == 0 && t.eq_ignore_ascii_case("begin") {
                begin_depth += 1;
            } else if depth == 0 && t.eq_ignore_ascii_case("end") {
                begin_depth -= 1;
            } else if t == ";" && depth == 0 && begin_depth <= 0 {
                break;
            }
            j += 1;
        }
        if let Some(name) = ddl_object_name(&sql[s..end]) {
            out.push((name, sql[s..end].to_string()));
        }
        i = j + 1;
    }
    out
}

/// design.md の「データベース設計」節にある ```sql ブロックを連結して返す。
fn design_ddl(design: &str) -> String {
    let lines: Vec<&str> = design.lines().collect();
    let mut start = None;
    let mut level = 0usize;
    for (i, l) in lines.iter().enumerate() {
        let heading = l.trim_start_matches('#');
        if l.starts_with('#') && heading.starts_with(' ') {
            let lv = l.len() - heading.len();
            if let Some(s) = start {
                if lv <= level {
                    start = Some(s);
                    level = lv;
                    let _ = i;
                    break;
                }
            } else if heading[1..].contains("データベース設計") {
                start = Some(i);
                level = lv;
            }
        }
    }
    let start = start.expect("design.md に「データベース設計」節が無い");
    let mut end = lines.len();
    for (i, l) in lines.iter().enumerate().skip(start + 1) {
        if l.starts_with('#') {
            let heading = l.trim_start_matches('#');
            let lv = l.len() - heading.len();
            if lv <= level {
                end = i;
                break;
            }
        }
    }
    let mut ddl = String::new();
    let mut in_sql = false;
    for l in &lines[start..end] {
        let t = l.trim();
        if !in_sql && t.starts_with("```") && t.contains("sql") {
            in_sql = true;
            continue;
        }
        if in_sql && t.starts_with("```") {
            in_sql = false;
            continue;
        }
        if in_sql {
            ddl.push_str(l);
            ddl.push('\n');
        }
    }
    ddl
}

/// 設計書 §8 の DDL と、マイグレーション適用後の sqlite_master を照合する
/// （監査対応の機械チェック: scripts/check_docs_consistency.py と対）。
/// `connect_in_memory` は `connect` と同じ migrate 経路を通るため、
/// schema_migrations のブートストラップや ALTER による文の書き換えも
/// そのまま拾える。
/// 文書側は列を意味のまとまりで書くことがあるため（ALTER 追加列を
/// 末尾に置く sqlite の書き換えと常に一致するとは限らない）、
/// CREATE TABLE は列の集合として比較する。
#[test]
fn ddl_matches_design_section8() {
    let db = Db::connect_in_memory().unwrap();
    let conn = db.lock().unwrap();
    let mut stmt = conn
        .prepare("SELECT type, name, sql FROM sqlite_master WHERE sql IS NOT NULL")
        .unwrap();
    let rows: Vec<(String, String, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();

    // FTS5 のシャドウテーブル（<ft>_data 等）と sqlite_ 内部オブジェクトは
    // SQLite の内部生成物なので照合対象から除く
    let virtual_tables: Vec<String> = rows
        .iter()
        .filter(|(ty, _, sql)| ty == "table" && sql.to_lowercase().contains("using fts"))
        .map(|(_, name, _)| name.clone())
        .collect();
    let mut actual: std::collections::BTreeMap<String, String> = std::collections::BTreeMap::new();
    for (_, name, sql) in &rows {
        if name.starts_with("sqlite_") {
            continue;
        }
        if virtual_tables
            .iter()
            .any(|vt| name.starts_with(&format!("{vt}_")))
        {
            continue;
        }
        actual.insert(name.clone(), normalize_for_compare(sql));
    }

    let design = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs/design.md"),
    )
    .expect("docs/design.md が読めない");
    let mut expected: std::collections::BTreeMap<String, String> =
        std::collections::BTreeMap::new();
    for (name, stmt_text) in create_statements(&design_ddl(&design)) {
        expected.insert(name, normalize_for_compare(&stmt_text));
    }

    let mut diffs: Vec<String> = Vec::new();
    for (name, actual_sql) in &actual {
        match expected.get(name) {
            None => diffs.push(format!("{name}: 実スキーマにあって §8 に無い")),
            Some(expected_sql) if expected_sql != actual_sql => diffs.push(format!(
                "{name}: §8 と実スキーマで文が違う\n  doc: {expected_sql}\n  sql: {actual_sql}"
            )),
            _ => {}
        }
    }
    for name in expected.keys() {
        if !actual.contains_key(name) {
            diffs.push(format!("{name}: §8 にあって実スキーマに無い"));
        }
    }
    assert!(
        diffs.is_empty(),
        "§8 DDL と適用後スキーマのずれ:\n{}",
        diffs.join("\n")
    );
}
