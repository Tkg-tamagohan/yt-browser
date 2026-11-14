use super::vref;
use crate::db::*;

/// DB-BM-01: ブックマークの保存と一覧の往復（FR-49、仕様決定 BZ）。
/// 必須項目（video_id / position_sec / label / created_at）が保持され、
/// 表示用タイトルは videos 台帳 → watch_history の順で解決する
/// （暫定解釈: video_meta_lookup と同じ規則。台帳・履歴のどちらにも
/// 無い動画は title=None）。
#[test]
fn bookmark_roundtrip_and_title_resolution() {
    let db = Db::connect_in_memory().unwrap();
    // videos 台帳だけにある動画
    db.favorite_add(&vref("AAAAAAAAAAA", "台帳タイトル"))
        .unwrap();
    // watch_history だけにある動画
    db.history_upsert("BBBBBBBBBBB").unwrap();
    db.history_update_progress("BBBBBBBBBBB", "履歴タイトル", 0.0, None, false)
        .unwrap();
    // 台帳と履歴の両方にある動画（台帳が優先）
    db.favorite_add(&vref("CCCCCCCCCCC", "台帳側")).unwrap();
    db.history_upsert("CCCCCCCCCCC").unwrap();
    db.history_update_progress("CCCCCCCCCCC", "履歴側", 0.0, None, false)
        .unwrap();

    let a = db
        .bookmark_insert("AAAAAAAAAAA", 754, "12:34")
        .unwrap()
        .expect("新規挿入は行を返す");
    assert_eq!(a.video_id, "AAAAAAAAAAA");
    assert_eq!(a.position_sec, 754);
    assert_eq!(a.label, "12:34");
    assert!(!a.created_at.is_empty());
    assert_eq!(a.title.as_deref(), Some("台帳タイトル"));

    let b = db
        .bookmark_insert("BBBBBBBBBBB", 60, "1:00")
        .unwrap()
        .unwrap();
    assert_eq!(b.title.as_deref(), Some("履歴タイトル"));

    let c = db
        .bookmark_insert("CCCCCCCCCCC", 30, "0:30")
        .unwrap()
        .unwrap();
    assert_eq!(c.title.as_deref(), Some("台帳側"));

    // 台帳・履歴のどちらにも無い動画は title=None
    let d = db
        .bookmark_insert("DDDDDDDDDDD", 10, "0:10")
        .unwrap()
        .unwrap();
    assert_eq!(d.title, None);

    let list = db.bookmark_list().unwrap();
    assert_eq!(list.len(), 4);
}

/// DB-BM-02: 同一動画・同一位置の重複保存の抑止（暫定解釈:
/// UNIQUE (video_id, position_sec)）。重複時は None を返し行数と
/// 既存ラベルを維持する。別位置・別動画は別行として保存できる。
#[test]
fn bookmark_insert_dedupes_same_position() {
    let db = Db::connect_in_memory().unwrap();
    let first = db
        .bookmark_insert("AAAAAAAAAAA", 100, "編集済みラベル")
        .unwrap()
        .unwrap();
    // 同位置の再保存は抑止され、既存行（ラベル）はそのまま
    assert!(db
        .bookmark_insert("AAAAAAAAAAA", 100, "1:40")
        .unwrap()
        .is_none());
    let list = db.bookmark_list().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].label, "編集済みラベル");
    assert_eq!(list[0].id, first.id);
    // 別位置・別動画は別行
    assert!(db
        .bookmark_insert("AAAAAAAAAAA", 101, "1:41")
        .unwrap()
        .is_some());
    assert!(db
        .bookmark_insert("BBBBBBBBBBB", 100, "1:40")
        .unwrap()
        .is_some());
    assert_eq!(db.bookmark_list().unwrap().len(), 3);
}

/// DB-BM-03: 一覧の並びは保存の新しい順（暫定解釈: created_at 降順、
/// 同時刻は id 降順）。created_at は秒精度で同時刻になりうるため、
/// 直近に挿入した行が先頭に来ることも併せて確認する。
#[test]
fn bookmark_list_orders_newest_first() {
    let db = Db::connect_in_memory().unwrap();
    for (i, pos) in [10_i64, 20, 30].iter().enumerate() {
        db.bookmark_insert("AAAAAAAAAAA", *pos, &format!("mark{}", i + 1))
            .unwrap();
    }
    let list = db.bookmark_list().unwrap();
    assert_eq!(
        list.iter().map(|b| b.position_sec).collect::<Vec<_>>(),
        [30, 20, 10]
    );
}

/// DB-BM-04: 個別削除（FR-49）。id 指定で対象行だけを消し、
/// 未登録の id でも正常終了する（冪等）。
#[test]
fn bookmark_remove_deletes_single_row() {
    let db = Db::connect_in_memory().unwrap();
    let a = db.bookmark_insert("AAAAAAAAAAA", 10, "a").unwrap().unwrap();
    let b = db.bookmark_insert("AAAAAAAAAAA", 20, "b").unwrap().unwrap();
    assert!(db.bookmark_remove(a.id).unwrap());
    let list = db.bookmark_list().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, b.id);
    // 冪等: もう一度消してもエラーにならない
    assert!(!db.bookmark_remove(a.id).unwrap());
    assert!(!db.bookmark_remove(999).unwrap());
}

/// DB-BM-05: ラベルの後付け編集（FR-49）。label だけが更新され、
/// 位置などの他列は保持される。未登録 id は NotFound。
#[test]
fn bookmark_update_label() {
    let db = Db::connect_in_memory().unwrap();
    let a = db
        .bookmark_insert("AAAAAAAAAAA", 754, "12:34")
        .unwrap()
        .unwrap();
    db.bookmark_update_label(a.id, "副反応の説明").unwrap();
    let cur = db.bookmark_get(a.id).unwrap().unwrap();
    assert_eq!(cur.label, "副反応の説明");
    assert_eq!(cur.position_sec, 754);
    assert_eq!(cur.created_at, a.created_at);
    assert!(matches!(
        db.bookmark_update_label(999, "x"),
        Err(DbError::NotFound)
    ));
    assert!(db.bookmark_get(999).unwrap().is_none());
}

/// DB-BM-06: 保存データは再起動（再接続）後も残る（FR-49 の永続化）。
#[test]
fn bookmark_persists_across_reconnect() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bm.db");
    {
        let db = Db::connect(&path).unwrap();
        db.bookmark_insert("AAAAAAAAAAA", 754, "12:34").unwrap();
    }
    let db = Db::connect(&path).unwrap();
    let list = db.bookmark_list().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].video_id, "AAAAAAAAAAA");
    assert_eq!(list[0].position_sec, 754);
    assert_eq!(list[0].label, "12:34");
}
