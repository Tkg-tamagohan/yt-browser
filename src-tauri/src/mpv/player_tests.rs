use super::{mpv_args, ArmedQueue, LoadRetryState, SpawnOptions};
use crate::model::QueueArmItem;
use serde_json::json;
use std::path::Path;

/// mpv_args 検証用の既定 SpawnOptions。各テストで必要なフィールドだけ
/// 上書きして使う。
fn base_spawn_opts() -> SpawnOptions {
    SpawnOptions {
        video_id: "vid00000000".to_string(),
        start_sec: 0.0,
        ytdl_format: "best".to_string(),
        ytdlp_path: None,
        po_token: None,
        wheel_script: None,
        wheel_volume_delta: None,
        pip_geometry: None,
        pip_fit_aspect: true,
        tone_mapping: None,
        hdr_compute_peak: None,
        extra_args: None,
        screen: None,
        network_timeout_ms: 0,
        hwdec: "auto-safe".to_string(),
        log_dir: None,
        initial_speed: 1.0,
        initial_volume: 100.0,
    }
}

fn arm(uid: i64, video_id: &str) -> QueueArmItem {
    QueueArmItem {
        uid,
        video_id: video_id.to_string(),
    }
}

/// 武装キュー: 取り出しは uid＋video_id の対をそのまま返す
/// （BG-10、仕様決定 BK。同一動画でも uid で区別できる）。
#[test]
fn armed_queue_take_next_returns_uid_pair() {
    let mut q = ArmedQueue::default();
    assert!(q.set(vec![arm(1, "a"), arm(2, "a")], false, None, None));
    let first = q.take_next().unwrap();
    assert_eq!((first.uid, first.video_id.as_str()), (1, "a"));
    let second = q.take_next().unwrap();
    assert_eq!((second.uid, second.video_id.as_str()), (2, "a"));
    assert!(q.take_next().is_none());
}

/// 武装キュー: loop_all は取り出した項目を同一 uid のまま末尾へ戻す。
#[test]
fn armed_queue_loop_requeues_same_item() {
    let mut q = ArmedQueue::default();
    assert!(q.set(vec![arm(7, "x")], true, None, None));
    for _ in 0..3 {
        let item = q.take_next().unwrap();
        assert_eq!((item.uid, item.video_id.as_str()), (7, "x"));
    }
    assert_eq!(q.items.len(), 1);
}

/// 武装キュー: base_seq が現在世代と食い違う置き換えは拒否され、
/// 一致すれば適用される（仕様決定 AD）。空からの取り出しは世代を進めない。
#[test]
fn armed_queue_set_respects_base_seq() {
    let mut q = ArmedQueue::default();
    assert!(q.set(vec![arm(1, "a"), arm(2, "b")], false, Some(0), None));
    q.take_next().unwrap();
    // seq は 1 へ進み、古い世代に基づく置き換えは拒否
    assert!(!q.set(vec![arm(9, "z")], false, Some(0), None));
    assert_eq!(q.items.len(), 1);
    // 現在世代に基づく置き換えは適用（seq はリセットしない）
    assert!(q.set(vec![arm(9, "z")], false, Some(1), None));
    assert_eq!(q.items.len(), 1);
    assert_eq!(q.seq, 1);
}

/// ロード失敗の自動再送: 1 ロードにつき 1 回だけ予約できる（BG-9、仕様決定 BJ）。
#[test]
fn load_retry_allows_one_retry_per_load() {
    let mut s = LoadRetryState::default();
    s.arm("u1", json!({}));
    assert!(s.begin_retry().is_some());
    // 同じロードへの 2 回目の予約はできない（無限再試行の防止）
    assert!(s.begin_retry().is_none());
}

/// ロード失敗の自動再送: 新しい loadfile の発行で再送回数がリセットされる。
#[test]
fn load_retry_rearms_on_new_load() {
    let mut s = LoadRetryState::default();
    s.arm("u1", json!({}));
    assert!(s.begin_retry().is_some());
    s.arm("u2", json!({}));
    assert!(s.begin_retry().is_some());
}

/// ロード失敗の自動再送: 世代が違う古い予約の再送対象は返さない。
#[test]
fn load_retry_rejects_stale_generation() {
    let mut s = LoadRetryState::default();
    s.arm("u1", json!({"start": "1"}));
    let gen = s.begin_retry().unwrap();
    // 遅延中に別のロードが発行されたら、古い世代の再送は成立しない
    s.arm("u2", json!({"start": "2"}));
    assert!(s.spec_for(gen).is_none());
    // 同じ世代なら記録したパラメータを返す
    let gen2 = s.begin_retry().unwrap();
    let (url, opts) = s.spec_for(gen2).unwrap();
    assert_eq!(url, "u2");
    assert_eq!(opts["start"], "2");
}

/// ロード失敗の自動再送: loadfile を一度も発行していなければ予約できない。
#[test]
fn load_retry_requires_armed_spec() {
    let mut s = LoadRetryState::default();
    assert!(s.begin_retry().is_none());
}

