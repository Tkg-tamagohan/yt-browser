# yt-browser 実装計画

[requirements-definition.md](requirements-definition.md) と [design.md](design.md) に基づくフェーズ別の実装計画。
各フェーズはおおむね 1 PR 単位を目安とし、終了時に受け入れ条件を満たすことをチェックする。

## 前提と環境

- 開発と検証の第一環境は Linux の Devin VM。
  mpv と yt-dlp は `apt` / `pip` / バイナリで入れられる
- Windows ビルドは Windows 環境が必要になるため、必要になった時点で Windows 子セッションまたはユーザー側で検証する
- GUI 実機検証は Devin VM の Chrome ではなく mpv / Tauri 本体の起動確認になるため、画面確認はコンピュータツール経由で行う

## フェーズ

Phase 0〜81 の完了済み記録は [implementation-history.md](implementation-history.md) に分離した。

### Phase 82：監査対応の安定化とリーク解消（Step 1）

- [x] `PlayerCards.svelte` の `control()` が `Promise<boolean>` を返すように改め、音声出力や字幕、画質変更失敗時に `<select>` 表示を元へ戻し、`formatOverrides` の誤登録を防ぐ
- [x] `PlayerCards.svelte`、`chat/+page.svelte`、`deeplink.svelte.ts`、`shortcuts.svelte.ts`、`feed/+page.svelte`、`library/+page.svelte`、`+page.svelte`、`ui-prefs.ts` に破棄後の遅延解決ガードとタイマーおよびイベント購読解除を入れる
- [x] `shortcuts.svelte.ts` の一覧選択から非表示要素配下の行を除外する（`shortcuts-logic.ts` へ `isVisibleRow` を切り出し、`KS-07` テストを追加する）
- [x] `bookmark.svelte.ts`、`livewait.svelte.ts`、`chat.svelte.ts`、`players.svelte.ts`、`queue.svelte.ts` の初期化失敗時に途中登録済みの購読を解除して `initPromise` をリセットし、次回呼び出し時の再試行を可能にする（`init-retry.test.ts` を追加する）
- [x] Rust 側の非テストコードから生の `.lock().unwrap()`、`.state()` 直参照、`snap.as_ref().expect()`、および `InnerTube::post_json` の `unreachable!()` を排し、`crate::util::lock` と `try_state` 等へ統一する
- [x] `mpv/devices.rs` に 5 秒タイムアウトと `kill_on_drop`、`innertube/mod.rs` に HTTP タイムアウトと `ytcfg` 取得の直列化ガードを追加し、`deep_link.rs`、`innertube/tests.rs`、`devices.rs` に回帰テストを追加する
- [x] `mpv/player.rs`、`spawn.rs`、`ipc.rs`、`manager.rs` で `shutdown` および `cleanup_failed_spawn` の子プロセス回収、`IpcClient` リーダー終了、並列 `close_all`、`event_pump` からの PiP 再適用および SponsorBlock スキップ切り出し、`Full` 溢れ警告とチャネル拡張を入れる
- [x] 開発ビルドの Native Messaging 登録制御を `YB_DEV_NO_NM=1` 環境変数へ移し、`scripts/e2e_dev.py` の `lib.rs` 書き換えと `.e2e-backup` 退避を廃止する
- [x] 受け入れ：単体テスト、ビルド、整合チェックがすべて通過することを確認する

### Phase 83：応答性改善、DB ロック競合対策、拡張の耐障害性強化（Step 2a）

- [x] `db/mod.rs` と `native_host.rs` の読み取り専用接続へ SQLite `busy_timeout(5000ms)` を設定し、マイグレーション `v23`（`idx_videos_meta_resolve`）を追加して `docs/design.md` §8 と同期する
- [x] `playlist_add_many` の挿入位置採番を 1 回取得の加算（$O(N)$）へ改め、`channel_delete`・`videos_set_kind`・`videos_mark_read`・`feed_auto_read` をトランザクション化する
- [x] `data_export`・`data_import`・`db_vacuum`・`db_cleanup_orphans`・`history_stats`・`library_search`・`playlist_import`（DB 部分）・`diag::export_bundle` の同期重い処理を `tokio::task::spawn_blocking` へ逃がす
- [x] `VideoActions.svelte` の行ごと `<VideoActionsMenu>` 描画を廃止してレイアウト常時マウントの `VideoContextMenu` へ統合し、`settings/+page.svelte` の初期読み込みを `Promise.all` 並列化＋一部失敗通知に改め、検索デバウンス（`library-page.svelte.ts` / `+page.svelte`）と通知重複排除・タイマー掃除を入れる
- [x] Chrome 拡張の Native Messaging 呼び出しへ 5 秒タイムアウトと `settled` ガード、タブ単位失敗バッジ、`popup` の直近失敗理由表示、`tabs.onUpdated` のイベント絞り込みを入れる
- [x] 受け入れ：単体テスト、ビルド、整合チェックがすべて通過することを確認する

### Phase 84：構造整理、共通ヘルパー集約、スクリプト堅牢化（Step 2b）

