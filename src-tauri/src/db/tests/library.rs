use super::vref;
use crate::db::*;
use crate::model::PlaylistEntry;

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
    let ids = |rows: &[PlaylistEntry]| rows.iter().map(|i| i.video_id.clone()).collect::<Vec<_>>();
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

/// DB-LD-11: プレイリスト一覧の並べ替え（FR-35、仕様決定 BF）。
/// 新規作成は末尾（MAX(sort_order)+1）に入り、並べ替え結果は
/// `playlist_list` の順序に反映される。集合の不一致（欠落・余剰・重複）は
/// `MismatchedItems` で拒否して順序を変えない。
#[test]
fn playlist_reorder_lists_updates_list_order() {
    let db = Db::connect_in_memory().unwrap();
    let a = db.playlist_create("A").unwrap();
    let b = db.playlist_create("B").unwrap();
    let c = db.playlist_create("C").unwrap();
    assert_eq!([a.sort_order, b.sort_order, c.sort_order], [0, 1, 2]);
    let names = |db: &Db| {
        db.playlist_list()
            .unwrap()
            .into_iter()
            .map(|p| p.name)
            .collect::<Vec<_>>()
    };
    db.playlist_reorder_lists(&[c.id, a.id, b.id]).unwrap();
    assert_eq!(names(&db), ["C", "A", "B"]);
    assert_eq!(
        db.playlist_list()
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
            db.playlist_reorder_lists(&bad),
            Err(DbError::MismatchedItems)
        ));
    }
    assert_eq!(names(&db), ["C", "A", "B"]);
    // 並べ替え後の新規作成も末尾に入る
    let d = db.playlist_create("D").unwrap();
    assert_eq!(d.sort_order, 3);
    assert_eq!(names(&db), ["C", "A", "B", "D"]);
}

/// DB-LD-12: 並べ替え前（全行 sort_order = 0）の既存データでも
/// 新規作成が末尾に入り、初回の並べ替え保存で 0..n-1 に正規化される
/// （FR-35、仕様決定 BF）。
#[test]
fn playlist_create_appends_after_legacy_zero_rows() {
    let db = Db::connect_in_memory().unwrap();
    let a = db.playlist_create("A").unwrap();
    let b = db.playlist_create("B").unwrap();
    db.lock()
        .unwrap()
        .execute("UPDATE playlists SET sort_order = 0", [])
        .unwrap();
    let c = db.playlist_create("C").unwrap();
    assert_eq!(c.sort_order, 1);
    let list = db.playlist_list().unwrap();
    assert_eq!(
        list.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
        ["A", "B", "C"]
    );
    db.playlist_reorder_lists(&[a.id, b.id, c.id]).unwrap();
    assert_eq!(
        db.playlist_list()
            .unwrap()
            .iter()
            .map(|p| p.sort_order)
            .collect::<Vec<_>>(),
        [0, 1, 2]
    );
}