/// スクリーンショットのファイル名規約（FR-38、仕様決定 BN）。
/// `ytb-<videoId>-<yyyymmddHHMMSS>.png`。
#[test]
fn screenshot_file_name_format() {
    assert_eq!(
        super::screenshot_file_name("dQw4w9WgXcQ", "20260115143022"),
        "ytb-dQw4w9WgXcQ-20260115143022.png"
    );
}

/// 字幕トラックの抽出（FR-40、仕様決定 BP）。
/// `type=sub` だけを残し、id の取れないエントリは落とす。
#[test]
fn sub_tracks_filters_sub_only() {
    let list = json!([
        {"id": 1, "type": "video", "title": "v", "lang": null},
        {"id": 2, "type": "audio", "lang": "ja"},
        {"id": 3, "type": "sub", "title": "English", "lang": "en"},
        {"id": 4, "type": "sub", "lang": "ja"},
        {"id": 5, "type": "sub"},
        {"type": "sub", "title": "no id"}
    ]);
    let subs = super::sub_tracks_of(&list);
    assert_eq!(subs.len(), 3);
    assert_eq!(subs[0].id, 3);
    assert_eq!(subs[0].title.as_deref(), Some("English"));
    assert_eq!(subs[0].lang.as_deref(), Some("en"));
    assert_eq!(subs[1].id, 4);
    assert_eq!(subs[1].title, None);
    assert_eq!(subs[1].lang.as_deref(), Some("ja"));
    assert_eq!(subs[2].id, 5);
    assert_eq!(subs[2].title, None);
    assert_eq!(subs[2].lang, None);
}

/// 字幕トラックの抽出: 配列でない・空の入力は空を返す（FR-40）。
#[test]
fn sub_tracks_of_handles_bad_input() {
    assert!(super::sub_tracks_of(&json!(null)).is_empty());
    assert!(super::sub_tracks_of(&json!([])).is_empty());
    assert!(super::sub_tracks_of(&json!("x")).is_empty());
}

/// `sid` プロパティ値の正規化（FR-40）。
/// false→"no"、文字列はそのまま、数値は ID 文字列、取れない入力は空。
#[test]
fn sid_normalizes_values() {
    assert_eq!(super::sid_of(&json!(false)), "no");
    assert_eq!(super::sid_of(&json!(null)), "no");
    assert_eq!(super::sid_of(&json!("auto")), "auto");
    assert_eq!(super::sid_of(&json!("no")), "no");
    assert_eq!(super::sid_of(&json!(3)), "3");
    assert_eq!(super::sid_of(&json!({})), "");
}

/// `sub_track` アクションの sid 文字列の解釈（FR-40）。
/// "no"/"auto" は文字列、数字は ID 数値、その他は None。
#[test]
fn parse_sid_values() {
    assert_eq!(super::parse_sid("no"), Some(json!("no")));
    assert_eq!(super::parse_sid("auto"), Some(json!("auto")));
    assert_eq!(super::parse_sid("3"), Some(json!(3)));
    assert_eq!(super::parse_sid("abc"), None);
    assert_eq!(super::parse_sid("-1"), None);
    assert_eq!(super::parse_sid(""), None);
    assert_eq!(super::parse_sid("1.5"), None);
}

/// DB-PO-03: PO Token の mpv 起動引数への構成（FR-86、仕様決定 DK）。
/// `ytdlp.po_token` 設定値は `--ytdl-raw-options=extractor-args=youtube:po_token=<値>`
/// として mpv へ渡り、ytdl_hook 経由の yt-dlp 呼び出しへ伝達される。
/// 既存引数（--ytdl-format・--script-opts 等）との併存と、
/// 未設定時に引数を付けないことを確認する。トークン値は仮値で検証する。
#[test]
fn mpv_args_passes_po_token_via_ytdl_raw_options() {
    let socket = Path::new("sock");
    // 設定あり: ytdl-raw-options に extractor-args が載る
    let mut opts = base_spawn_opts();
    opts.po_token = Some("TOKEN_VALUE".to_string());
    opts.ytdlp_path = Some("yt-dlp".to_string());
    let args = mpv_args(&opts, socket, None);
    assert!(args
        .contains(&"--ytdl-raw-options=extractor-args=youtube:po_token=TOKEN_VALUE".to_string()));
    // 既存の ytdl 系引数と併存する
    assert!(args.iter().any(|a| a.starts_with("--ytdl-format=")));
    assert!(args
        .iter()
        .any(|a| a.starts_with("--script-opts=") && a.contains("ytdl_hook-ytdl_path=")));
    // 未設定・空・空白のみなら ytdl-raw-options を付けない
    for token in [None, Some(String::new()), Some("   ".to_string())] {
        let mut opts = base_spawn_opts();
        opts.po_token = token;
        let args = mpv_args(&opts, socket, None);
        assert!(args.iter().all(|a| !a.starts_with("--ytdl-raw-options=")));
    }
}

