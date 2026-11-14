use crate::db::*;
use crate::model::LiveWait;

fn wait(video_id: &str, scheduled_ms: Option<i64>, registered_ms: i64) -> LiveWait {
    LiveWait {
        video_id: video_id.to_string(),
        title: format!("配信 {video_id}"),
        channel_id: Some("UCchan000000000000001".to_string()),
        channel_title: Some("テストCH".to_string()),
        thumbnail_url: Some("https://i.ytimg.com/vi/x.jpg".to_string()),
        scheduled_start_ms: scheduled_ms,
        registered_ms,
    }
}

/// DB-LW-01: 待機の登録と一覧の往復（FR-44、仕様決定 BT(2)(5)）。
/// 表示メタ・予定時刻・登録時刻がそのまま読み出せる。
#[test]
fn live_wait_insert_list_round_trip() {
    let db = Db::connect_in_memory().unwrap();
    assert!(db.live_wait_list().unwrap().is_empty());

    let mut e = wait("dQw4w9WgXcQ", Some(1_837_263_600_000), 1_000);
    assert!(db.live_wait_insert(&e).unwrap());
    let list = db.live_wait_list().unwrap();
    assert_eq!(list.len(), 1);
    let g = &list[0];
    assert_eq!(g.video_id, "dQw4w9WgXcQ");
    assert_eq!(g.title, "配信 dQw4w9WgXcQ");
    assert_eq!(g.channel_id.as_deref(), Some("UCchan000000000000001"));
    assert_eq!(g.channel_title.as_deref(), Some("テストCH"));
    assert_eq!(g.thumbnail_url.as_deref(), Some("https://i.ytimg.com/vi/x.jpg"));
    assert_eq!(g.scheduled_start_ms, Some(1_837_263_600_000));
    assert_eq!(g.registered_ms, 1_000);

    // NULL 可のメタは欠けたまま往復する
    e.video_id = "abc12345678".to_string();
    e.channel_id = None;
    e.channel_title = None;
    e.thumbnail_url = None;
    e.scheduled_start_ms = None;
    assert!(db.live_wait_insert(&e).unwrap());
    let g = db.live_wait_get("abc12345678").unwrap().unwrap();
    assert!(g.channel_id.is_none() && g.channel_title.is_none());
    assert!(g.thumbnail_url.is_none() && g.scheduled_start_ms.is_none());
    // 未登録の取得は None
    assert!(db.live_wait_get("zzzzzzzzzzz").unwrap().is_none());
}

/// DB-LW-02: 同一動画の重複待機の抑止（仕様決定 BT(6)）。
/// 2 回目の挿入は false で返り、既存行は上書きされない。
#[test]
fn live_wait_insert_dedups_by_video_id() {
    let db = Db::connect_in_memory().unwrap();
    let mut e = wait("dQw4w9WgXcQ", Some(100), 1_000);
    assert!(db.live_wait_insert(&e).unwrap());
    // 予定や題名の異なる重複は抑止され、先の行が残る
    e.title = "別タイトル".to_string();
    e.scheduled_start_ms = Some(200);
    assert!(!db.live_wait_insert(&e).unwrap());
    let list = db.live_wait_list().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].title, "配信 dQw4w9WgXcQ");
    assert_eq!(list[0].scheduled_start_ms, Some(100));
    // 別動画は登録できる
    assert!(db.live_wait_insert(&wait("abc12345678", None, 2_000)).unwrap());
    assert_eq!(db.live_wait_list().unwrap().len(), 2);
}

/// DB-LW-03: 待機一覧の順序（予定時刻の昇順・未定は末尾・登録順のタイブレーク）。
#[test]
fn live_wait_list_order() {
    let db = Db::connect_in_memory().unwrap();
    db.live_wait_insert(&wait("v_late_____", Some(300), 10)).unwrap();
    db.live_wait_insert(&wait("v_none_____", None, 5)).unwrap();
    db.live_wait_insert(&wait("v_early____", Some(100), 15)).unwrap();
    db.live_wait_insert(&wait("v_early2___", Some(100), 8)).unwrap();
    let ids: Vec<String> = db
        .live_wait_list()
        .unwrap()
        .into_iter()
        .map(|w| w.video_id)
        .collect();
    // 予定昇順（同時刻は登録時刻昇順）→ 予定未定は末尾
    assert_eq!(
        ids,
        vec!["v_early2___", "v_early____", "v_late_____", "v_none_____"]
    );
}

/// DB-LW-04: 個別解除。既存行は true・未登録は false で冪等。
#[test]
fn live_wait_remove_individual() {
    let db = Db::connect_in_memory().unwrap();
    db.live_wait_insert(&wait("dQw4w9WgXcQ", Some(100), 1)).unwrap();
    db.live_wait_insert(&wait("abc12345678", None, 2)).unwrap();
    assert!(db.live_wait_remove("dQw4w9WgXcQ").unwrap());
    let ids: Vec<String> = db
        .live_wait_list()
        .unwrap()
        .into_iter()
        .map(|w| w.video_id)
        .collect();
    assert_eq!(ids, vec!["abc12345678"]);
    // 未登録の解除は false（冪等）
    assert!(!db.live_wait_remove("dQw4w9WgXcQ").unwrap());
    assert!(!db.live_wait_remove("zzzzzzzzzzz").unwrap());
}

/// DB-LW-05: DB 再接続をまたぐ永続化（仕様決定 BT(5)）。
/// アプリ再起動相当で待機リストと予定時刻が復元される。
#[test]
fn live_wait_persists_across_reconnect() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    {
        let db = Db::connect(&path).unwrap();
        db.live_wait_insert(&wait("dQw4w9WgXcQ", Some(1_837_263_600_000), 1_000))
            .unwrap();
        db.live_wait_insert(&wait("abc12345678", None, 2_000)).unwrap();
    }
    let db = Db::connect(&path).unwrap();
    let list = db.live_wait_list().unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(list[0].scheduled_start_ms, Some(1_837_263_600_000));
    assert_eq!(list[1].scheduled_start_ms, None);
    assert_eq!(list[0].registered_ms, 1_000);
}

/// DB-LW-06: 予定時刻の更新。InnerTube 側の予定変更への追随で、
/// 値が同じときは行を書き換えない（registered_ms 起点の失効に影響しない）。
#[test]
fn live_wait_set_scheduled_follows() {
    let db = Db::connect_in_memory().unwrap();
    db.live_wait_insert(&wait("dQw4w9WgXcQ", Some(100), 1_000)).unwrap();
    // 予定が後ろへずれた（YouTube 側で延期）
    db.live_wait_set_scheduled("dQw4w9WgXcQ", Some(200)).unwrap();
    assert_eq!(
        db.live_wait_get("dQw4w9WgXcQ")
            .unwrap()
            .unwrap()
            .scheduled_start_ms,
        Some(200)
    );
    // 未定 → 確定、確定 → 未定（中止ロビー等）の両方向へ追随する
    db.live_wait_set_scheduled("dQw4w9WgXcQ", None).unwrap();
    assert_eq!(
        db.live_wait_get("dQw4w9WgXcQ")
            .unwrap()
            .unwrap()
            .scheduled_start_ms,
        None
    );
    // 未登録の動画 ID への更新は対象なしで終わる
    db.live_wait_set_scheduled("zzzzzzzzzzz", Some(1)).unwrap();
    assert!(db.live_wait_get("zzzzzzzzzzz").unwrap().is_none());
}
