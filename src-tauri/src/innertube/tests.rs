use super::*;

#[test]
fn extract_cfg_finds_keys() {
    let html = r#"<script>ytcfg.set({"INNERTUBE_API_KEY":"AIzaSyDUMMY","INNERTUBE_CONTEXT_CLIENT_VERSION":"2.20260101.00.00","VISITOR_DATA":"CgsxMjM0NTY3ODkwIw%3D%3D"});</script>"#;
    let cfg = InnerTube::extract_cfg(html).unwrap();
    assert_eq!(cfg.api_key, "AIzaSyDUMMY");
    assert_eq!(cfg.client_version, "2.20260101.00.00");
    assert_eq!(cfg.visitor_data, "CgsxMjM0NTY3ODkwIw%3D%3D");
}

#[test]
fn extract_cfg_missing_key_is_error() {
    let err = InnerTube::extract_cfg("<html></html>").unwrap_err();
    assert!(matches!(
        err,
        InnerTubeError::CfgMissing("INNERTUBE_API_KEY")
    ));
}

#[test]
fn parse_related_extracts_compact_video_renderer() {
    let v = serde_json::json!({
        "contents": {
            "twoColumnWatchNextResults": {
                "secondaryResults": {
                    "secondaryResults": {
                        "results": [
                            {
                                "compactVideoRenderer": {
                                    "videoId": "rel123abcde",
                                    "title": {"simpleText": "関連動画A"},
                                    "lengthText": {"simpleText": "10:30"},
                                    "viewCountText": {"simpleText": "123,456 回視聴"},
                                    "shortBylineText": {"runs": [{
                                        "text": "チャンネルA",
                                        "navigationEndpoint": {
                                            "browseEndpoint": {"browseId": "UCchannel0000000000001"}
                                        }
                                    }]},
                                    "thumbnail": {"thumbnails": [
                                        {"url": "https://i.ytimg.com/vi/rel123abcde/default.jpg"},
                                        {"url": "https://i.ytimg.com/vi/rel123abcde/hqdefault.jpg"}
                                    ]}
                                }
                            },
                            {"compactPlaylistRenderer": {"playlistId": "PLxxx"}}
                        ]
                    }
                }
            }
        }
    });
    let out = parse_related(&v);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].video_id, "rel123abcde");
    assert_eq!(out[0].title, "関連動画A");
    assert_eq!(out[0].duration_sec, Some(630));
    assert_eq!(out[0].view_count, Some(123456));
    assert_eq!(out[0].channel_id.as_deref(), Some("UCchannel0000000000001"));
    assert_eq!(
        out[0].thumbnail_url.as_deref(),
        Some("https://i.ytimg.com/vi/rel123abcde/hqdefault.jpg")
    );
}

/// golden fixture: 実際に取得した `/youtubei/v1/next` 応答（技術方針 O）。
/// 2026-10 時点の WEB クライアント応答は `lockupViewModel` 形式で、
/// `LOCKUP_CONTENT_TYPE_PLAYLIST`（Mix 等）は除外する。
#[test]
fn parse_related_golden_fixture() {
    let v: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/youtube_next_related.json"
    ))
    .unwrap();
    let out = parse_related(&v);
    assert_eq!(out.len(), 2);
    let first = &out[0];
    assert_eq!(first.video_id, "j4Rj2-bvnQM");
    assert!(first.title.starts_with("80s Greatest Hits"));
    assert_eq!(first.channel_title.as_deref(), Some("Pure 80s Retro"));
    assert_eq!(
        first.channel_id.as_deref(),
        Some("UC_zEaYJqYB67M8DGHk5Xo8A")
    );
    assert_eq!(first.duration_sec, Some(3690)); // "1:01:30"
    assert_eq!(first.view_count, Some(32000)); // "32K"
    assert!(first
        .thumbnail_url
        .as_deref()
        .unwrap_or("")
        .contains("i.ytimg.com/vi/j4Rj2-bvnQM"));
    let second = &out[1];
    assert_eq!(second.video_id, "pLvKkPyDl7M");
    assert_eq!(
        second.channel_id.as_deref(),
        Some("UCSu7x-J8ESHojsq8mCvFOZw")
    );
    assert_eq!(second.duration_sec, Some(848)); // "14:08"
    assert_eq!(second.view_count, Some(156000)); // "156K"
}