- [x] Rust 側のオン/オフ設定値パーサを `crate::util::{setting_on_enabled, setting_off_disabled}` へ集約し、`deps.rs`・`feed/mod.rs`・`mpv/mod.rs`・`chat/popup.rs` から委譲する
- [x] `yt/mod.rs` の `yt-dlp` 実行コマンド呼び出し 7 箇所を終了ステータス検査付きの共通ヘルパー `run_checked` へ統合する
- [x] フロントエンド側の設定値パーサを `feed-settings.ts` の `onOffEnabled` / `offDisabledEnabled` へ集約し、`pip.ts`・`eof-logic.ts`・`chat-popup-logic.ts`・`shortcuts-logic.ts` から委譲する
- [x] `players.svelte.ts` に `isUiErrorCode` を追加して `settings/+page.svelte` のエラーコード判定を置き換え、`i18n.test.ts`（`LN-01`）へ `ja` / `en` の `{placeholder}` 一致検証を追加する
- [x] `scripts/make_latest_json.py` に複数成果物・空署名・空バージョン拒否および UTF-8 読み込みを入れ、`scripts/bump_version.py` に `replace_cargo_lock_version` フォールバックと CRLF/LF 両対応の `Cargo.lock` 一致処理を入れる
- [x] 受け入れ：単体テスト、ビルド、整合チェックがすべて通過することを確認する

### Phase 85：Native Messaging ローカル IPC 化と実装計画書の履歴分離（Step 2c）

- [x] `native_host.rs` と `lib.rs` に本体側ローカル IPC 待受（Windows 名前付きパイプ `\\.\pipe\io.github.tkg-tamagohan.yt-browser-nm-ipc` / Linux Unix ソケット `<app_data_dir>/native-ipc.sock`）を追加し、ホストモードに 3 段配送（ローカル IPC → 本体直接 `spawn` → `open::that_detached` フォールバック）を実装する（`DB-NM-17`・`DB-NM-18`）
- [x] `docs/implementation-plan.md` から Phase 0〜81 を `docs/implementation-history.md` へ分離し、解消済みの残課題重複記述を整理する
- [x] `docs/design.md`（§3.4.1・§11）・`README.md`・`docs/features.md`・`scripts/check_docs_consistency.py`（Phase 見出し連番整合検証を含む）を更新する
- [x] 受け入れ：Chrome / Edge 拡張からのローカル IPC 起動と未起動時直接 `spawn` が実機で動作することを確認する（`DB-NM-17`・`DB-NM-18` の自動テスト・ビルド・整合チェックはすべて通過。Windows 上の Edge 拡張から起動中インスタンスへ名前付きパイプ経由で `open` / `queue` / `playlist_add` が配信され（`list_playlists` はホストが本体 DB を直接読む経路として確認）、未起動時はホストが本体バイナリを直接 `spawn` して動画再生まで到達することを実機で確認）

### Phase 86：同期コマンドの非同期化、ローカル IPC 権限強化、テストモジュール分離（Step 3）

- [x] `commands/` 配下で `State<Db>` を同期で触る Tauri コマンドを `async` 化し、DB ロック保持が重い経路（`list_feed`・`mark_read`・履歴・プレイリスト・キュー保存・フィルタ更新）は `tokio::task::spawn_blocking` へ移す。コマンド名・引数名・戻り値型と `design.md` §3.1 のシグネチャは変えない
- [x] ローカル IPC を堅牢化する。Windows 名前付きパイプに `reject_remote_clients(true)` を付け、Unix ソケットを `0600` に制限し（`DB-NM-19`）、終了時にソケット残骸を消す `cleanup_ipc_endpoint` を `RunEvent::Exit` で呼ぶ（`DB-NM-20`）
- [x] `mpv/player.rs` と `native_host.rs` のテストモジュールを `#[path]` 付き外部ファイルへ分離し、テスト ID と private 項目へのアクセスを維持する
- [x] `feed` / `metaresolve` の `detect_client` に `connect_timeout(5s)` を付け、`security.yml` に `cargo audit` ジョブを追加する
- [x] 受け入れ：lint・単体テスト・ビルド・整合チェックがすべて通過することを確認する（Windows dev ビルドで非同期化した全コマンドを実 UI から操作確認し、UiError・ハング・ログの ERROR/WARN なしを実機で検証済み）

### Phase 87：上部ナビゲーション追従、設定右サイドバー保存、未保存離脱確認と再生時遷移の設定化

