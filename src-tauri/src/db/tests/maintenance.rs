use super::vref;
use crate::db::*;

/// DB-LD-29: 孤児行整理（FR-66、仕様決定 CQ）。
/// `channels` に無いチャンネルの videos 行のうち、どの参照元にも
/// 残っていないものだけを削除する。参照のある行・チャンネル ID が
/// 空の台帳メタ行・購読中チャンネルの行は消さない。
/// 台帳に無い video_id を持つ favorites / watch_later / playlist_items
/// の参照切れ行も一緒に掃除する（暫定解釈）。
#[test]
fn cleanup_orphans_removes_only_unreferenced_rows() {
    let db = Db::connect_in_memory().unwrap();
    // 購読中チャンネルと配信動画
    db.feed_subscribe(&SubscribeArgs {
        channel_id: "UClive00000000000001",
        title: "残るCH",
        thumbnail_url: None,
        category_id: None,
        entries: &[NewVideo {
            video_id: "subvid00001",
            channel_id: "UClive00000000000001",
            channel_title: "残るCH",
            title: "配信V",
            thumbnail_url: None,
            published_at: Some("2026-10-01 00:00:00"),
            kind: "video",
        }],
        etag: None,
        last_modified: None,
    })
    .unwrap();
    // 購読解除したチャンネルの残存配信動画（孤児候補）
    db.feed_subscribe(&SubscribeArgs {
        channel_id: "UCgone00000000000001",
        title: "消すCH",
        thumbnail_url: None,
        category_id: None,
        entries: &[
            NewVideo {
                video_id: "orphan00001",
                channel_id: "UCgone00000000000001",
                channel_title: "消すCH",
                title: "孤児V1",
                thumbnail_url: None,
                published_at: Some("2026-10-02 00:00:00"),
                kind: "video",
            },
            NewVideo {
                video_id: "orphan00002",
                channel_id: "UCgone00000000000001",
                channel_title: "消すCH",
                title: "孤児V2",
                thumbnail_url: None,
                published_at: Some("2026-10-03 00:00:00"),
                kind: "video",
            },
            NewVideo {
                video_id: "favorited01",
                channel_id: "UCgone00000000000001",
                channel_title: "消すCH",
                title: "残すFav",
                thumbnail_url: None,
                published_at: Some("2026-10-04 00:00:00"),
                kind: "video",
            },
        ],
        etag: None,
        last_modified: None,
    })
    .unwrap();
    db.channel_delete("UCgone00000000000001").unwrap();
    // 孤児側の 1 件はお気に入り参照があるので消えない
    let mut fav = vref("favorited01", "残すFav");
    fav.channel_id = Some("UCgone00000000000001".to_string());
    db.favorite_add(&fav).unwrap();
    // チャンネル ID を持たない台帳メタ行は対象外
    let mut meta = vref("ledger00001", "台帳のみ");
    meta.channel_id = None;
    db.watch_later_add(&meta).unwrap();

    let removed = db.db_cleanup_orphans().unwrap();
    assert_eq!(removed, 2, "孤児の配信動画 2 件だけ消える");
    {
        let conn = db.lock().unwrap();
        let exists = |vid: &str| {
            conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM videos WHERE video_id = ?1)",
                [vid],
                |r| r.get::<_, i64>(0),
            )
            .unwrap()
        };
        assert_eq!(exists("orphan00001"), 0);
        assert_eq!(exists("orphan00002"), 0);
        assert_eq!(exists("favorited01"), 1, "お気に入り参照は残す");
        assert_eq!(exists("subvid00001"), 1, "購読中の動画は残す");
        assert_eq!(exists("ledger00001"), 1, "台帳メタ行は残す");

        // 参照切れの行を直接仕込む（videos に無い video_id）
        conn.execute(
            "INSERT INTO favorites (video_id) VALUES ('dangling001')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO watch_later (video_id) VALUES ('dangling002')",
            [],
        )
        .unwrap();
        conn.execute("INSERT INTO playlists (name) VALUES ('P')", [])
            .unwrap();
        conn.execute(
            "INSERT INTO playlist_items (playlist_id, video_id, position)
             VALUES ((SELECT MAX(id) FROM playlists), 'dangling003', 0)",
            [],
        )
        .unwrap();
    }
    let removed = db.db_cleanup_orphans().unwrap();
    assert_eq!(removed, 3, "参照切れの 3 行を掃除する");
    {
        let conn = db.lock().unwrap();
        for (table, vid) in [
            ("favorites", "dangling001"),
            ("watch_later", "dangling002"),
            ("playlist_items", "dangling003"),
        ] {
            let n: i64 = conn
                .query_row(
                    &format!("SELECT COUNT(*) FROM {table} WHERE video_id = ?1"),
                    [vid],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(n, 0, "{table} の {vid} は消える");
        }
    }
    // 2 度目の実行は 0 件（冪等）
    assert_eq!(db.db_cleanup_orphans().unwrap(), 0);
}

/// DB-LD-30: DB サイズ表示と VACUUM（FR-66、仕様決定 CQ）。
/// インメモリ接続では page_count × page_size の近似値を返し、
/// `db_vacuum` は正常終了する。ファイル接続では本体＋WAL 系
/// ファイルの合計を返す（暫定解釈）。
#[test]
fn db_size_and_vacuum() {
    let db = Db::connect_in_memory().unwrap();
    let size = db.db_size().unwrap();
    assert!(size.bytes > 0, "インメモリでも近似値が返る");
    db.db_vacuum().unwrap();

    // ファイル接続は実ファイルのサイズを拾う
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("maint.db");
    let fdb = Db::connect(&path).unwrap();
    let fsize = fdb.db_size().unwrap();
    assert!(fsize.bytes > 0);
    // ダンプ後はサイズが増え、VACUUM で正常終了する
    for i in 0..50 {
        fdb.setting_set(&format!("bulk.key.{i}"), &"x".repeat(200))
            .unwrap();
    }
    let grown = fdb.db_size().unwrap();
    assert!(grown.bytes >= fsize.bytes);
    fdb.db_vacuum().unwrap();
    // VACUUM 後も取得と操作が続けられる
    assert!(fdb.db_size().unwrap().bytes > 0);
    assert!(fdb.schema_version().unwrap() > 0);
}