/// SponsorBlock シーク: EOF 直前に予約されたスキップが load_video による切替後に
/// 実行機会を得た場合、世代不一致でシーク送信を抑止する。
#[tokio::test]
async fn sponsor_skip_aborts_after_eof_video_switch() {
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::{Arc, Mutex};

    let sponsor = Arc::new(Mutex::new(super::SponsorState::default()));
    let sponsor_gen = Arc::new(AtomicU64::new(0));
    let switch_op = Arc::new(tokio::sync::Mutex::new(()));

    // 切替開始（load_video 相当）が先に世代を進め、ロックを保持している状態を作る
    let switch_guard = switch_op.lock().await;
    sponsor_gen.fetch_add(1, Ordering::SeqCst);

    let seek_sent = Arc::new(AtomicBool::new(false));
    let skip_task = {
        let sponsor = Arc::clone(&sponsor);
        let sponsor_gen = Arc::clone(&sponsor_gen);
        let switch_op = Arc::clone(&switch_op);
        let seek_sent = Arc::clone(&seek_sent);
        tokio::spawn(async move {
            super::execute_sponsor_skip(&sponsor, &sponsor_gen, &switch_op, 0, 0, || async move {
                seek_sent.store(true, Ordering::SeqCst);
                Ok::<(), &'static str>(())
            })
            .await
        })
    };

    // 切替を完了してロックを解放したあと、予約済みスキップは stale 終了する
    *super::lock(&sponsor) = super::SponsorState::default();
    drop(switch_guard);

    let outcome = skip_task.await.unwrap();
    assert_eq!(outcome, super::SponsorSkipOutcome::Stale);
    assert!(!seek_sent.load(Ordering::SeqCst));
    assert!(super::lock(&sponsor).fired.is_empty());
}

/// SponsorBlock シーク: 先行してシーク IPC 送信に入ったスキップがある場合、
/// load_video の loadfile 相当処理はスキップシーク完了まで待機する。
#[tokio::test]
async fn sponsor_skip_inflight_finishes_before_video_switch_load() {
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::{Arc, Mutex};

    let sponsor = Arc::new(Mutex::new(super::SponsorState::default()));
    let sponsor_gen = Arc::new(AtomicU64::new(0));
    let switch_op = Arc::new(tokio::sync::Mutex::new(()));

    let seek_entered = Arc::new(tokio::sync::Notify::new());
    let allow_seek_finish = Arc::new(tokio::sync::Notify::new());
    let loadfile_ran = Arc::new(AtomicBool::new(false));
    let seek_finished_before_load = Arc::new(AtomicBool::new(false));

    let skip_task = {
        let sponsor = Arc::clone(&sponsor);
        let sponsor_gen = Arc::clone(&sponsor_gen);
        let switch_op = Arc::clone(&switch_op);
        let seek_entered = Arc::clone(&seek_entered);
        let allow_seek_finish = Arc::clone(&allow_seek_finish);
        let loadfile_ran = Arc::clone(&loadfile_ran);
        let seek_finished_before_load = Arc::clone(&seek_finished_before_load);
        tokio::spawn(async move {
            super::execute_sponsor_skip(&sponsor, &sponsor_gen, &switch_op, 0, 0, || async move {
                seek_entered.notify_one();
                allow_seek_finish.notified().await;
                seek_finished_before_load
                    .store(!loadfile_ran.load(Ordering::SeqCst), Ordering::SeqCst);
                Ok::<(), &'static str>(())
            })
            .await
        })
    };

    seek_entered.notified().await;

    let switch_task = {
        let sponsor_gen = Arc::clone(&sponsor_gen);
        let switch_op = Arc::clone(&switch_op);
        let loadfile_ran = Arc::clone(&loadfile_ran);
        tokio::spawn(async move {
            sponsor_gen.fetch_add(1, Ordering::SeqCst);
            let _g = switch_op.lock().await;
            loadfile_ran.store(true, Ordering::SeqCst);
        })
    };

    tokio::task::yield_now().await;
    assert!(!loadfile_ran.load(Ordering::SeqCst));

    allow_seek_finish.notify_one();
    let outcome = skip_task.await.unwrap();
    switch_task.await.unwrap();

    assert!(seek_finished_before_load.load(Ordering::SeqCst));
    assert!(loadfile_ran.load(Ordering::SeqCst));
    assert_eq!(outcome, super::SponsorSkipOutcome::Stale);
}

