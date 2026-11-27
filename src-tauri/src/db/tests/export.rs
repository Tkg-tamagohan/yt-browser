use super::vref;
use crate::db::*;
use crate::model::{
    Category, ExportDump, ExportPlaylistItem, ExportSetting, EXPORT_FORMAT_VERSION,
};

/// 購読・カテゴリ・設定・プレイリストを一式入れた DB を作る共通準備。
fn seed(db: &Db) -> (Category, i64) {
    let cat = db.category_create("音楽").unwrap();
    db.feed_subscribe(&SubscribeArgs {
        channel_id: "UCchan000000000000000001",
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

/// DB-LD-25: エクスポート/インポートの往復（FR-62、仕様決定 CM）。
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
    let ch = dst
        .channel_get("UCchan000000000000000001")
        .unwrap()
        .unwrap();
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

/// DB-LD-26: インポートは対象テーブルをダンプ内容で置き換える
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
        .channel_get("UCchan000000000000000001")
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

/// DB-LD-27: `history_stats` は watch_history を集計して累計視聴時間・
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

/// DB-LD-31: ローカル保護 4 キー（`ytdlp.po_token` / `ytdlp.path` /
/// `mpv.extra_args` / `setup.deps_shown`）は `export_dump` の出力 `settings`
/// から除外され、非保護キーのみが出力される（FR-94、仕様決定 DX）。
#[test]
fn export_dump_excludes_local_protected_settings() {
    let db = Db::connect_in_memory().unwrap();
    db.setting_set("ytdlp.po_token", "web.gvs+SECRET_TOKEN")
        .unwrap();
    db.setting_set("ytdlp.path", "C:\\tools\\yt-dlp.exe")
        .unwrap();
    db.setting_set("mpv.extra_args", "--profile=fast").unwrap();
    db.setting_set("setup.deps_shown", "on").unwrap();
    db.setting_set("feed.show_shorts", "off").unwrap();
    db.setting_set("ui.theme", "dark").unwrap();

    let dump = db.export_dump().unwrap();
    let keys: Vec<&str> = dump.settings.iter().map(|s| s.key.as_str()).collect();
    assert_eq!(keys, vec!["feed.show_shorts", "ui.theme"]);
    for protected in crate::db::export::LOCAL_PROTECTED_SETTINGS {
        assert!(
            !keys.contains(protected),
            "保護キー {protected} がダンプへ露出してはならない"
        );
    }
}

/// DB-LD-32: 別値の保護 4 キーを含む旧形式ダンプを `import_dump` で
/// 取り込んでも、ローカルの保護 4 キー既存値が保持され（ローカル未設定時は注入されない）、
/// 非保護設定の前後差分だけが返り、`DELETE FROM playlists` に伴う `queue_state` の
/// `ON DELETE CASCADE` と一時キュー保持が正しく機能する（FR-94、仕様決定 DX）。
#[test]
fn import_dump_preserves_local_protected_settings_and_cascades_playlist_queues() {
    let db = Db::connect_in_memory().unwrap();
    let (_cat, pl_id) = seed(&db);
    // ローカル保護 4 キーと追加の非保護キー、一時キュー＋プレイリストキューを仕込む
    db.setting_set("ytdlp.po_token", "local-secret-token")
        .unwrap();
    db.setting_set("ytdlp.path", "C:\\local\\yt-dlp.exe")
        .unwrap();
    db.setting_set("mpv.extra_args", "--local-flag").unwrap();
    db.setting_set("setup.deps_shown", "on").unwrap();
    db.setting_set("obsolete.key", "remove-me").unwrap();
    db.queue_save(
        &[crate::model::QueueItem {
            uid: 1,
            video_id: "aaaaaaaaaa1".to_string(),
            title: "一時キュー動画".to_string(),
            channel_title: Some("CH".to_string()),
            thumbnail_url: None,
        }],
        0,
    )
    .unwrap();
    db.queue_playlist_save(pl_id, 1).unwrap();
    assert_eq!(db.queue_playlist_load().unwrap().len(), 1);

    // 保護 4 キーに別値を混入させた旧形式/外部ダンプを組み立てる
    let mut dump = db.export_dump().unwrap();
    dump.settings.retain(|s| s.key != "obsolete.key");
    for s in &mut dump.settings {
        if s.key == "feed.show_shorts" {
            s.value = "on".to_string();
        }
    }
    dump.settings.push(ExportSetting {
        key: "ui.theme".to_string(),
        value: "light".to_string(),
    });
    dump.settings.push(ExportSetting {
        key: "ytdlp.po_token".to_string(),
        value: "evil-token".to_string(),
    });
    dump.settings.push(ExportSetting {
        key: "ytdlp.path".to_string(),
        value: "/evil/yt-dlp".to_string(),
    });
    dump.settings.push(ExportSetting {
        key: "mpv.extra_args".to_string(),
        value: "--script=/evil.lua".to_string(),
    });
    dump.settings.push(ExportSetting {
        key: "setup.deps_shown".to_string(),
        value: "off".to_string(),
    });

    let changed = db.import_dump(&dump).unwrap();

    // ローカル保護 4 キーは既存値がそのまま保持される
    assert_eq!(
        db.setting_get("ytdlp.po_token").unwrap().as_deref(),
        Some("local-secret-token")
    );
    assert_eq!(
        db.setting_get("ytdlp.path").unwrap().as_deref(),
        Some("C:\\local\\yt-dlp.exe")
    );
    assert_eq!(
        db.setting_get("mpv.extra_args").unwrap().as_deref(),
        Some("--local-flag")
    );
    assert_eq!(
        db.setting_get("setup.deps_shown").unwrap().as_deref(),
        Some("on")
    );
    // 非保護設定の反映と前後差分（保護キーと無変化キーは含まれない）
    assert_eq!(
        db.setting_get("feed.show_shorts").unwrap().as_deref(),
        Some("on")
    );
    assert_eq!(
        db.setting_get("ui.theme").unwrap().as_deref(),
        Some("light")
    );
    assert!(db.setting_get("obsolete.key").unwrap().is_none());
    assert_eq!(
        changed,
        vec![
            ("feed.show_shorts".to_string(), Some("on".to_string())),
            ("obsolete.key".to_string(), None),
            ("ui.theme".to_string(), Some("light".to_string())),
        ]
    );
    // DELETE FROM playlists の ON DELETE CASCADE でプレイリストキュー行は削除され、
    // 一時キューは保持される
    assert!(db.queue_playlist_load().unwrap().is_empty());
    assert_eq!(db.queue_load().unwrap().items.len(), 1);

    // ローカル未設定の DB へ保護 4 キー入りダンプを取り込んでも注入されない
    let clean = Db::connect_in_memory().unwrap();
    clean.import_dump(&dump).unwrap();
    for protected in crate::db::export::LOCAL_PROTECTED_SETTINGS {
        assert!(
            clean.setting_get(protected).unwrap().is_none(),
            "未設定ローカル DB へ保護キー {protected} が注入されてはならない"
        );
    }

    // 保護キーに 16 KiB 超の値と重複エントリが混入していても読み飛ばされ、
    // ローカル保護 4 キーを保持しつつ非保護設定・購読・プレイリストが復元される
    let mut oversized_dup = db.export_dump().unwrap();
    oversized_dup.settings.push(ExportSetting {
        key: "mpv.extra_args".to_string(),
        value: "x".repeat(16 * 1024 + 10),
    });
    oversized_dup.settings.push(ExportSetting {
        key: "mpv.extra_args".to_string(),
        value: "--dup-ignored".to_string(),
    });
    oversized_dup.settings.push(ExportSetting {
        key: "ytdlp.po_token".to_string(),
        value: "y".repeat(20 * 1024),
    });
    db.import_dump(&oversized_dup).unwrap();
    assert_eq!(
        db.setting_get("mpv.extra_args").unwrap().as_deref(),
        Some("--local-flag")
    );
    assert_eq!(
        db.setting_get("ytdlp.po_token").unwrap().as_deref(),
        Some("local-secret-token")
    );
    assert!(db
        .channel_get("UCchan000000000000000001")
        .unwrap()
        .is_some());
    assert_eq!(db.playlist_list().unwrap().len(), 1);
}

/// DB-LD-33: `validate_dump` の境界検証（不正 `channel_id` / 不正 `video_id` /
/// 129 バイトキー / 16 KiB 超値 / 151 文字プレイリスト名の拒否、未知非保護キーと
/// 境界上限値の受理）および `ExportPlaylistItem.channel_id` 非 UC 値の `None`
/// 正規化を検証する（FR-94、仕様決定 DX・DZ）。
#[test]
fn validate_dump_enforces_boundaries_and_normalizes_playlist_channel_id() {
    let db = Db::connect_in_memory().unwrap();
    let (_cat, _pl) = seed(&db);
    let base = db.export_dump().unwrap();

    // 不正な ExportChannel.channel_id（21 文字・非 UC）は拒否
    let mut bad = base.clone();
    bad.channels[0].channel_id = "UCchan000000000000001".to_string();
    assert!(matches!(db.import_dump(&bad), Err(DbError::Import(_))));
    let mut bad = base.clone();
    bad.channels[0].channel_id = "XXchan000000000000000001".to_string();
    assert!(matches!(db.import_dump(&bad), Err(DbError::Import(_))));

    // 不正な ExportPlaylistItem.video_id（10 文字・記号不正）は拒否
    let mut bad = base.clone();
    bad.playlists[0].items[0].video_id = "short12345".to_string();
    assert!(matches!(db.import_dump(&bad), Err(DbError::Import(_))));
    let mut bad = base.clone();
    bad.playlists[0].items[0].video_id = "bad!video01".to_string();
    assert!(matches!(db.import_dump(&bad), Err(DbError::Import(_))));

    // settings key 129 バイトは拒否、128 バイトの未知非保護キーは受理
    let mut bad = base.clone();
    bad.settings.push(ExportSetting {
        key: "k".repeat(129),
        value: "v".to_string(),
    });
    assert!(matches!(db.import_dump(&bad), Err(DbError::Import(_))));

    // settings value 16 KiB + 1 バイトは拒否
    let mut bad = base.clone();
    bad.settings.push(ExportSetting {
        key: "custom.unknown_key".to_string(),
        value: "v".repeat(16 * 1024 + 1),
    });
    assert!(matches!(db.import_dump(&bad), Err(DbError::Import(_))));

    // playlists.name 151 文字（マルチバイト）は拒否
    let mut bad = base.clone();
    bad.playlists[0].name = "あ".repeat(151);
    assert!(matches!(db.import_dump(&bad), Err(DbError::Import(_))));

    // 境界上限（128B 未知キー・16 KiB 値・前後空白付き 150 文字プレイリスト名）と
    // ExportPlaylistItem.channel_id の非 UC 値→None 正規化を受理し、プレイリスト名はトリム保存される
    let mut ok = base;
    ok.settings.push(ExportSetting {
        key: "k".repeat(128),
        value: "v".repeat(16 * 1024),
    });
    ok.playlists[0].name = format!("   {}   ", "あ".repeat(150));
    ok.playlists[0].items[0].channel_id = Some("@not_a_uc_id".to_string());
    ok.playlists[0].items[1].channel_id = Some("UCvalid00000000000000001".to_string());

    let dst = Db::connect_in_memory().unwrap();
    dst.import_dump(&ok).unwrap();
    assert_eq!(dst.playlist_list().unwrap()[0].name, "あ".repeat(150));
    assert_eq!(
        dst.setting_get(&"k".repeat(128)).unwrap().map(|v| v.len()),
        Some(16 * 1024)
    );
    // 非 UC の channel_id は None（videos テーブル上は空文字 → video_channel で None）に正規化
    assert_eq!(dst.video_channel("aaaaaaaaaa1").unwrap().0, None);
    // 正規 UC の channel_id はそのまま保持される
    assert_eq!(
        dst.video_channel("bbbbbbbbbb2").unwrap().0.as_deref(),
        Some("UCvalid00000000000000001")
    );
}

/// DB-LD-42: `youtube.api_key` が `LOCAL_PROTECTED_SETTINGS` に含まれ、
/// `export_dump` から除外され、`import_dump` でもローカル既存値が保持され未設定 DB へ
/// 注入されないことを検証する（FR-97、仕様決定 EC）。
#[test]
fn export_and_import_protect_youtube_api_key() {
    let db = Db::connect_in_memory().unwrap();
    db.setting_set("youtube.api_key", "AIzaSyLOCAL_KEEP")
        .unwrap();
    db.setting_set("ui.theme", "dark").unwrap();

    let dump = db.export_dump().unwrap();
    let keys: Vec<&str> = dump.settings.iter().map(|s| s.key.as_str()).collect();
    assert_eq!(keys, vec!["ui.theme"]);
    assert!(!keys.contains(&"youtube.api_key"));

    let mut evil_dump = dump;
    evil_dump.settings.push(ExportSetting {
        key: "youtube.api_key".to_string(),
        value: "AIzaSyEVIL_OVERWRITE".to_string(),
    });
    let changed = db.import_dump(&evil_dump).unwrap();
    assert!(changed.is_empty(), "{changed:?}");
    assert_eq!(
        db.setting_get("youtube.api_key").unwrap().as_deref(),
        Some("AIzaSyLOCAL_KEEP")
    );

    let clean = Db::connect_in_memory().unwrap();
    clean.import_dump(&evil_dump).unwrap();
    assert_eq!(clean.setting_get("youtube.api_key").unwrap(), None);
}