/// golden fixture: 実際に取得した `/youtubei/v1/player` 応答（技術方針 O、FR-33）。
/// 2026-10 時点の WEB クライアント応答。fixture の動画（Lofi Girl の mix）の
/// 概要欄はトラックリストの `[mm:ss]` 表記と URL を含む。
#[test]
fn parse_description_golden_fixture() {
    let v: Value =
        serde_json::from_str(include_str!("../../tests/fixtures/youtube_player.json")).unwrap();
    let d = parse_description(&v);
    assert!(d.contains("Tracklist"));
    assert!(d.contains("https://fanlink.tv/lofigirl-music"));
    assert!(d.contains("[03:25] Fatb - Cotton Cloud"));
    assert!(d.lines().count() > 10); // 改行はそのまま保持される
}

/// 概要欄が欠落・非文字列の応答は空文字列へ潰す（FR-33）。
#[test]
fn parse_description_missing_is_empty() {
    assert_eq!(parse_description(&serde_json::json!({})), "");
    assert_eq!(
        parse_description(&serde_json::json!({"videoDetails": {}})),
        ""
    );
    assert_eq!(
        parse_description(&serde_json::json!({"videoDetails": {"shortDescription": null}})),
        ""
    );
    assert_eq!(
        parse_description(&serde_json::json!({"videoDetails": {"shortDescription": "本文"}})),
        "本文"
    );
}

/// golden fixture: `youtube_player.json` の `videoDetails` から
/// タイトル・チャンネル・サムネイルを取る（FR-29、仕様決定 BD）
#[test]
fn parse_video_meta_golden_fixture() {
    let v: Value =
        serde_json::from_str(include_str!("../../tests/fixtures/youtube_player.json")).unwrap();
    let m = parse_video_meta(&v).unwrap();
    assert_eq!(m.video_id, "lTRiuFIWV54");
    assert_eq!(m.title, "1 A.M Study Session 📚 [lofi hip hop]");
    assert_eq!(m.channel_id.as_deref(), Some("UCSJ4gkVC6NrvII8umztf0Ow"));
    assert_eq!(m.channel_title.as_deref(), Some("Lofi Girl"));
    assert!(m
        .thumbnail_url
        .as_deref()
        .unwrap_or("")
        .contains("i.ytimg.com/vi/lTRiuFIWV54"));
}

/// タイトルの欠落・空文字・`videoDetails` 自体の欠落は None。
/// サムネイル列挙が無い応答は `hqdefault.jpg` へ倒す
#[test]
fn parse_video_meta_missing_title_is_none() {
    assert!(parse_video_meta(&serde_json::json!({})).is_none());
    assert!(parse_video_meta(&serde_json::json!({"videoDetails": {}})).is_none());
    assert!(parse_video_meta(&serde_json::json!({"videoDetails": {"title": ""}})).is_none());
    assert!(parse_video_meta(&serde_json::json!({"videoDetails": {"title": "   "}})).is_none());
}

#[test]
fn parse_video_meta_missing_thumbnail_fallback() {
    let m = parse_video_meta(&serde_json::json!({
        "videoDetails": {"videoId": "abc12345678", "title": "タイトル"}
    }))
    .unwrap();
    assert_eq!(
        m.thumbnail_url.as_deref(),
        Some("https://i.ytimg.com/vi/abc12345678/hqdefault.jpg")
    );
}

#[test]
fn parse_compact_count_variants() {
    assert_eq!(parse_compact_count("32K"), Some(32000));
    assert_eq!(parse_compact_count("1.2M"), Some(1200000));
    assert_eq!(parse_compact_count("3B"), Some(3000000000));
    assert_eq!(parse_compact_count("1,234"), Some(1234));
    assert_eq!(parse_compact_count("views"), None);
    assert_eq!(parse_compact_count(""), None);
}

#[test]
fn parse_length_text_variants() {
    assert_eq!(parse_length_text("3:05"), Some(185));
    assert_eq!(parse_length_text("1:02:03"), Some(3723));
    assert_eq!(parse_length_text(""), None);
    assert_eq!(parse_length_text("LIVE"), None);
}

