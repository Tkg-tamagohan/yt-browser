use crate::db::*;

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
