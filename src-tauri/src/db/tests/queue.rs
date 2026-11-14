use crate::db::*;
use crate::model::QueueItem;

fn item(video_id: &str, title: &str) -> QueueItem {
    QueueItem {
        uid: 0,
        video_id: video_id.to_string(),
        title: title.to_string(),
        channel_title: Some("テストCH".to_string()),
        thumbnail_url: Some("https://i.ytimg.com/vi/x.jpg".to_string()),
    }
}

/// DB-Q-01: 一時キューの保存と読み出し（FR-30、仕様決定 AY）。
/// 項目の順序・同一動画の重複・表示メタ・現在位置がそのまま往復する。
#[test]
fn queue_save_load_round_trip() {
    let db = Db::connect_in_memory().unwrap();
    // 未保存は空
    let empty = db.queue_load().unwrap();
    assert!(empty.items.is_empty());
    assert_eq!(empty.current_index, 0);

    let items = vec![
        item("dQw4w9WgXcQ", "A"),
        item("dQw4w9WgXcQ", "A"), // 同一動画の重複は許容
        QueueItem {
            uid: 0,
            video_id: "abc12345678".to_string(),
            title: "B".to_string(),
            channel_title: None,
            thumbnail_url: None,
        },
    ];
    db.queue_save(&items, 1).unwrap();
    let snap = db.queue_load().unwrap();
    assert_eq!(snap.items.len(), 3);
    assert_eq!(snap.items[0].video_id, "dQw4w9WgXcQ");
    assert_eq!(snap.items[1].video_id, "dQw4w9WgXcQ");
    assert_eq!(snap.items[1].title, "A");
    assert_eq!(snap.items[1].channel_title.as_deref(), Some("テストCH"));
    assert_eq!(snap.items[2].video_id, "abc12345678");
    assert_eq!(snap.items[2].channel_title, None);
    assert_eq!(snap.items[2].thumbnail_url, None);
    assert_eq!(snap.current_index, 1);
}

/// DB-Q-02: 全置換の書き換えと消滅。save は旧項目を残さず置き換え、
/// 空での保存は項目を消して位置を 0 へ戻す（キュー消滅と同じ結果）。
#[test]
fn queue_save_replaces_and_clears() {
    let db = Db::connect_in_memory().unwrap();
    db.queue_save(&[item("a1234567890", "A"), item("b1234567890", "B")], 1)
        .unwrap();
    // 部分置き換えではなく全置換: 別集合で上書きすると旧項目は残らない
    db.queue_save(&[item("c1234567890", "C")], 0).unwrap();
    let snap = db.queue_load().unwrap();
    assert_eq!(snap.items.len(), 1);
    assert_eq!(snap.items[0].video_id, "c1234567890");
    assert_eq!(snap.current_index, 0);
    // 空保存は項目を消し位置を 0 へ戻す
    db.queue_save(&[], 0).unwrap();
    let snap = db.queue_load().unwrap();
    assert!(snap.items.is_empty());
    assert_eq!(snap.current_index, 0);
}

/// DB-Q-03: DB 再接続をまたぐ永続化。アプリ再起動相当でキューと
/// 現在位置が復元される（時間を置いた継続視聴の基盤）。
#[test]
fn queue_persists_across_reconnect() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    {
        let db = Db::connect(&path).unwrap();
        db.queue_save(&[item("a1234567890", "A"), item("b1234567890", "B")], 1)
            .unwrap();
    }
    let db = Db::connect(&path).unwrap();
    let snap = db.queue_load().unwrap();
    assert_eq!(snap.items.len(), 2);
    assert_eq!(snap.items[1].video_id, "b1234567890");
    assert_eq!(snap.items[1].title, "B");
    assert_eq!(snap.current_index, 1);
}

/// DB-Q-04: 現在位置の収め。末尾を超える index は末尾へ、空キューは 0 へ。
#[test]
fn queue_index_is_clamped() {
    let db = Db::connect_in_memory().unwrap();
    db.queue_save(&[item("a1234567890", "A"), item("b1234567890", "B")], 99)
        .unwrap();
    assert_eq!(db.queue_load().unwrap().current_index, 1);
    db.queue_save(&[], 5).unwrap();
    assert_eq!(db.queue_load().unwrap().current_index, 0);
}