/// FR-24/AQ: 継続トークンの種別判定。`liveChatReplayContinuationData`
/// はリプレイ、その他（timed/invalidation/reload）はライブ。
/// 混在時はリプレイを優先する。
#[test]
fn next_continuation_detects_kind() {
    let live = serde_json::json!({"continuations": [
        {"timedContinuationData": {"continuation": "LIVETOKEN", "timeoutMs": 8000}}
    ]});
    let (t, ms, k) = next_continuation(&live).unwrap();
    assert_eq!(
        (t.as_str(), ms, k),
        ("LIVETOKEN", 8000, ContinuationKind::Live)
    );

    let replay = serde_json::json!({"continuations": [
        {"liveChatReplayContinuationData": {"continuation": "REPLAYTOKEN"}}
    ]});
    let (t, _, k) = next_continuation(&replay).unwrap();
    assert_eq!((t.as_str(), k), ("REPLAYTOKEN", ContinuationKind::Replay));

    // 混在（終了直後の切替期）はリプレイ優先
    let mixed = serde_json::json!({"continuations": [
        {"timedContinuationData": {"continuation": "LIVE2", "timeoutMs": 500}},
        {"liveChatReplayContinuationData": {"continuation": "REPLAY2"}}
    ]});
    let (t, _, k) = next_continuation(&mixed).unwrap();
    assert_eq!((t.as_str(), k), ("REPLAY2", ContinuationKind::Replay));

    assert!(next_continuation(&serde_json::json!({})).is_none());
    assert!(next_continuation(&serde_json::json!({"continuations": []})).is_none());
}

/// FR-24/AQ: liveBroadcastDetails タイムスタンプの ISO8601 パース。
#[test]
fn iso8601_ms_parses_timestamps() {
    // Z 終端
    assert_eq!(iso8601_ms("1970-01-01T00:00:00Z"), Some(0));
    assert_eq!(iso8601_ms("2026-05-01T12:00:00Z"), Some(1_777_636_800_000));
    // 小数秒とオフセット
    assert_eq!(iso8601_ms("1970-01-01T00:00:00.500Z"), Some(500));
    assert_eq!(iso8601_ms("1970-01-01T09:00:00+09:00"), Some(0));
    // 負オフセットは UTC 側へ進める（00:30 - (-00:30) = 01:00Z）
    assert_eq!(iso8601_ms("1970-01-01T00:30:00-00:30"), Some(3_600_000));
    // 非ゼロオフセットの実例（+09:00 は 9 時間前）
    assert_eq!(
        iso8601_ms("2026-05-01T21:00:00+09:00"),
        iso8601_ms("2026-05-01T12:00:00Z")
    );
    // 不正形は None
    assert_eq!(iso8601_ms("2026/05/01"), None);
    assert_eq!(iso8601_ms(""), None);
    assert_eq!(iso8601_ms("not a date"), None);
}

/// FR-24/AQ: ytInitialPlayerResponse から放送窓を取る。
#[test]
fn extract_broadcast_window_from_player_response() {
    let html = r#"window["ytInitialPlayerResponse"] = {"videoDetails":{"lengthSeconds":"600"},"microformat":{"playerMicroformatRenderer":{"liveBroadcastDetails":{"startTimestamp":"2026-05-01T12:00:00Z","endTimestamp":"2026-05-01T12:15:00Z","isLiveNow":false}}}};"#;
    let w = extract_broadcast_window(html);
    assert_eq!(w.start_ms, Some(1_777_636_800_000));
    assert_eq!(w.end_ms, Some(1_777_637_700_000));
    assert_eq!(w.length_secs, Some(600));
    assert_eq!(w.is_live_now, Some(false));

    // 情報の無い HTML は全て None
    let w = extract_broadcast_window("<html>no data</html>");
    assert!(w.start_ms.is_none() && w.end_ms.is_none() && w.length_secs.is_none());
    assert!(w.is_live_now.is_none());
}

