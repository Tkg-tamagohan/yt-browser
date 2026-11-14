use super::vref;
use crate::db::*;
use crate::model::{
    Category, ExportDump, ExportPlaylistItem, ExportSetting, EXPORT_FORMAT_VERSION,
};

/// 購読・カテゴリ・設定・プレイリストを一式入れた DB を作る共通準備。
fn seed(db: &Db) -> (Category, i64) {
    let cat = db.category_create("音楽").unwrap();
    db.feed_subscribe(&SubscribeArgs {
        channel_id: "UCchan000000000000001",
        title: "テストCH",
        thumbnail_url: Some("https://example.com/ch.jpg"),
        category_id: Some(cat.id),
        entries: &[],
        etag: Some("\"etag-1\""),
        last_modified: Some("Wed, 01 Jan 2026 00:00:00 GMT"),
    })
    .unwrap();
    db.setting_set("feed.show_shorts", "off").unwrap();
    db.setting_set("player.volume", "80").unwrap();
    let pl = db.playlist_create("夜の選曲").unwrap();
    db.playlist_add_many(pl.id, &[vref("aaaaaaaaaa1", "A"), vref("bbbbbbbbbb2", "B")])
        .unwrap();
    (cat, pl.id)
}

/// DB-LD-23: エクスポート/インポートの往復（FR-62、仕様決定 CM）。
/// `export_dump` が 4 テーブルを全列で吐き、別 DB への `import_dump` で
/// 同じ内容が復元される（プレイリスト項目と表示メタの補完を含む）。
#[test]
fn export_import_roundtrip() {
    let db = Db::connect_in_memory().unwrap();
    let (cat, pl_id) = seed(&db);

    let dump = db.export_dump().unwrap();
    assert_eq!(dump.version, EXPORT_FORMAT_VERSION);
    assert_eq!(dump.channels.len(), 1);
    assert_eq!(dump.channels[0].category_id, Some(cat.id));
    assert_eq!(dump.channels[0].rss_etag.as_deref(), Some("\"etag-1\""));
    assert_eq!(dump.categories.len(), 1);
    assert_eq!(dump.settings.len(), 2);
    assert_eq!(dump.playlists.len(), 1);
    assert_eq!(dump.playlists[0].items.len(), 2);
    assert_eq!(dump.playlists[0].items[0].video_id, "aaaaaaaaaa1");
    assert_eq!(
        dump.playlists[0].items[0].title.as_deref(),
        Some("A"),
        "項目は videos 台帳の表示メタを保持する"
    );

    // 別 DB へ復元する
    let dst = Db::connect_in_memory().unwrap();
    dst.import_dump(&dump).unwrap();
    let ch = dst.channel_get("UCchan000000000000001").unwrap().unwrap();
    assert_eq!(ch.title, "テストCH");
    assert_eq!(ch.category_id, Some(cat.id));
    assert_eq!(dst.category_list().unwrap()[0].name, "音楽");
    assert_eq!(
        dst.setting_get("player.volume").unwrap().as_deref(),
        Some("80")
    );
    let lists = dst.playlist_list().unwrap();
    assert_eq!(lists.len(), 1);
    assert_eq!(lists[0].name, "夜の選曲");
    assert_eq!(lists[0].item_count, 2);
    let items = dst.playlist_items(pl_id).unwrap();
    assert_eq!(
        items
            .iter()
            .map(|i| i.video_id.as_str())
            .collect::<Vec<_>>(),
        ["aaaaaaaaaa1", "bbbbbbbbbb2"]
    );
    // 復元先に無かった動画も台帳メタが補完されて一覧で見える
    assert_eq!(items[0].title, "A");
    // JSON 直列化を経た往復でも同じ結果になる
    let json = serde_json::to_string(&dump).unwrap();
    let parsed: ExportDump = serde_json::from_str(&json).unwrap();
    let dst2 = Db::connect_in_memory().unwrap();
    dst2.import_dump(&parsed).unwrap();
    assert_eq!(dst2.channel_list().unwrap().len(), 1);
    assert_eq!(dst2.playlist_list().unwrap()[0].item_count, 2);
}