- [x] 全タブ共通の上部ナビゲーション（`<nav class="app-nav">`）を `position: sticky` で上端追従させ、実測高さ `--app-nav-height` を共有して `QueuePanel` と重ならないようにする（FR-89、仕様決定 DR）
- [x] 設定画面を 2 カラム化し、右サイドバーに「保存」「変更を戻す」ボタン、変更差分リスト（セクション名、項目名、`変更前 → 変更後`、PO Token マスク）、通知欄を常時配置する（FR-90、仕様決定 DS）
- [x] 設定画面の未保存離脱確認を `ui.confirm_unsaved`（既定 `on`）で選択可能にし、`beforeNavigate` と 3 択確認モーダル（保存して移動、破棄して移動、キャンセル）を実装する（FR-91、仕様決定 DT）
- [x] 再生開始時の再生画面 `/` への自動遷移を `player.navigate_on_play`（既定 `on`）で選択可能にし、再生開始とライブ待機通知「今すぐ再生」の遷移先を `navigateOnPlay()` に統一する（FR-92、仕様決定 DU）
- [x] 受け入れ：全タブでヘッダー追従、設定右サイドバーでの差分表示とリセットおよび保存、未保存離脱確認の 3 択分岐、再生画面遷移オプトアウトを実機 UI で確認する（自動テスト、lint、ビルド、docs 整合チェックは通過済み。Windows 上の WebView2 dev インスタンス（CDP）で、全タブの sticky ヘッダー追従と実測高さ `--app-nav-height` の反映、右サイドバーの差分表示・リセット・保存および PO Token の `***` マスク、3 択モーダルの全分岐と `ui.confirm_unsaved=off` 時の直接遷移、未保存変更ありでの再読み込み時のブラウザ標準離脱抑止、`player.navigate_on_play` の ON/OFF 両設定での再生開始時遷移の有無を実機確認）

### Phase 88：配信終了および再生終了時のチャットポップアップ自動クローズ

- [x] `chat.popup_close_on_end`（既定 `on`）を設定キーとして定義し、`chat/popup.rs` へ解釈関数、終了判定（`should_close_popup_on_player_end`）、フェイルセーフ付き DB 読み取り、および単体テスト（`DB-CP-07`、`DB-CP-08`、`DB-CP-10`）を実装する（FR-93、仕様決定 DV）
- [x] `ChatPoller::run_live` へ `is_live_continuation_ended` とライブ継続終了時（`None` および `Replay` 遷移）のポップアップ自動クローズを組み込み、単体テスト `DB-CP-09` を追加する
- [x] `PlayerManager` へ `has_other_video_id`（単体テスト `DB-CP-11`）と `VideoEndHook` を設け、`close`、`spawn_emitter`（終了時および別動画への連続再生時）、`switch`（別動画読み替え成功後）からロック解放後に `lib.rs` 経由で `chat::popup::handle_player_video_end` を呼ぶよう組み込む
- [x] 設定画面「チャットポップアップ」節、`chat-popup-logic.ts`、`settings-diff-logic.ts`、`settings-filter.ts`、日英 i18n リソースと各単体テスト（`CP-05`、`SD-02`、`SF-02`）を追加する
- [ ] 受け入れ：設定画面での切替と即時反映、ライブ終了および再生終了（EOF 自動クローズ、手動クローズ、複数プレイヤー残存維持、連続再生と `player_switch` での別動画切替、リプレイ末尾維持、`off` 時維持）を実機 UI で確認する（自動テスト、lint、ビルド、docs 整合チェックは通過済み、GUI 実機検証は未実施）

### Phase 89：ダンプ境界保護と再同期、視聴履歴チャンネル補完、フィード索引最適化（v24）

