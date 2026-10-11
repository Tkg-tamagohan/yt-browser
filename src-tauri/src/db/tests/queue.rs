use crate::db::*;
use crate::model::QueueItem;

fn item(video_id: &str, title: &str) -> QueueItem {
    QueueItem {
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
        db.queue_save(
            &[item("a1234567890", "A"), item("b1234567890", "B")],
            1,
        )
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