/// FR-24/AQ: 終了済み判定（リプレイ経路のルーティング）。
/// `isLiveNow` が優先、欠落時は `endTimestamp` の有無で判断する。
#[test]
fn broadcast_window_is_ended() {
    let w = |is_live_now: Option<bool>, end_ms: Option<i64>| BroadcastWindow {
        is_live_now,
        end_ms,
        ..Default::default()
    };
    // 実機確認値: 終了済みは isLiveNow:false + endTimestamp あり
    assert!(w(Some(false), Some(1)).is_ended());
    assert!(!w(Some(true), None).is_ended());
    // 予約配信のロビーは isLiveNow:false でも endTimestamp が無い
    assert!(!w(Some(false), None).is_ended());
    // isLiveNow 欠落時は endTimestamp の有無で判断
    assert!(w(None, Some(1)).is_ended());
    assert!(!w(None, None).is_ended());
    // isLiveNow:true だが end がある矛盾形は「放送中」を優先
    assert!(!w(Some(true), Some(1)).is_ended());
}

/// FR-44/BT: `player` 応答から放送状態を取る。
/// 予約配信の実応答（isLiveNow:false＋startTimestamp＋isUpcoming）を再現する。
#[test]
fn parse_broadcast_status_upcoming() {
    // 2026-11 実機観測の形: playabilityStatus は分類に使わない
    let v = serde_json::json!({
        "playabilityStatus": {"status": "LIVE_STREAM_OFFLINE"},
        "videoDetails": {"videoId": "abc12345678", "isUpcoming": true,
                         "isLiveContent": true},
        "microformat": {"playerMicroformatRenderer": {
            "liveBroadcastDetails": {
                "isLiveNow": false,
                "startTimestamp": "2028-03-21T15:00:00+00:00"
            }
        }}
    });
    let s = parse_broadcast_status(&v);
    assert_eq!(s.is_live_now, Some(false));
    assert_eq!(s.start_ms, Some(1_837_263_600_000));
    assert!(s.end_ms.is_none());
    assert!(s.scheduled_start_ms.is_none());
    assert!(s.has_details);
    assert!(s.is_upcoming);
}

/// FR-44/BT: 放送中と終了済みの分類材料。`scheduledStartTimestamp` が
/// あれば startTimestamp より優先して予定時刻へ入れる（未観測フィールド）。
#[test]
fn parse_broadcast_status_live_ended_scheduled() {
    let live = serde_json::json!({
        "videoDetails": {"videoId": "a"},
        "microformat": {"playerMicroformatRenderer": {
            "liveBroadcastDetails": {
                "isLiveNow": true,
                "startTimestamp": "2026-11-03T18:00:39+00:00"
            }
        }}
    });
    let s = parse_broadcast_status(&live);
    assert_eq!(s.is_live_now, Some(true));
    assert!(s.has_details && !s.is_upcoming);

    let ended = serde_json::json!({
        "videoDetails": {"videoId": "b"},
        "microformat": {"playerMicroformatRenderer": {
            "liveBroadcastDetails": {
                "isLiveNow": false,
                "startTimestamp": "2026-05-01T12:00:00Z",
                "endTimestamp": "2026-05-01T13:00:00Z"
            }
        }}
    });
    let s = parse_broadcast_status(&ended);
    assert_eq!(s.is_live_now, Some(false));
    assert_eq!(s.end_ms, Some(1_777_640_400_000));

    let scheduled = serde_json::json!({
        "videoDetails": {"videoId": "c"},
        "microformat": {"playerMicroformatRenderer": {
            "liveBroadcastDetails": {
                "isLiveNow": false,
                "startTimestamp": "2028-03-21T15:00:00+00:00",
                "scheduledStartTimestamp": "2028-03-21T16:00:00+00:00"
            }
        }}
    });
    let s = parse_broadcast_status(&scheduled);
    assert_eq!(s.scheduled_start_ms, Some(1_837_267_200_000));

    // 通常動画は liveBroadcastDetails を持たない
    let vod = serde_json::json!({"videoDetails": {"videoId": "dQw4w9WgXcQ"}});
    let s = parse_broadcast_status(&vod);
    assert!(!s.has_details && !s.is_upcoming && s.is_live_now.is_none());
}