- [x] ダンプ境界保護とランタイムおよび UI 再同期を実装する。`LOCAL_PROTECTED_SETTINGS`（`ytdlp.po_token`、`ytdlp.path`、`mpv.extra_args`、`setup.deps_shown`）を `export_dump` から除外し、`import_dump` で単一トランザクション内にローカル既存値を退避復元しつつ（行不在の未設定時は非注入、空文字既存行は保持）ダンプ側の同 4 キーを読み飛ばす。`validate_dump` では検証ループ先頭で保護 4 キーを除外してから `is_channel_id`、`is_video_id`、設定キー 128 バイトおよび値 16 KiB 上限、プレイリスト名 150 文字上限（トリム保存）を強制し、`ExportPlaylistItem.channel_id` の非 UC 値のみ `None` へ正規化する。`data_import` から非保護設定差分への `settings://changed` 送出、`pip.fit_aspect` 即時反映、`FeedPoller::sync_after_import`、`MetaResolver::wake_now` を実行し、フロント側では `SettingsOpsSection` でインポート前後に `beginImportQueueGuard` / `endImportQueueGuard` による旧プレイリストキュー永続化の世代ガード＋抑止制御（失敗時は保留分リプレイ、成功時は破棄）を入れつつ DB サイズ再取得と保護 4 キー案内表示、`handleImported` からのカテゴリ再取得および `refreshQueues` を組み込む（FR-94、仕様決定 DX、単体テスト `DB-LD-25`、`DB-LD-26`、`DB-LD-31`、`DB-LD-32`、`DB-LD-33`、`QS-03`）
- [x] 視聴履歴のチャンネル補完と `v24` バックフィルを実装する。`history_upsert` と `history_update_progress` で既存非空値を保持しつつ `videos` → `live_waits` → `queue_items`（`channel_title` のみ）の SQL 補完を行い、ライブ待機自動再生時は削除前 `LiveWait` の `HistoryChannelHint` を `PlayerManager::play_with_channel_hint` 経由で `VideoStartHook` 発火前に引き継ぐ。順序非依存 UPSERT の `history_set_channel`（`history_stats` の `channel_id` 未設定グループは `channel_title = NULL` で合算）と `PlayerManager::VideoStartHook`（`play`、`switch`、連続再生 `continued: true` の 3 契機でロック解放後に発火）を追加し、`lib.rs` から同一 `video_id` の in-flight 重複抑止付きで `InnerTube::video_meta` → 15 秒タイムアウト付き `yt::video_channel_ref`（同一 JSON から参照候補・`channel_title`・`title` を同時抽出し、参照候補が無く `channel_title` のみ取れた場合も保持）の共通ヘルパ（`playing_channel` 第 3 段と共用）を注入する（仕様決定 DY、単体テスト `DB-LD-34`、`DB-LD-35`）
- [x] SQLite マイグレーション `v24`（`watch_history` バックフィル、`idx_videos_unread`、`idx_videos_feed_pub`、`idx_videos_channel_pub` 更新）と `docs/design.md` §8 DDL 同期、および `feed_list_filtered` の 4 分岐 SQL 最適化を実装し、`ANALYZE` 前後の全 4 分岐で `SCAN v` 素走査と `USE TEMP B-TREE` を排除する（単体テスト `DB-LD-35`、`DB-LD-36`）
- [x] プレイリスト名上限 `MAX_PLAYLIST_NAME_LEN` を 150 文字へ引き上げ、`playlist_import` の `name: None` 分岐で前後空白トリム後の 150 文字単純切り詰めを適用する（仕様決定 DZ、単体テスト `DB-LD-37`）
- [ ] 受け入れ：隔離 dev インスタンス（`YB_DEV_NO_NM=1`、CDP）でダンプ出力と再取込後の保護 4 キー保持とカテゴリ、`sponsor.show_segments`（シークバー区間帯表示）、DB サイズ再同期、および購読動画と未購読動画の再生後の履歴検索とチャンネル別視聴統計を実機 UI で確認する（期待結果：`#ops-import-btn` から有効ダンプを取り込んだ際、画面リロードなしで DB サイズ表示・カテゴリ一覧・プレイリストキュー一覧がダンプ内容へ更新され、ローカル保護 4 キーが維持されること。自動テスト、lint、ビルド、docs 整合チェックに加え、隔離 dev インスタンスでの `scripts/e2e_smoke.py`（9 項目）、`data_export` 除外と `data_import` 保護 4 キー保持、`settings://changed` 差分送出とリロードなしテーマ反映、`play_video` 音量 0 実再生による購読動画および未購読動画の `VideoStartHook` 補完、履歴チャンネル名検索、統計タブのチャンネル別集計表示までは実機確認済み。OS ファイル選択ダイアログを伴う `#ops-import-btn` 押下経由の `handleImported` 連動のみ手動確認が残る）

### Phase 90：再生開始失敗の原因別エラー通知とログ診断

- [x] `src-tauri/src/mpv/error_diag.rs` を新設し、`ytdl_hook` 由来行の選別とリングバッファ保持（1 インスタンスあたり 50 行かつ UTF-8 16 KiB 上限、古い行から破棄）、7 カテゴリの優先順位分類、許可リスト方式の `errorDetail`、および URL、引用符、各種パス、`=` 右辺の伏せ字処理と 200 文字切り詰めを実装する（FR-95、仕様決定 EA、単体テスト `DB-ED-01` 〜 `DB-ED-08`）
- [x] `mpv/spawn.rs` の `launch` へ `configure_ytdl_logging`（`request_log_messages("error")` と `change-list ytdl-raw-options append verbose=`、失敗時は警告のみで続行）を組み込み、`mpv/player.rs` と `mpv/terminal.rs`、`mpv/manager.rs`、`model/mod.rs` で `event_pump` 非停止の非同期終端印待機（最大 200ms、新規ロードやポンプ停止の世代更新で打ち切り、切断で即時終了）、初回と自動再送の行合算、`video_unavailable` での 1 回再送省略、連続再生次項目への終端理由整備（`kept_open` による `begin_replace` 切替）とアンロード済み待機中の動画切替・画質変更での終端保護（`prepare_replace`）、ループ無限エラー再読み込み抑止、Unix `mpv-*.log` の `0600` 事前作成、および `player://ended` への `errorKind` / `errorDetail` 付与を実装する（単体テスト `DB-ED-09` 〜 `DB-ED-17`）
- [x] `commands/diag.rs` と `lib.rs` へアプリログディレクトリだけを安全に開く専用コマンド `diag_open_log_dir` を追加する
- [x] フロントエンドへ `playback-error-logic.ts`（日英 7 カテゴリ文言と後方互換、未知値フォールバック、単体テスト `PE-01` 〜 `PE-05`）、`notices.svelte.ts` と `+layout.svelte` の 12 秒表示オプションおよび手動閉じるボタン（`×`）とタイマー解放、`PlayerCards.svelte` の導線配線、`settings/+page.svelte` の `/settings?section=deps` スクロール遷移とクエリ消費、および日英 i18n キーを実装する
- [ ] 受け入れ：`--verbose` 有無の URL 解決時間計測（10 回ずつ、中央値差 -3ms を確認済み）、現行 `mpv` での IPC `log-message` 先着と `--verbose` / `--extractor-args` 展開および正常再生時 0 件確認（確認済み）、および隔離 dev インスタンス（`scripts/e2e_dev.py`、CDP）での `scripts/e2e_smoke.py`（9 項目）、実 `play_video`（`00000000000`）による `video_unavailable` 即時通知と `×` ボタン消去、別画面から `/settings?section=deps` 遷移時の `loadSettings` 完了後スクロールと同一画面フィルタ解除後スクロール、通知履歴記録、英語 UI 切替時の通知表示までは確認済み。`deno` 無効化・配信開始前・オフラインの実機再現および OS ファイルマネージャのログフォルダ起動（`diag_open_log_dir`）の目視確認のみ未実施