/// SponsorBlock 取得: 動画切替中は `video_id` 更新前でも世代不一致により
/// 旧動画の取得結果が `SponsorState` へ混入しない（ループで同 ID に戻った場合も弾く）。
#[test]
fn sponsor_fetch_rejected_while_video_switch_in_progress() {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Mutex;

    let sponsor = Mutex::new(super::SponsorState::default());
    let sponsor_gen = AtomicU64::new(0);
    let gen_old = sponsor_gen.load(Ordering::Acquire);

    // load_video("vid_new") が sponsor 初期化・世代更新まで進め、
    // loadfile_replace().await 待機中で video_id はまだ "vid_old" の状態
    sponsor_gen.fetch_add(1, Ordering::SeqCst);
    {
        let mut sp = super::lock(&sponsor);
        sponsor_gen.fetch_add(1, Ordering::SeqCst);
        *sp = super::SponsorState::default();
    }
    let gen_new = sponsor_gen.load(Ordering::Acquire);

    let old_seg = vec![crate::sponsor::ActiveSegment {
        category: "sponsor".to_string(),
        start: 10.0,
        end: 20.0,
        action: crate::sponsor::CategoryAction::Skip,
    }];
    let new_seg = vec![crate::sponsor::ActiveSegment {
        category: "sponsor".to_string(),
        start: 30.0,
        end: 40.0,
        action: crate::sponsor::CategoryAction::Skip,
    }];

    // video_id がまだ旧動画でも、世代不一致で旧取得結果は拒否される
    assert!(!super::apply_sponsor_segments_if_current(
        &sponsor,
        &sponsor_gen,
        "vid_old",
        gen_old,
        "vid_old",
        old_seg.clone(),
    ));
    assert!(super::lock(&sponsor).segments.is_empty());

    // 切替後の正しい動画・世代では反映される
    assert!(super::apply_sponsor_segments_if_current(
        &sponsor,
        &sponsor_gen,
        "vid_new",
        gen_new,
        "vid_new",
        new_seg,
    ));
    assert_eq!(super::lock(&sponsor).segments.len(), 1);
    assert_eq!(super::lock(&sponsor).segments[0].start, 30.0);

    // ループ等で再度 "vid_old" へ戻った場合でも古い gen_old の取得結果は拒否される
    assert!(!super::apply_sponsor_segments_if_current(
        &sponsor,
        &sponsor_gen,
        "vid_old",
        gen_old,
        "vid_old",
        old_seg,
    ));
    assert_eq!(super::lock(&sponsor).segments[0].start, 30.0);
}

/// DB-ED-11: 終端印待機ヘルパの非同期挙動（FR-95、仕様決定 EA）。
/// 終端印の先行到着・遅延到着・新規読み込みによる世代切替打ち切り・切断即時終了を検証する。
#[tokio::test]
async fn wait_for_load_error_snapshot_handles_marker_generation_and_disconnect() {
    use super::super::error_diag::YtdlErrorBuffer;
    use super::wait_for_load_error_snapshot;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use tokio::sync::watch;

    // 1. 終端印が先行到着済みなら待機せず即座にスナップショットを返す
    let buf = Arc::new(Mutex::new(YtdlErrorBuffer::default()));
    let (sig_tx, _) = watch::channel(0u64);
    let gen = Arc::new(AtomicU64::new(1));
    {
        let mut g = super::lock(&buf);
        g.push_log_message("ytdl_hook", "ERROR: Requested format is not available\n");
        g.push_log_message(
            "ytdl_hook",
            "youtube-dl failed: unexpected error occurred\n",
        );
    }
    let snap = wait_for_load_error_snapshot(&buf, &sig_tx, &gen, 1, Duration::from_secs(5))
        .await
        .expect("終端印到着済みなのでスナップショットを返すべき");
    assert_eq!(snap.len(), 2);

    // 2. 終端印が遅延到着した場合、到着通知で起床して追加行を含めて返す
    let buf2 = Arc::new(Mutex::new(YtdlErrorBuffer::default()));
    let (sig_tx2, _) = watch::channel(0u64);
    let gen2 = Arc::new(AtomicU64::new(2));
    let buf2_bg = buf2.clone();
    let sig_tx2_bg = sig_tx2.clone();
    let waiter = tokio::spawn(async move {
        wait_for_load_error_snapshot(&buf2, &sig_tx2, &gen2, 2, Duration::from_secs(5)).await
    });
    tokio::task::yield_now().await;
    {
        let mut g = super::lock(&buf2_bg);
        g.push_log_message(
            "ytdl_hook",
            "[debug] WARNING: [youtube] abcdefghijk: n challenge solving failed\n",
        );
        assert!(g.push_log_message(
            "ytdl_hook",
            "youtube-dl failed: unexpected error occurred\n"
        ));
    }
    sig_tx2_bg.send_modify(|v| *v = v.wrapping_add(1));
    let snap2 = waiter
        .await
        .unwrap()
        .expect("遅延終端印の到着でスナップショットを返すべき");
    assert_eq!(snap2.len(), 2);

    // 3. 待機中に新規ロードで load_gen が進んだら None を返して打ち切る（旧動画の終端送出防止）
    let buf3 = Arc::new(Mutex::new(YtdlErrorBuffer::default()));
    let (sig_tx3, _) = watch::channel(0u64);
    let gen3 = Arc::new(AtomicU64::new(10));
    let gen3_bg = gen3.clone();
    let sig_tx3_bg = sig_tx3.clone();
    let waiter3 = tokio::spawn(async move {
        wait_for_load_error_snapshot(&buf3, &sig_tx3, &gen3, 10, Duration::from_secs(5)).await
    });
    tokio::task::yield_now().await;
    gen3_bg.fetch_add(1, Ordering::SeqCst);
    sig_tx3_bg.send_modify(|v| *v = v.wrapping_add(1));
    assert!(
        waiter3.await.unwrap().is_none(),
        "世代切替時は旧ロードの分類を打ち切って None を返すべき"
    );

    // 4. 待機中に IPC 切断が起きたらタイムアウトを待たず現時点の行を返す
    let buf4 = Arc::new(Mutex::new(YtdlErrorBuffer::default()));
    let (sig_tx4, _) = watch::channel(0u64);
    let gen4 = Arc::new(AtomicU64::new(20));
    super::lock(&buf4).push_log_message("ytdl_hook", "ERROR: Unable to download webpage\n");
    let buf4_bg = buf4.clone();
    let sig_tx4_bg = sig_tx4.clone();
    let waiter4 = tokio::spawn(async move {
        wait_for_load_error_snapshot(&buf4, &sig_tx4, &gen4, 20, Duration::from_secs(5)).await
    });
    tokio::task::yield_now().await;
    super::lock(&buf4_bg).mark_disconnected();
    sig_tx4_bg.send_modify(|v| *v = v.wrapping_add(1));
    let snap4 = waiter4.await.unwrap().expect("切断時は既存行を返すべき");
    assert_eq!(snap4, vec!["ERROR: Unable to download webpage"]);
    assert!(super::lock(&buf4_bg).is_disconnected());
}

