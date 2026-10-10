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
