use crate::db::*;

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

/// DB-LD-10: 保持上限 10000 件を超える履歴は起動時剪定で最古から消える
/// （FR-7、仕様決定 AV）。
#[test]
fn history_prune_keeps_recent_cap() {
    let db = Db::connect_in_memory().unwrap();
    // last_watched_at を古い順にずらして 10001 件を直接仕込む
    {
        let mut conn = db.lock().unwrap();
        let tx = conn.transaction().unwrap();
        for i in 0..10001 {
            tx.execute(
                "INSERT INTO watch_history (video_id, title, last_watched_at)
                 VALUES (?1, 't', datetime('2020-01-01', '+' || ?2 || ' seconds'))",
                rusqlite::params![format!("v{i}"), i],
            )
            .unwrap();
        }
        tx.commit().unwrap();
    }
    db.history_prune().unwrap();
    let list = db.history_list(10001).unwrap();
    assert_eq!(list.len(), 10000);
    // 最新（v10000）が先頭、境界の v1 が最後に残り、最古の v0 が消える
    assert_eq!(list[0].video_id, "v10000");
    assert_eq!(list[9999].video_id, "v1");
    assert!(db.history_get("v0").unwrap().is_none());
    // 上限以内に戻った後は何も消さない
    db.history_prune().unwrap();
    assert_eq!(db.history_list(10001).unwrap().len(), 10000);
}