### Phase 91：フィード画面のライブ中・配信予定検出表示と上部配置（v25）

- [x] `innertube/mod.rs` へ `classify_broadcast_meta`（`<= 0` 開始時刻正規化、プレミア公開含む）、`is_transient_or_blocked_playability`、`InnerTubeError::is_rate_limited`、`classify_kind` 集約を追加する（FR-96、仕様決定 EB、単体テスト `DB-IT-01`、`DB-IT-02`）
- [x] SQLite マイグレーション `v25`（`broadcast_state`・`broadcast_start_ms`・`broadcast_checked_at_ms` 追加、部分索引 `idx_videos_broadcast_active`・`idx_videos_feed_broadcast`、直近 14 日 `upcoming`・直近 24 時間 `live`/`premiere` バックフィル）、`model/mod.rs`（`FeedItem`・`FeedCursor.is_live`・`FeedFilter.kind`）、`db/feed.rs`（2 段階 `feed_list_filtered`・`feed_list_broadcast_sql`・`videos_resolve_meta_tx`・`videos_expire_stale_upcoming`・`broadcast_refresh_targets`・`videos_apply_broadcast_refresh`・`videos_touch_broadcast_checked`）を実装する（単体テスト `DB-LD-38` 〜 `DB-LD-41`）
- [x] `metaresolve/mod.rs` へ定期再確認（緊急度 8:2 配分、7 日超過打ち切り、HTTP 429 指数バックオフ、連続 3 回失敗中断、連続 6 回ボット応答個別解除、手動 60 秒クールダウン `wake_manual`）を実装し、`livewait/mod.rs` から `meta_snapshot` 相乗り同期（`ingested_only`）、`commands/feed.rs` の `feed_refresh` から `wake_manual` 呼び出しを組み込む（単体テスト `DB-MR-12`、`DB-LW-07`）
- [x] フロントエンドへ `fmtBroadcastDateTime`、`feed-page-logic.ts`（ライブ中優先全順序、`broadcastDisplayOf`、`mergeFeedReload` / `mergeFeedLoadMore`）、`routes/feed/+page.svelte`（`live_now` / `upcoming` フィルタ、バッジ・開始時刻描画）、日英 i18n キー 7 件を実装する（単体テスト `FMT-01`、`FP-04` 〜 `FP-06`）
- [ ] 受け入れ：実際のライブ配信中・配信予定（時刻あり/未定）・終了済みアーカイブ・プレミア公開を含む購読チャンネルで、ライブ中上部表示、バッジ・開始時刻表示、6 種別フィルタ、チャンネル別グループ表示、手動「今すぐ更新」、英語 UI、開始/終了後の自動追従を実機 UI で確認する（自動テスト、lint、ビルド、docs 整合チェックは通過済み、実配信を用いた GUI 実機検証は未実施）

### Phase 92：YouTube Data API v3 クライアント基盤と秘匿保護

- [x] `innertube/mod.rs` から `BroadcastStatus`、`MetaSnapshot`、`classify_kind`、`classify_broadcast_meta` を `model/mod.rs` へ、`iso_utc` と `iso8601_ms` を `util.rs` へ移設し、`util.rs` へ ISO 8601 継続時間パーサ `parse_iso8601_duration`（`P0D`、`PT0S`、`P#DT#H#M#S`、オーバーフロー検査付き）と `model/mod.rs` へ UI DTO（`YtApiAvailability`、`YtApiStatus`、`YtApiTestOutcome`）を追加する（FR-97、仕様決定 EC）
- [x] 純粋リーフモジュール `ytapi/mod.rs`（`crate::model` と `crate::util` のみ参照）を新設し、定数固定の本番 URL、`X-Goog-Api-Key`（`set_sensitive(true)`、URL クエリ禁止）、カスタム `Debug` と `.without_url()`、接続 5 秒および全体 8 秒タイムアウト、2 系統サーキットブレーカー（`shared` 枠 10,000 units と `search` 枠 100 回の独立遮断、米国 DST 考慮の次回 `00:00 America/Los_Angeles` プラス 60 秒解除）、キー無効ラッチ（`HTTP 400` `keyInvalid`、`HTTP 401`、`HTTP 403`）、段階クールダウン（`HTTP 429`、`5xx`、通信失敗）、`HTTP 404` 非遮断、トリム後文字列変化時のみリセットするホットリロード、`videos.list` 1 unit の `test_connection`（候補キー試行時の保存済み遮断器非破壊）を実装する（単体テスト `DB-YA-01` 〜 `DB-YA-08`）
- [x] 6 箇所の秘匿保護と IPC コマンドを実装する
  `db/export.rs` の `LOCAL_PROTECTED_SETTINGS` を `youtube.api_key` を加えた 5 キーへ拡張し（`commands/system.rs` 一致テスト `DB-LD-42`、保護動作 `DB-LD-31`、`DB-LD-32`）、`diag.rs` の設定断片（`api_key`、`apikey`）、ヘッダ正規表現（`x-goog-api-key`、`x-api-key`）、JSON 秘匿名（`x-goog-api-key`、`x-api-key`）を更新し（単体テスト `DB-DG-10`、`DB-DG-11`）、`commands/system.rs` と `lib.rs` へ `ytapi_status`、`ytapi_test_connection`、`settings://changed` の秘匿値 `""` マスク、`settings_set` および `data_import` の `on_key_changed` 連動を組み込む