/// DB-ED-12: 初回と自動再送で異なる原因の行が出たとき、再送時の終端印リセット後に
/// 両方の行を合算して優先順位どおりに分類される（受け入れ基準 4）。
#[tokio::test]
async fn retry_combines_first_and_second_attempt_logs() {
    use super::super::error_diag::{classify, PlaybackErrorKind, YtdlErrorBuffer};
    use super::{should_retry_load_error, wait_for_load_error_snapshot};
    use std::sync::atomic::AtomicU64;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use tokio::sync::watch;

    let buf = Arc::new(Mutex::new(YtdlErrorBuffer::default()));
    let (sig_tx, _) = watch::channel(0u64);
    let gen = Arc::new(AtomicU64::new(1));
    let mut retry = LoadRetryState::default();
    retry.arm("https://www.youtube.com/watch?v=jNQXAC9IVRw", json!({}));

    // 初回失敗: network エラー
    {
        let mut g = super::lock(&buf);
        g.push_log_message(
            "ytdl_hook",
            "ERROR: Unable to download API page: timed out\n",
        );
        g.push_log_message(
            "ytdl_hook",
            "youtube-dl failed: unexpected error occurred\n",
        );
    }
    let first_snap = wait_for_load_error_snapshot(&buf, &sig_tx, &gen, 1, Duration::from_secs(1))
        .await
        .unwrap();
    let first = classify(&first_snap);
    assert_eq!(first.kind, PlaybackErrorKind::Network);
    assert!(should_retry_load_error(first.kind));

    // 1 回目の再送予約: 行は保持したまま終端印だけリセット
    let retry_gen = retry.begin_retry().expect("初回失敗は再送予約できるべき");
    assert!(retry.spec_for(retry_gen).is_some());
    super::lock(&buf).reset_end_marker_for_retry();
    assert!(!super::lock(&buf).is_ready_to_classify());

    // 再送失敗: JS チャレンジ失敗警告 + フォーマットなし
    {
        let mut g = super::lock(&buf);
        g.push_log_message(
            "ytdl_hook",
            "[debug] WARNING: [youtube] jNQXAC9IVRw: n challenge solving failed\n",
        );
        g.push_log_message("ytdl_hook", "ERROR: Requested format is not available\n");
        g.push_log_message(
            "ytdl_hook",
            "youtube-dl failed: unexpected error occurred\n",
        );
    }
    let combined_snap =
        wait_for_load_error_snapshot(&buf, &sig_tx, &gen, 1, Duration::from_secs(1))
            .await
            .unwrap();
    // 初回の network 行と再送の js_challenge/no_format 行が両方含まれる
    assert!(combined_snap.iter().any(|l| l.contains("timed out")));
    assert!(combined_snap
        .iter()
        .any(|l| l.contains("challenge solving failed")));
    assert_eq!(
        classify(&combined_snap).kind,
        PlaybackErrorKind::JsChallenge
    );
    // 2 回目の再送予約は拒否され最終失敗となる
    assert!(retry.begin_retry().is_none());
}

