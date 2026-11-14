use crate::db::*;

/// DB-LD-28: カテゴリ一覧の並べ替え（FR-63、仕様決定 CN）。
/// 新規作成は末尾（MAX(sort_order)+1）に入り、並べ替え結果は
/// `category_list` の順序に反映される。集合の不一致（欠落・余剰・
/// 重複・未登録）は `MismatchedItems` で拒否して順序を変えない
/// （`playlist_reorder_lists` と同じ検証規則）。
#[test]
fn category_reorder_updates_list_order() {
    let db = Db::connect_in_memory().unwrap();
    let a = db.category_create("A").unwrap();
    let b = db.category_create("B").unwrap();
    let c = db.category_create("C").unwrap();
    assert_eq!([a.sort_order, b.sort_order, c.sort_order], [0, 1, 2]);
    let names = |db: &Db| {
        db.category_list()
            .unwrap()
            .into_iter()
            .map(|p| p.name)
            .collect::<Vec<_>>()
    };
    db.category_reorder(&[c.id, a.id, b.id]).unwrap();
    assert_eq!(names(&db), ["C", "A", "B"]);
    assert_eq!(
        db.category_list()
            .unwrap()
            .iter()
            .map(|p| p.sort_order)
            .collect::<Vec<_>>(),
        [0, 1, 2]
    );
    for bad in [
        vec![c.id, a.id],            // 欠落
        vec![c.id, a.id, b.id, 999], // 余剰
        vec![c.id, a.id, a.id],      // 重複
        vec![c.id, a.id, 999],       // 未登録の id
    ] {
        assert!(matches!(
            db.category_reorder(&bad),
            Err(DbError::MismatchedItems)
        ));
    }
    assert_eq!(names(&db), ["C", "A", "B"]);
    // 並べ替え後の新規作成も末尾に入る
    let d = db.category_create("D").unwrap();
    assert_eq!(d.sort_order, 3);
    assert_eq!(names(&db), ["C", "A", "B", "D"]);
    // 空集合の並べ替え（カテゴリ 0 件）は成功のまま何もしない
    let empty = Db::connect_in_memory().unwrap();
    empty.category_reorder(&[]).unwrap();
}