- [x] フロントエンドへ `SettingsDepsSection.svelte` の API キー入力欄（単一 `<input>` の `type` 切替）、状態バッジ表示、1 unit 接続テストボタン、および `settings-diff-logic.ts`（`***` マスク）、`settings/+page.svelte`（`onCommitted` 内 `savedSnapshot.youtubeApiKey` 更新）、`settings-filter.ts`、日英 i18n キーと各単体テスト（`SD-06`、`SF-03`、`SV-05`、`LN-04`）を実装する
- [ ] 受け入れ：設定画面「依存コマンド」節で API キーの入力、表示切替、保存差分マスク、状態バッジ表示、1 unit 接続テスト、データエクスポート除外および診断 zip マスクを実機 UI で確認する（自動テスト、lint、ビルド、docs 整合チェック、モックサーバー検証は実装 PR で通過確認、実 API キーを用いた GUI 実機確認は未実施）

### Phase 93：YouTube Data API v3 のメタ遅延解決とライブ待機および単発メタ取得への優先適用

- [x] `util.rs` の `parse_iso8601_duration` と `ytapi/mod.rs` の `can_use` と `get_json` の未使用許可を外し、`YtApi::videos_list`（重複除去と最大 50 件チャンクの `GET /videos?part=snippet,contentDetails,liveStreamingDetails,statistics` 呼び出し、1 unit 消費、`+00:00` 正規化、サムネイル優先選択、`classify_kind_for_initial` / `classify_kind_for_refresh` によるガード 4、フォールバック判定 `should_fallback`）を実装する（FR-98、仕様決定 ED、単体テスト `DB-YA-09` 〜 `DB-YA-12`）
- [x] `db/feed.rs` の `videos_resolve_meta_tx` と `apply_broadcast_columns_tx` で既存 `cur_kind == "premiere"` を `None` や `"live"` で上書きしないよう SQL と分岐で保護し、`metaresolve/mod.rs` へ `Arc<YtApi>` を配線して共通枠利用可能時は期限切れ予定解除後に `FR-96` 再確認対象（最大 10 件）と未解決対象（`fails < 5`）を最大 50 件 1 バッチで照会し、ガード 2（`HTTP 200` 欠落時に `meta_checked=1` や空説明文の書込と毎 tick フォールバックを行わず `fails` 加算と `KIND_FAILED` に留める）、`JoinSet` 並列 `shorts_probe`、ガード 3（`live` と `upcoming` 双方の 3 回連続欠落で `broadcast_state = None` 解除）、および `InnerTube` 透過フォールバックを実装する（単体テスト `DB-MR-13` 〜 `DB-MR-16`）
- [x] `livewait/mod.rs` へ `Arc<YtApi>` を配線し、`poll_due` で存在確認と 24 時間超過解除後の `active_due` を 50 件チャンク単位で `ytapi.videos_list` 巡回（利用不可またはエラー時はチャンク単位で `InnerTube` フォールバック、単発 `HTTP 200` 欠落では `live_wait` 行と `broadcast_state` を保持して次回へ繰越、`ingested = 1` 同期では `premiere` を保護）する処理を実装する（単体テスト `DB-LW-08` 〜 `DB-LW-10`）
- [x] 単発 4 経路（`commands/search.rs` の `get_description`、`commands/feed.rs` の `resolve_video_channel` と `playing_channel`、`commands/queue.rs` の `video_meta_resolve`、`commands/livewait.rs` の `live_wait_add`）および `lib.rs` の `VideoStartHook` と常駐ワーカー生成へ `Arc<YtApi>` の優先配線と透過フォールバックを組み込み、設定画面の案内文言（`i18n-ja.ts` と `i18n-en.ts`）と 7 文書を更新する
- [ ] 受け入れ：有効な API キーを設定した実機環境でメタ遅延解決の 50 件バッチ化、ライブ待機巡回、概要欄と動画メタとチャンネル解決の公式 API 優先動作、およびクォータ上限または無効キー時の `InnerTube` と `yt-dlp` への透過フォールバックを確認する（自動テスト、lint、ビルド、docs 整合チェック、モックサーバー検証は実装 PR で通過確認、実 API キーを用いた実機確認は未実施）

### Phase 94：YouTube Data API v3 による @handle チャンネル購読とプレイリスト取り込み

