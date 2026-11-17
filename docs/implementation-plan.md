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