/// DB-Q-06: キュー項目の uid の往復（BG-10、仕様決定 BK）。
/// 保存した uid が読み出しでもそのまま返り、uid を持たない旧形式の
/// 行（DEFAULT 0）は 0 で読み出せる（復元側で新規払い出しへ振り替える）。
#[test]
fn queue_uid_round_trip() {
    let db = Db::connect_in_memory().unwrap();
    let items = vec![
        QueueItem {
            uid: 11,
            ..item("a1234567890", "A")
        },
        QueueItem {
            uid: 12,
            ..item("a1234567890", "A")
        },
    ];
    db.queue_save(&items, 0).unwrap();
    let snap = db.queue_load().unwrap();
    // 同一動画でも uid が別々に保存・復元される
    assert_eq!(snap.items[0].uid, 11);
    assert_eq!(snap.items[1].uid, 12);
    assert_eq!(snap.items[0].video_id, "a1234567890");
    assert_eq!(snap.items[1].video_id, "a1234567890");

    // uid 列追加前の行（DEFAULT 0 で入った行）は 0 で読み出せる
    {
        let conn = db.lock().unwrap();
        conn.execute(
            "INSERT INTO queue_items (position, video_id, title)
             VALUES (2, 'legacy00000', '旧行')",
            [],
        )
        .unwrap();
    }
    let snap = db.queue_load().unwrap();
    assert_eq!(snap.items[2].uid, 0);
    assert_eq!(snap.items[2].video_id, "legacy00000");
}

/// DB-Q-05: `video_meta_lookup` の補完順（FR-29、仕様決定 AX）。
/// `videos` 台帳があればそちら、無ければ `watch_history`、
/// どちらにも無ければ None。台帳側は空の channel_id を NULL 扱いする。
#[test]
fn video_meta_lookup_precedence() {
    let db = Db::connect_in_memory().unwrap();
    // どちらにも無い動画は None
    assert!(db.video_meta_lookup("notexists01").unwrap().is_none());

    // watch_history のみ: サムネイル無しのメタが返る
    // （channel_id/channel_title は現行コードの書き込み対象外の列のため
    //   生 SQL でシードする）
    {
        let conn = db.lock().unwrap();
        conn.execute(
            "INSERT INTO watch_history
               (video_id, title, channel_id, channel_title)
             VALUES ('histonly001', '履歴のみ', 'UCchan000000000000001', '履歴CH')",
            [],
        )
        .unwrap();
    }
    let m = db.video_meta_lookup("histonly001").unwrap().unwrap();
    assert_eq!(m.title, "履歴のみ");
    assert_eq!(m.channel_title.as_deref(), Some("履歴CH"));
    assert_eq!(m.thumbnail_url, None);

    // videos 台帳があると台帳側が優先される（サムネイルも返る）
    db.favorite_add(&super::vref("bothvideo01", "台帳の題名"))
        .unwrap();
    {
        let conn = db.lock().unwrap();
        conn.execute(
            "INSERT INTO watch_history (video_id, title)
             VALUES ('bothvideo01', '履歴の題名')",
            [],
        )
        .unwrap();
    }
    let m = db.video_meta_lookup("bothvideo01").unwrap().unwrap();
    assert_eq!(m.title, "台帳の題名");
    assert_eq!(m.channel_title.as_deref(), Some("テストCH"));
    assert_eq!(
        m.thumbnail_url.as_deref(),
        Some("https://i.ytimg.com/vi/x.jpg")
    );
}