- [x] `ytapi/mod.rs` へ `PLAYLIST_ITEMS_PAGE_SIZE = 50`、`PLAYLIST_ITEMS_MAX_PAGES = 100`、`YtPlaylistItem`、`YtPlaylist`、`resolve_channel_handle`（`GET /channels?part=id&forHandle`、1 unit、`is_channel_id` 検証、空 `items` は `Ok(None)`）、および `playlist_items`（`fetch_title == true` 時のみ `GET /playlists?part=snippet`、空 `items` は `playlistItems.list` を呼ばず `NotFound`、`GET /playlistItems?part=snippet,contentDetails,status` を `seen_tokens` ループ抑止付き最大 100 ページ巡回、`public` と `unlisted` のみ採用、`videoOwnerChannelId` の `is_channel_id` 検証、`videoPublishedAt` の `iso8601_ms` と `iso_utc` による `+00:00` 正規化）を実装する（FR-99、仕様決定 EE、単体テスト `DB-YA-13` 〜 `DB-YA-16`）
- [x] `commands/feed.rs` の `resolve_channel_ref` と `subscribe_channel` へ `Arc<YtApi>` を注入し、`@handle` 購読時に `ytapi.resolve_channel_handle` を優先して `yt-dlp` 不在環境でも購読を完結させ、空 `items` と `HTTP 404` は `FeedError::NotFound` で即エラーとし、未設定や無効キーやクォータ上限や一時障害時のみ `yt::channel_id` へフォールバックする経路を実装する（単体テスト `DB-PF-14`）
- [x] `db/library.rs` と `db/mod.rs` へ `PlaylistImportItem`、`video_upsert_with_published`（`COALESCE(videos.published_at, excluded.published_at)` で既存非 NULL 保持）、`playlist_add_many_with_published` を追加し、`commands/library.rs` の `import_playlist_with` と `playlist_import` で `WL`、`HL`、`LM`、`LL` 系、`RD` 系は `yt::playlist_meta` へ直行し、それ以外の `list` 付きプレイリストは `ytapi.playlist_items` を優先して `NotFound` と有効 0 件を `invalid_input`（非フォールバック）とし、その他障害時のみ `yt::playlist_meta` へフォールバックする処理、ならびに設定画面案内文言（`i18n-ja.ts` と `i18n-en.ts`）と 7 文書を実装する（単体テスト `DB-LD-43`）
- [ ] 受け入れ：有効な API キーを設定した実機環境で `@handle` チャンネル購読（`yt-dlp` なしでも成功すること、存在しないハンドルで `NotFound` になること）と標準プレイリスト取り込み（投稿日時の補完と非公開項目の除外、`WL` 等のフォールバック、クォータ上限時の `yt-dlp` 切替）を実機 UI で確認する（自動テスト、lint、ビルド、docs 整合チェック、モックサーバー検証は実装 PR で通過確認、実 API キーを用いた GUI 実機確認は未実施）

### Phase 95：YouTube Data API v3 によるキーワード検索と動画説明文フィルタ補完

- [x] `ytapi/mod.rs` へ `SearchSession`（10 分 TTL、`SEARCH_MAX_PAGES_PER_CALL = 2`、トリム後キー変化時のみリセットする `on_key_changed` と `session_epoch` 保護）、`YtVideoItem.view_count`（`statistics.viewCount` 解析）、`YtVideoItem::to_search_result`（`is_channel_id` 厳格化）、および `search_videos` / `search_videos_at`（`search.list` `part=id`・`type=video`・`maxResults=50`・`relatedToVideoId` 非送信・`seen_tokens` ループ抑止・`HashSet` 重複排除・`videos_list` 併用・`Search` と `Shared` 独立遮断器）を実装する（FR-100、FR-25、仕様決定 EF、単体テスト `DB-YA-17` 〜 `DB-YA-21`）
- [x] `filter/mod.rs` へ `Matcher::has_video_desc_rules`、`has_channel_id_rules`、`has_channel_title_rules`（有効な `literal` と空でない `RegexSet` のみを対象とし `highlight` と不正正規表現のみの空集合を除外）を追加し、`yt/mod.rs` の `parse_search_jsonl` へ `truncate(count)` 前段の `HashSet` 重複排除を組み込む
- [x] `commands/search.rs` の `search` と `search_with` へ `Arc<YtApi>` を注入して `SearchSession` を優先配線し（`yt-dlp` 不在でも動作し、累積 `count` と既存 IPC 署名を維持）、`should_fallback` 時は `yt::search` へ透過フォールバックしたうえで事前フィルタ後の候補に対して条件付き（`QuotaBucket::Shared` 利用可能かつ `has_video_desc_rules` 真、または `channel_id` / `channel_title` 欠落かつ対応ブロックや NG ルール有効）50 件チャンク `videos_list` 補強と `survivors >= count` 早期打ち切りを行う（FR-85 改訂）
- [x] 設定画面の API キー案内文言（`i18n-ja.ts` と `i18n-en.ts` の `settings.deps.youtubeApiKeyDesc`）から検索順次対応予定の文言を除いてキーワード検索を追加し、7 文書（`requirements-definition.md`・`decision-records.md`・`design.md`・`implementation-plan.md`・`features.md`・`user-guide.md`・`README.md`）を更新する
- [ ] 受け入れ：有効な API キーを設定した実機環境でキーワード検索（`yt-dlp` 不在環境での検索成功、「さらに読み込む」時の 10 分 TTL セッション再利用、`video_desc` NG フィルタの評価、検索枠と共通枠の独立フォールバック）を実機 UI で確認する（自動テスト、lint、ビルド、docs 整合チェック、モックサーバー検証は実装 PR で通過確認、実 API キーを用いた GUI 実機確認は未実施）