/// DB-ED-13: `video_unavailable` では自動再送を省略し、他の 6 カテゴリでは再送を許容する（受け入れ基準 4、仕様決定 EA）。
#[test]
fn video_unavailable_skips_auto_retry_while_other_kinds_retry() {
    use super::super::error_diag::PlaybackErrorKind;
    use super::should_retry_load_error;

    assert!(!should_retry_load_error(
        PlaybackErrorKind::VideoUnavailable
    ));
    for kind in [
        PlaybackErrorKind::CookieFailed,
        PlaybackErrorKind::Blocked,
        PlaybackErrorKind::JsChallenge,
        PlaybackErrorKind::NoFormat,
        PlaybackErrorKind::Network,
        PlaybackErrorKind::Unknown,
    ] {
        assert!(
            should_retry_load_error(kind),
            "{kind:?} は自動再送の対象であるべき"
        );
    }
}

/// DB-ED-14: 連続再生キューの 3 経路（連続失敗、`--keep-open=yes` の `eof-reached` 経由の
/// 正常 → 失敗 → 正常、および `--keep-open=no` の `end-file(eof)` 経由の正常 → 失敗 → 正常）で、
/// `prepare_terminal_for_continue` を通して置換予約が次項目の読み込み失敗を誤吸収せず、
/// いずれのエラー終端も診断とキュー進行へ届く（FR-95）。
#[test]
fn consecutive_error_queue_items_both_trigger_terminal() {
    use super::super::terminal::TerminalTracker;
    use super::prepare_terminal_for_continue;

    // 1. エラー → エラー（連続失敗）
    let tracker = TerminalTracker::default();
    assert!(tracker.on_end_file("error"));
    assert!(!tracker.completed());
    assert!(!prepare_terminal_for_continue(&tracker, "error", false));
    assert!(tracker.on_end_file("error"));
    assert!(!tracker.completed());

    // 2. --keep-open=yes: eof-reached(kept_open=true) → 旧 end-file(stop) → 次項目エラー → 正常
    let t2 = TerminalTracker::default();
    assert!(t2.on_eof());
    assert!(t2.completed());
    assert!(prepare_terminal_for_continue(&t2, "eof", true));
    // 旧ファイルアンロード時の end-file(stop) は replace 予約で吸収される
    assert!(!t2.on_end_file("stop"));
    // 次項目が file-loaded 前にエラー終端 → 抑止されず受理される
    assert!(t2.on_end_file("error"));
    assert!(!t2.completed());
    assert!(!prepare_terminal_for_continue(&t2, "error", false));
    // 3 件目が正常にロードされて EOF 到達
    t2.on_file_loaded();
    assert!(t2.on_eof());
    assert!(t2.completed());

    // 3. --keep-open=no（mpv.extra_args 等）: end-file(eof, kept_open=false) → 次項目エラー → 正常
    // 旧ファイルは既にアンロード済みのため replace 予約を張らず、次項目のエラーが吸収されない
    let t3 = TerminalTracker::default();
    assert!(t3.on_end_file("eof"));
    assert!(t3.completed());
    assert!(!prepare_terminal_for_continue(&t3, "eof", false));
    // end-file(stop) を挟まず次項目の end-file(error) が届いても即座に受理される
    assert!(t3.on_end_file("error"));
    assert!(!t3.completed());
    assert!(!prepare_terminal_for_continue(&t3, "error", false));
    t3.on_file_loaded();
    assert!(t3.on_end_file("eof"));
    assert!(t3.completed());
}