/// DB-LD-24: インポートは対象テーブルをダンプ内容で置き換える
/// （FR-62、仕様決定 CM）。ダンプに無い行は消え、既存のチャンネル/
/// カテゴリ/設定/プレイリストはダンプ側へ揃う。形式版の不一致や
/// 参照切れのダンプは `Import` エラーで DB を変更しない。
#[test]
fn import_replaces_target_tables_and_rejects_invalid() {
    let db = Db::connect_in_memory().unwrap();
    let (cat, _pl) = seed(&db);
    // 「ダンプした時点」の状態を先に保存する
    let mut dump = db.export_dump().unwrap();
    // ダンプ後に増えた分は置き換えで消える側のデータ
    db.category_create("不要カテゴリ").unwrap();
    db.setting_set("obsolete.key", "x").unwrap();
    db.playlist_create("不要リスト").unwrap();

    // ダンプ側を書き換えて「過去の状態」を模する
    dump.channels[0].category_id = None;
    dump.settings.retain(|s| s.key != "player.volume");
    dump.settings.push(ExportSetting {
        key: "feed.auto_read_days".to_string(),
        value: "7".to_string(),
    });
    dump.playlists[0].name = "改名済み".to_string();
    dump.playlists.truncate(1);
    db.import_dump(&dump).unwrap();

    // ダンプに無い行は消える
    assert_eq!(db.category_list().unwrap().len(), 1);
    assert_eq!(db.category_list().unwrap()[0].id, cat.id);
    assert!(db.setting_get("obsolete.key").unwrap().is_none());
    assert!(db.setting_get("player.volume").unwrap().is_none());
    assert_eq!(
        db.setting_get("feed.auto_read_days").unwrap().as_deref(),
        Some("7")
    );
    // ダンプ側の内容に揃う
    assert!(db
        .channel_get("UCchan000000000000001")
        .unwrap()
        .unwrap()
        .category_id
        .is_none());
    assert_eq!(db.playlist_list().unwrap().len(), 1);
    assert_eq!(db.playlist_list().unwrap()[0].name, "改名済み");

    // 形式版が違うダンプは拒否されて内容が変わらない
    let mut bad = db.export_dump().unwrap();
    bad.version = 999;
    assert!(matches!(db.import_dump(&bad), Err(DbError::Import(_))));
    assert_eq!(db.playlist_list().unwrap()[0].name, "改名済み");

    // 存在しないカテゴリを指すチャンネルは拒否される
    let mut bad = db.export_dump().unwrap();
    bad.channels[0].category_id = Some(9999);
    assert!(matches!(db.import_dump(&bad), Err(DbError::Import(_))));

    // プレイリスト項目の video_id 重複も拒否される
    let mut bad = db.export_dump().unwrap();
    bad.playlists[0].items.push(ExportPlaylistItem {
        video_id: "aaaaaaaaaa1".to_string(),
        position: 99,
        title: None,
        channel_id: None,
        channel_title: None,
        thumbnail_url: None,
    });
    assert!(matches!(db.import_dump(&bad), Err(DbError::Import(_))));

    // 空要素のダンプは受理される（初期状態への復元）
    let empty = ExportDump {
        version: EXPORT_FORMAT_VERSION,
        exported_at: "2026-01-01 00:00:00".to_string(),
        channels: vec![],
        categories: vec![],
        settings: vec![],
        playlists: vec![],
    };
    db.import_dump(&empty).unwrap();
    assert!(db.channel_list().unwrap().is_empty());
    assert!(db.category_list().unwrap().is_empty());
    assert!(db.playlist_list().unwrap().is_empty());
}

