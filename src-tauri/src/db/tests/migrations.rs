use crate::db::*;

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