/// DB-Q-07: プレイリストキューの保存位置の永続化（FR-48、仕様決定 BY）。
/// 保存→読み出し→更新→消去の往復と、一時キュー（id=1 行）との独立性、
/// プレイリストごとに 1 行で管理されることを確認する。
#[test]
fn queue_playlist_state_round_trip() {
    let db = Db::connect_in_memory().unwrap();
    let pl1 = db.playlist_create("PL1").unwrap();
    let pl2 = db.playlist_create("PL2").unwrap();
    // 未保存は空
    assert!(db.queue_playlist_load().unwrap().is_empty());

    // 一時キューの保存はプレイリストキュー側へ影響しない
    db.queue_save(&[item("a1234567890", "A")], 0).unwrap();
    assert!(db.queue_playlist_load().unwrap().is_empty());

    // 保存→読み出し（プレイリスト名が JOIN で取れる）
    db.queue_playlist_save(pl1.id, 3).unwrap();
    db.queue_playlist_save(pl2.id, 1).unwrap();
    let rows = db.queue_playlist_load().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].playlist_id, pl1.id);
    assert_eq!(rows[0].playlist_name, "PL1");
    assert_eq!(rows[0].current_index, 3);
    assert_eq!(rows[1].playlist_id, pl2.id);
    assert_eq!(rows[1].current_index, 1);
    // 一時キューの位置は変わらない（独立に動作する）
    assert_eq!(db.queue_load().unwrap().current_index, 0);

    // 再保存は同じ playlist_id の行を更新する（高々 1 行）
    db.queue_playlist_save(pl1.id, 5).unwrap();
    let rows = db.queue_playlist_load().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].playlist_id, pl1.id);
    assert_eq!(rows[0].current_index, 5);

    // 消去は対象行だけを消す
    db.queue_playlist_clear(pl1.id).unwrap();
    let rows = db.queue_playlist_load().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].playlist_id, pl2.id);
    // 未保存行の消去は空操作
    db.queue_playlist_clear(pl1.id).unwrap();
}

/// DB-Q-08: プレイリスト削除で保存行が消える（FR-48、仕様決定 BY）。
/// `playlist_id` の ON DELETE CASCADE で、プレイリスト削除に
/// 残存する保存行を残さない。
#[test]
fn queue_playlist_state_cascades_on_playlist_delete() {
    let db = Db::connect_in_memory().unwrap();
    let pl = db.playlist_create("PL").unwrap();
    db.queue_playlist_save(pl.id, 2).unwrap();
    assert_eq!(db.queue_playlist_load().unwrap().len(), 1);
    db.playlist_delete(pl.id).unwrap();
    assert!(db.queue_playlist_load().unwrap().is_empty());
}

/// DB-Q-09: v17 マイグレーション（FR-48、仕様決定 BY）。
/// 旧形式の単一行 queue_state（CHECK (id = 1)）の現在位置を引き継ぎ、
/// playlist_id 列とプレイリストキュー行を追加できる。
/// （v16 は Phase 59 の video_bookmarks で、queue_state に触れないため
/// 移行前状態は v16 適用後も変わらない）
#[test]
fn migrate_v17_preserves_queue_state() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE schema_migrations (
           version INTEGER PRIMARY KEY,
           applied_at TEXT NOT NULL DEFAULT (datetime('now'))
         );",
    )
    .unwrap();
    for m in migrations::MIGRATIONS.iter().filter(|m| m.version < 17) {
        conn.execute_batch(m.sql).unwrap();
        conn.execute(
            "INSERT INTO schema_migrations (version) VALUES (?1)",
            [m.version],
        )
        .unwrap();
    }
    // v16 状態でも queue_state は単一行（CHECK (id = 1)）
    conn.execute(
        "INSERT INTO queue_state (id, current_index) VALUES (1, 2)",
        [],
    )
    .unwrap();
    assert!(conn
        .execute(
            "INSERT INTO queue_state (id, current_index) VALUES (2, 0)",
            []
        )
        .is_err());
    for m in migrations::MIGRATIONS.iter().filter(|m| m.version == 17) {
        conn.execute_batch(m.sql).unwrap();
        conn.execute(
            "INSERT INTO schema_migrations (version) VALUES (?1)",
            [m.version],
        )
        .unwrap();
    }
    // 一時キュー行の位置が引き継がれる
    let idx: i64 = conn
        .query_row(
            "SELECT current_index FROM queue_state WHERE id = 1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(idx, 2);
    // プレイリストキュー行を追加できる（単一行制約は外れている）
    conn.execute("INSERT INTO playlists (id, name) VALUES (10, 'PL')", [])
        .unwrap();
    conn.execute(
        "INSERT INTO queue_state (playlist_id, current_index) VALUES (10, 4)",
        [],
    )
    .unwrap();
    let pid: Option<i64> = conn
        .query_row(
            "SELECT playlist_id FROM queue_state WHERE id = 1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(pid, None);
    // 同一 playlist_id の 2 行目は部分一意索引で弾かれる
    assert!(conn
        .execute(
            "INSERT INTO queue_state (playlist_id, current_index) VALUES (10, 0)",
            [],
        )
        .is_err());
}