/// DB-LD-25: `history_stats` は watch_history を集計して累計視聴時間・
/// チャンネル別・日別の内訳を返す（FR-65、仕様決定 CP）。
/// 1 行の視聴量は「completed なら duration_sec（無ければ position_sec）、
/// 未完は position_sec」で見積もり、日別は last_watched_at の日付で
/// まとめる（暫定解釈）。
#[test]
fn history_stats_aggregates_watch_history() {
    let db = Db::connect_in_memory().unwrap();
    {
        let conn = db.lock().unwrap();
        // 完了: position 0・duration 600 → 600 秒と見積もる
        conn.execute(
            "INSERT INTO watch_history
               (video_id, title, channel_id, channel_title,
                position_sec, duration_sec, last_watched_at, completed)
             VALUES ('done0000001', '完了', 'UCa', 'CH-A', 0, 600,
                     '2026-11-10 10:00:00', 1)",
            [],
        )
        .unwrap();
        // 未完: position 120 → 120 秒
        conn.execute(
            "INSERT INTO watch_history
               (video_id, title, channel_id, channel_title,
                position_sec, duration_sec, last_watched_at, completed)
             VALUES ('half0000001', '途中', 'UCa', 'CH-A', 120, 300,
                     '2026-11-10 12:00:00', 0)",
            [],
        )
        .unwrap();
        // duration 未記録の完了: position 0 のままなら見積もり 0
        conn.execute(
            "INSERT INTO watch_history
               (video_id, title, channel_id, channel_title,
                position_sec, duration_sec, last_watched_at, completed)
             VALUES ('nodur000001', '長さ不明', 'UCb', 'CH-B', 0, NULL,
                     '2026-11-11 09:00:00', 1)",
            [],
        )
        .unwrap();
        // チャンネル不明行は NULL のまとまりに入る
        conn.execute(
            "INSERT INTO watch_history
               (video_id, title, position_sec, last_watched_at)
             VALUES ('noch0000001', '出所不明', 45, '2026-11-11 20:00:00')",
            [],
        )
        .unwrap();
    }
    let stats = db.history_stats().unwrap();
    assert_eq!(stats.total_sec, 765);
    assert_eq!(stats.entry_count, 4);

    // チャンネル別: UCa=720、UCb=0、NULL=45（視聴時間の降順）
    assert_eq!(stats.by_channel.len(), 3);
    assert_eq!(stats.by_channel[0].channel_id.as_deref(), Some("UCa"));
    assert_eq!(stats.by_channel[0].channel_title.as_deref(), Some("CH-A"));
    assert_eq!(stats.by_channel[0].total_sec, 720);
    assert_eq!(stats.by_channel[0].entry_count, 2);
    // NULL のまとまりが末尾に来る（0 秒の UCb より 45 秒なので 2 番目）
    assert_eq!(stats.by_channel[1].channel_id, None);
    assert_eq!(stats.by_channel[1].total_sec, 45);
    assert_eq!(stats.by_channel[2].channel_id.as_deref(), Some("UCb"));
    assert_eq!(stats.by_channel[2].total_sec, 0);

    // 日別: 11/10=720（2 件）、11/11=45（2 件）の降順
    assert_eq!(stats.by_day.len(), 2);
    assert_eq!(stats.by_day[0].day, "2026-11-11");
    assert_eq!(stats.by_day[0].total_sec, 45);
    assert_eq!(stats.by_day[0].entry_count, 2);
    assert_eq!(stats.by_day[1].day, "2026-11-10");
    assert_eq!(stats.by_day[1].total_sec, 720);

    // 履歴ゼロは全て空で返る
    let empty = Db::connect_in_memory().unwrap();
    let stats = empty.history_stats().unwrap();
    assert_eq!(stats.total_sec, 0);
    assert_eq!(stats.entry_count, 0);
    assert!(stats.by_channel.is_empty());
    assert!(stats.by_day.is_empty());
}