/// DB-ED-16: `ArmedQueue::take_next_for_end` が uid ベースで現行ループ項目の失敗のみを数え、
/// 1 項目ループ・全項目失敗ループ・単独ループモード切替後の失敗での無限再読み込みを防ぎつつ、
/// 初回ロード中や遷移後の削除で残る 1 件（同一動画 ID の別 uid を含む）、部分成功ループ、
/// 非ループ有限キューでは継続を保つ（FR-95、仕様決定 EA）。
#[test]
fn armed_queue_stops_infinite_error_loop_on_loop_all() {
    // 1. 1 項目ループ（[A], loop_all=true）で A がエラー終端 → 即座に None（追加 loadfile なし）
    let mut one = ArmedQueue::default();
    assert!(one.set(vec![arm(1, "vidA")], true, None, Some("vidA")));
    assert!(one.take_next_for_end("error", "vidA").is_none());
    assert_eq!(one.seq, 0);

    // 2. 2 項目ループ（[B, A], loop_all=true、A 再生中）で全項目が連続失敗 → B だけ試して 2 回目で停止
    let mut all_bad = ArmedQueue::default();
    assert!(all_bad.set(
        vec![arm(2, "vidB"), arm(1, "vidA")],
        true,
        None,
        Some("vidA")
    ));
    let first = all_bad
        .take_next_for_end("error", "vidA")
        .expect("1 回目のエラーでは別項目 B へ進むべき");
    assert_eq!((first.uid, first.video_id.as_str()), (2, "vidB"));
    assert!(
        all_bad.take_next_for_end("error", "vidB").is_none(),
        "全項目が連続失敗したら無限ループを止めるべき"
    );

    // 3. 全体ループ中に読み込み中の項目 A を削除して残り 1 件 B になった後で A が失敗
    //    → 削除済み A の失敗は B へ数えず、未再生の B へ進む
    let mut deleted_loading = ArmedQueue::default();
    assert!(deleted_loading.set(
        vec![arm(2, "vidB"), arm(1, "vidA")],
        true,
        None,
        Some("vidA")
    ));
    assert!(deleted_loading.set(vec![arm(2, "vidB")], true, Some(0), Some("vidA")));
    let next_b = deleted_loading
        .take_next_for_end("error", "vidA")
        .expect("削除された項目 A の失敗で残り 1 件 B を打ち切ってはいけない");
    assert_eq!((next_b.uid, next_b.video_id.as_str()), (2, "vidB"));
    // その後 B 自身も失敗した場合は現行ループ全件失敗として停止する
    assert!(deleted_loading.take_next_for_end("error", "vidB").is_none());

    // 4. 初回ロード中に同一動画 ID の重複登録（uid=1 と uid=2）から uid=1 を削除して uid=2 が残った場合
    //    → 初回 set で捕捉した current_uid=1 の失敗は残存 uid=2 へ誤算入されず、uid=2 を試してから停止する
    let mut init_dup_deleted = ArmedQueue::default();
    assert!(init_dup_deleted.set(
        vec![arm(2, "vidA"), arm(1, "vidA")],
        true,
        None,
        Some("vidA")
    ));
    assert!(init_dup_deleted.set(vec![arm(2, "vidA")], true, Some(0), Some("vidA")));
    let next_init_dup = init_dup_deleted
        .take_next_for_end("error", "vidA")
        .expect("初回ロード中に削除された uid=1 の失敗で残存 uid=2 を打ち切ってはいけない");
    assert_eq!(next_init_dup.uid, 2);
    assert!(init_dup_deleted
        .take_next_for_end("error", "vidA")
        .is_none());

    // 5. 遷移後の同一動画 ID 重複登録で uid=1 を削除して uid=2 が残った場合も uid=2 を試してから停止する
    let mut dup_deleted = ArmedQueue::default();
    assert!(dup_deleted.set(
        vec![arm(1, "vidA"), arm(2, "vidA")],
        true,
        None,
        Some("vid0")
    ));
    let cur = dup_deleted.take_next_for_end("eof", "vid0").unwrap();
    assert_eq!(cur.uid, 1);
    assert!(dup_deleted.set(vec![arm(2, "vidA")], true, Some(1), Some("vidA")));
    let next_dup = dup_deleted
        .take_next_for_end("error", "vidA")
        .expect("削除された uid=1 の失敗で残存 uid=2 を打ち切ってはいけない");
    assert_eq!(next_dup.uid, 2);
    assert!(dup_deleted.take_next_for_end("error", "vidA").is_none());

    // 6. 単独ループで一周後に全体ループ → 1 項目ループへ切り替えても同一 uid(10) が維持され、
    //    直後の画質変更失敗でもう一巡再読み込みせず即停止する
    let mut standalone = ArmedQueue::default();
    assert!(standalone.set(vec![arm(10, "vidA")], true, None, Some("vidA")));
    let looped = standalone.take_next_for_end("eof", "vidA").unwrap();
    assert_eq!(looped.uid, 10);
    // 1 項目ループ切替後も同一 uid=10 で再武装される（QS-05 と対になる）
    assert!(standalone.set(vec![arm(10, "vidA")], true, Some(1), Some("vidA")));
    assert!(
        standalone.take_next_for_end("error", "vidA").is_none(),
        "同一 uid を維持した単独ループの失敗は再読み込みせず即停止すべき"
    );

    // 7. 2 項目ループで A 失敗 → B ロード成功（reset_error_streak）→ B 正常終了 → A 失敗 → 再度 B へ進める
    let mut partial = ArmedQueue::default();
    assert!(partial.set(
        vec![arm(2, "vidB"), arm(1, "vidA")],
        true,
        None,
        Some("vidA")
    ));
    let b = partial.take_next_for_end("error", "vidA").unwrap();
    assert_eq!(b.video_id, "vidB");
    partial.reset_error_streak();
    let a = partial.take_next_for_end("eof", "vidB").unwrap();
    assert_eq!(a.video_id, "vidA");
    let b2 = partial
        .take_next_for_end("error", "vidA")
        .expect("直前に正常再生を挟んでいれば再び B へ進めるべき");
    assert_eq!(b2.video_id, "vidB");

    // 8. 非ループ（loop_all=false）の有限キューは全項目を順に消化して空で終了する
    let mut finite = ArmedQueue::default();
    assert!(finite.set(vec![arm(2, "vidB"), arm(3, "vidC")], false, None, None));
    assert_eq!(
        finite.take_next_for_end("error", "vidA").unwrap().video_id,
        "vidB"
    );
    assert_eq!(
        finite.take_next_for_end("error", "vidB").unwrap().video_id,
        "vidC"
    );
    assert!(finite.take_next_for_end("error", "vidC").is_none());
}