## 残課題（既知の制限と観察メモ）

ここに挙げるのは現在未解消の制限と観察メモであり、いずれも通常利用を妨げない。
実機未検証の残りは、拡張のホスト未登録時フォールバックのうち OS 確認ダイアログの表示確認（`yt-browser://` スキームへのタブ遷移までは Edge で実機確認済み）および[実装履歴](implementation-history.md)・本計画の各フェーズ節で「実機検証は未実施」と記した受け入れ項目である。

### 動作上の制限

- **画質変更時の再解決が稀に 403 になる**：yt-dlp が再発行した署名 URL を YouTube が拒否し、mpv が `No video or audio streams selected` で終了することがある
  一過性で再試行は通る
  BG-9・仕様決定 BJ の自動再試行を Phase 48 で実装済み（実機検証は未実施）
- **`set_property geometry` の位置オフセットが効かない（mpv 0.34 系の観測）**：mpv 0.38 以降では win32・x11 ともに実行時の geometry 変更で `±x±y` の位置が適用されると INV-3 で確認した（仕様決定 BU）
  `window-id` プロパティから OS API で窓を動かす代替経路もある。Wayland だけはプロトコル上の恒久制限で位置変更不可
  mpv バージョン別の実機確認は未検証

### 環境依存の観察

- **WebKitGTK のヒットずれ**：GPU なし環境で描画位置と入力ヒット領域がずれる事象があり、プレイヤーカードの常時マウント化（PR #15、設計書 §9.4）で実害経路を閉じた
  ほかのリマウント要素でも理論上は起こり得るが現行 UI では未観測（常時マウント化の対象拡大は Phase 73・FR-80・仕様決定 DE で実装済み、実機検証は未実施）。
  発生時の軽い回復手段は `location.reload()` で、一時停止中のカードもマウント時の `player_list` 取得で復元される（mpv 本体は動き続ける）
- **絵文字が豆腐化する**：検証環境のフォント不足が原因でアプリの不具合ではない
  チャットの絵文字表示はフォント環境に依存する
- **発言の多い配信での新規発言追従は未検証**：ライブ新着の逐次流入は PR #80 のポップアップ検証（実メッセージ 49 件）と Phase 33 のリプレイ同期で実機確認済み
  2026-10-11 に放送中の常時配信（ultrTyrXukw）でも新規発言の逐次追従（68→73 件）を確認したが、「発言の多い配信」という高負荷条件そのものでの追従確認は残る

### 先送りの拡張

- **拡張ストア版の `allowed_origins` 追加**：Edge と Chromium 系ブラウザへのホスト登録は Phase 79（FR-88・仕様決定 DM）で実装済み（Edge は実機確認済み、Chrome は未検証）であり、将来ストア公開する際はストア版の拡張 ID を `allowed_origins` へ追加する必要がある
- **yt-dlp の同梱**：`YtDlpResolver` の解決順は対応済みで、同梱配布は今後の選択肢として残す

### CI 環境の観察

- **Ubuntu ランナーラベルは明示固定**（仕様決定 AI）：`ubuntu-latest` の Ubuntu 26.04 への移行告知（2026-10-19 から段階移行、actions/runner-images#14748）を受け、`ubuntu-latest` は使わず CI 系は `ubuntu-26.04`、release.yml は `ubuntu-24.04`（配布物の動作下限を維持するため）に固定した
- **CI の網羅性強化、docs 専用変更スキップ、週次監査**（仕様決定 DQ）：必須チェック `CI` を維持したまま、`changes`（`scripts/ci_changes.py`）でドキュメントやメタファイルのみの変更時はビルドとテストのジョブをスキップする（`docs/design.md` 変更時は DDL 照合のため `rust` を実行する）
  あわせて `--locked` 強制、`scripts/test_scripts.py` と `check:js` の組み込み、`release.yml` の `preflight` と `scripts/verify_release.py`、週次の `security.yml`（`pnpm audit`）を整備した

## 引き継ぎ手順

- 進捗はこのファイルのチェックリスト（`[ ]` / `[x]`）で管理し、マージ時に更新する
- 作業再開時は「マージ済み Phase」「現在ブランチ」「未決事項」の三点を確認する
- 実機と手動の検証を後続の Devin や手元環境へ引き継ぐ手順は [verification-runbook.md](verification-runbook.md) を参照する
- ブランチは `devin/<エポック秒>-<短いスラッグ>`、変更は必ず PR 経由
- Windows 依存の作業は Windows 子セッションに切り出す（MSI ビルド検証は PR #12 で完了、Windows CI は PR #13 で常設化済み）