/// FR-53/CD・FR-56/CG: epoch ミリ秒 → UTC の ISO 表記。
/// `iso8601_ms` の逆変換として往復で確認する。
#[test]
fn iso_utc_roundtrip() {
    assert_eq!(iso_utc(0), "1970-01-01T00:00:00+00:00");
    assert_eq!(iso_utc(1_777_636_800_000), "2026-05-01T12:00:00+00:00");
    // 負側も div_euclid で日付へ落ちる（epoch 未満）
    assert_eq!(iso_utc(-86_400_000), "1969-12-31T00:00:00+00:00");
    // ミリ秒未満は切り捨て（表記精度は秒まで）
    assert_eq!(iso_utc(999), "1970-01-01T00:00:00+00:00");
    for s in [
        "1970-01-01T00:00:00Z",
        "2026-05-01T12:00:00Z",
        "2019-12-08T14:02:06-08:00",
    ] {
        let ms = iso8601_ms(s).unwrap();
        assert_eq!(iso8601_ms(&iso_utc(ms)), Some(ms));
    }
}

/// FR-53/CD・FR-56/CG: golden fixture（実取得のプレミア応答）から
/// メタ一式を切り出す。publishDate は -08:00 付きなので
/// 保存形式の "+00:00" へ正規化される。isLiveContent=false +
/// liveBroadcastDetails の組み合わせはプレミア判定の材料になる。
#[test]
fn parse_meta_snapshot_golden_fixture() {
    let v: Value =
        serde_json::from_str(include_str!("../../tests/fixtures/youtube_player.json")).unwrap();
    let s = parse_meta_snapshot(&v);
    assert_eq!(s.published_at.as_deref(), Some("2019-12-08T22:02:06+00:00"));
    assert_eq!(s.is_live_content, Some(false));
    assert_eq!(s.playability.as_deref(), Some("UNPLAYABLE"));
    assert!(s.broadcast.has_details);
    assert_eq!(s.broadcast.is_live_now, Some(false));
    assert!(s.broadcast.start_ms.is_some() && s.broadcast.end_ms.is_some());
    // FR-85/DJ: 実応答の shortDescription が切り出せる
    assert!(s.description.contains("https://fanlink.tv/lofigirl-music"));
}

/// FR-53/CD: publishDate 優先・uploadDate フォールバック・欠落は None。
/// 解釈不能な文字列は保持せず None へ倒す。
#[test]
fn parse_meta_snapshot_published_at_variants() {
    let wrap = |micro: serde_json::Value| serde_json::json!({"microformat": {"playerMicroformatRenderer": micro}});
    // publishDate 優先
    let s = parse_meta_snapshot(&wrap(serde_json::json!({
        "publishDate": "2026-01-02T03:04:05+00:00",
        "uploadDate": "2020-01-01T00:00:00+00:00"
    })));
    assert_eq!(s.published_at.as_deref(), Some("2026-01-02T03:04:05+00:00"));
    // publishDate 欠落は uploadDate
    let s = parse_meta_snapshot(&wrap(serde_json::json!({
        "uploadDate": "2020-01-01T00:00:00+00:00"
    })));
    assert_eq!(s.published_at.as_deref(), Some("2020-01-01T00:00:00+00:00"));
    // 解釈不能・欠落は None
    assert!(parse_meta_snapshot(&wrap(serde_json::json!({
        "publishDate": "not a date"
    })))
    .published_at
    .is_none());
    assert!(parse_meta_snapshot(&serde_json::json!({}))
        .published_at
        .is_none());
}

/// FR-56/CG: 再生状態と isLiveContent の切り出し。
/// 欠落時は None のまま（判定側で欠落扱いにする）。
#[test]
fn parse_meta_snapshot_state_fields() {
    let s = parse_meta_snapshot(&serde_json::json!({
        "playabilityStatus": {"status": "LIVE_STREAM_OFFLINE"},
        "videoDetails": {"videoId": "a", "isLiveContent": true}
    }));
    assert_eq!(s.playability.as_deref(), Some("LIVE_STREAM_OFFLINE"));
    assert_eq!(s.is_live_content, Some(true));
    let s = parse_meta_snapshot(&serde_json::json!({"videoDetails": {"videoId": "b"}}));
    assert!(s.playability.is_none() && s.is_live_content.is_none());
    // FR-85/DJ: shortDescription の欠落は空文字列に潰す
    // （解決済み・説明なしの区別は呼び出し側が空文字の書き込みで行う）
    assert_eq!(s.description, "");
    let s = parse_meta_snapshot(&serde_json::json!({
        "videoDetails": {"videoId": "c", "shortDescription": "概要\n2 行目"}
    }));
    assert_eq!(s.description, "概要\n2 行目");
}