/// DB-ED-15: `prepare_mpv_log_file` が親ディレクトリを用意し、Unix 環境では
/// 他ユーザーから読めない `0600` 権限で mpv ログファイルを事前作成・引き締めする（FR-95）。
#[test]
fn prepare_mpv_log_file_restricts_permissions() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nested").join("mpv-1-1-1.log");
    super::prepare_mpv_log_file(&path);
    assert!(path.parent().unwrap().is_dir());

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "新規作成された mpv ログは 0600 であるべき");

        // 既に 0644 で存在していても 0600 へ引き締める
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        super::prepare_mpv_log_file(&path);
        let mode2 = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode2, 0o600, "既存の mpv ログも 0600 へ引き締めるべき");
    }
}

/// DB-ED-17: `handle_load_error` の 200ms 待機中や自動再送の 1.5 秒待機中に `switch` や画質変更で
/// `prepare_replace` が呼ばれても、アンロード済み mpv に対して不要な `pending_replaces` を積まず
/// 旧 `ended_reason` も解除し、差し替え先の `end-file(error)` / `Disconnected` を飲み込まない（FR-95）。
#[test]
fn prepare_replace_handles_unloaded_error_and_retry_windows() {
    use super::super::terminal::TerminalTracker;

    // 1. handle_load_error の 200ms 待機中（end-file(error) 受理直後・ended_reason=Some("error")）に
    //    switch / 画質変更が割り込んだ場合
    let t1 = TerminalTracker::default();
    assert!(t1.on_end_file("error"));
    assert!(
        !t1.prepare_replace(),
        "既に end-file(error) でアンロード済みのため pending_replaces を積んではいけない"
    );
    assert!(
        t1.on_end_file("error"),
        "差し替え先動画の読み込み失敗が飲み込まれず受理されるべき"
    );

    // 2. 自動再送の 1.5 秒待機中（ended_reason は解除済みだが未だ loadfile 再送前）に
    //    switch / 画質変更が割り込んだ場合
    let t2 = TerminalTracker::default();
    assert!(t2.on_end_file("error"));
    t2.clear_ended_reason();
    assert!(
        !t2.prepare_replace(),
        "再送待機中で旧ファイル未ロードのため pending_replaces を積んではいけない"
    );
    assert!(
        t2.on_disconnect(),
        "差し替え後の切断イベントも旧 ended_reason に阻まれず受理されるべき"
    );

    // 3. 自動再送の loadfile を実際に送った後（mark_loading_started）や再生中・keep-open EOF 後は
    //    従来どおり pending_replaces を 1 積み、旧 end-file(stop) を消化して次失敗を受理する
    let t3 = TerminalTracker::default();
    assert!(t3.on_end_file("error"));
    t3.clear_ended_reason();
    t3.mark_loading_started();
    assert!(
        t3.prepare_replace(),
        "再送開始後はアクティブ読み込み中のため pending_replaces を積むべき"
    );
    assert!(!t3.on_end_file("stop"));
    assert!(t3.on_end_file("error"));

    // 4. prepare_replace 後に loadfile_replace 自体が失敗したときのロールバック
    let t4 = TerminalTracker::default();
    assert!(t4.on_end_file("error"));
    let armed = t4.prepare_replace();
    assert!(!armed);
    t4.cancel_prepared_replace(armed);
    assert!(
        !t4.prepare_replace(),
        "アンロード済み状態へのロールバック後も不要な pending_replaces を積まないべき"
    );

    // 5. --keep-open=no の EOF 後に画質変更して初回ロードが失敗した場合の一連の遷移:
    //    end-file(eof) → rearm_after_eof → prepare_replace(false) → arm → 初回失敗
    //    → 1 回だけ再送予約・実行許可（can_execute_load_retry + spec_for）→ 2 回目は再送打ち切り
    let t5 = TerminalTracker::default();
    let mut retry = super::LoadRetryState::default();
    assert!(t5.on_end_file("eof"));
    t5.rearm_after_eof();
    assert!(
        !t5.prepare_replace(),
        "--keep-open=no の EOF 後はアンロード済みのため pending_replaces を積まない"
    );
    let url = "https://www.youtube.com/watch?v=dQw4w9WgXcQ";
    let opts = serde_json::json!({ "start": "0" });
    retry.arm(url, opts.clone());
    assert!(t5.on_end_file("error"));
    let retry_gen = retry
        .begin_retry()
        .expect("初回ロード失敗で 1 回だけ自動再送を予約できるべき");
    t5.clear_ended_reason();
    assert!(super::can_execute_load_retry(3, 3, false));
    assert_eq!(retry.spec_for(retry_gen), Some((url.to_string(), opts)));
    t5.mark_loading_started();
    // 世代不一致または IPC 切断時は再送を実行しない
    assert!(!super::can_execute_load_retry(4, 3, false));
    assert!(!super::can_execute_load_retry(3, 3, true));
    // 再送も失敗した場合は 2 回目の再送を行わず終端へ進む
    assert!(t5.on_end_file("error"));
    assert!(retry.begin_retry().is_none());
}
