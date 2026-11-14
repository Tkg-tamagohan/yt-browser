use crate::db::*;

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

/// DB-CH-03: highlight 種別の NG フィルタを登録・一覧・削除できる
/// （FR-69、仕様決定 CT）。v21 の CHECK 拡張で受理される値域を確認する。
#[test]
fn filter_highlight_kind_roundtrip() {
    let db = Db::connect_in_memory().unwrap();
    let f = db.filter_add("chat_text", "highlight", "初見").unwrap();
    assert_eq!(f.kind, "highlight");
    assert!(f.enabled);
    assert_eq!(db.filter_list().unwrap().len(), 1);
    assert!(db.filter_remove(f.id).unwrap());
    assert!(db.filter_list().unwrap().is_empty());
}
