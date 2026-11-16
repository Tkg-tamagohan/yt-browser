# 実機と手動の検証および Devin 引き継ぎ手順書

Phase 82（Step 1）から Phase 85（Step 2c）までの実装に対し、自動テストでは覆えない実機と手動の検証項目および環境別の引き継ぎ条件を定める。
CI と事前検証コマンド（`cargo test` や `pnpm test`、ドキュメント整合性チェック）はコード修正時点で通過している。
この文書は、WebView2 実画面、ブラウザ拡張機能、ローカル IPC、mpv 実再生など未実施の動作検証を後続作業へ引き継ぐために用いる。

## 作業環境別の検証可否

| 検証領域 | Windows 実機 / Windows セッション | Devin Cloud（Linux VM） | 備考 |
|---|---|---|---|
| 自動ゲート（Rust、フロント、文書、Dry-run） | 可 | 可 | コミット前後の基準ゲート |
| WebView2 CDP スモーク（[e2e_smoke.py](../scripts/e2e_smoke.py)） | 可 | 不可 | Windows WebView2 専用のため Linux WebKitGTK は対象外 |
| Tauri 実ウィンドウでの UI 操作と目視確認 | 可 | 条件付き | Linux はディスプレイ環境（Xvfb 等）がある場合のみ |
| Chrome / Edge 拡張機能とローカル IPC 往復 | 可 | 条件付き | Linux はブラウザ導入済みかつ表示環境がある場合のみ |
| mpv 実再生と画質切替および残留プロセス確認 | 可 | 不可相当 | 音声デバイスや実動画ストリーム環境が必要 |

Devin Cloud（Linux VM）を主環境として作業する場合は、Linux 側で再現できる事前確認とビルド確認までを実施する。
WebView2 CDP スモークや Windows 拡張機能連携は、Windows 子セッションまたはユーザー実機へ切り出して確認する。
環境制約で実施できない項目は推測で合格にせず、「未検証」として理由を記録する。

## 環境分離と事前注意

開発環境や本番インストーラ済み環境が同居している端末では、以下の競合とパス固定仕様に注意する。

### 1. Named Pipe 名の本番と開発での共有（Windows）

[native_host.rs](../src-tauri/src/native_host.rs) の `LOCAL_IPC_PIPE` は `\\.\pipe\io.github.tkg-tamagohan.yt-browser-nm-ipc` 固定となっている。
[e2e_dev.py](../scripts/e2e_dev.py) は Tauri の `identifier` を `io.github.tkg-tamagohan.yt-browser-dev` へ切り替えて WebView2 データディレクトリを分離するが、パイプ名は分離されない。
本番の `yt-browser` が起動中だと、Chrome や Edge 拡張機能からのローカル IPC が本番アプリ側へ届く場合がある。
IPC 検証の前に、本番アプリとトレイ常駐やバックグラウンドプロセスをすべて終了させる。

### 2. ホストモードが参照するデータディレクトリの固定仕様

`native_host.rs` の `app_data_dir()` は本番識別子 `io.github.tkg-tamagohan.yt-browser` を固定で参照する（Windows は `%APPDATA%\io.github.tkg-tamagohan.yt-browser`、Linux は `$XDG_DATA_HOME` または `~/.local/share` 配下）。
このためホストモードのプレイリスト取得（`list_playlists`）は、`-dev` 用 DB の配下ではなく本番データディレクトリ配下の `yt-browser.db` を読み取る（busy timeout `5000ms`）。
Linux ではローカル IPC の Unix ドメインソケットも本番データディレクトリ配下の `native-ipc.sock` となるため、`--identifier` を `-dev` へ変えた起動ではホスト側とソケット位置が噛み合わない。
Linux でソケット IPC を実機検証する場合は、`--identifier` 上書きなしの `pnpm tauri dev` または通常ビルドバイナリを使用する。

### 3. `YB_DEV_NO_NM=1` と `--no-suppress-nm` の使い分け

[e2e_dev.py](../scripts/e2e_dev.py) は既定で `YB_DEV_NO_NM=1` を設定し、debug ビルド時のみ Native Messaging の自動登録を抑止する（ログ出力は `YB_DEV_NO_NM=1 のため Native Messaging 登録を省略する`）。
UI のみの確認時は既定のまま起動し、本番レジストリやマニフェスト書き換えを防ぐ。
開発ビルドでブラウザ拡張機能と Native Messaging を実際に結ぶときだけ `--no-suppress-nm` を付けて起動し、検証後は本番状態へ戻す。
プロファイル分離とレジストリ後片付けの詳細手順は [yt-browser-windows-cdp-testing](../.devin/skills/yt-browser-windows-cdp-testing/SKILL.md) に従う。

### 4. コールドスタート直接起動（`try_spawn_binary`）における `devUrl` 要件

`src-tauri/tauri.conf.json` は `"devUrl": "http://localhost:1420"` を定義している。
開発版バイナリ（`target/debug/yt-browser.exe` 等）がホスト第 2 段の `try_spawn_binary` によりブラウザから直接起動される際、別プロセスで `pnpm dev`（Vite サーバー）が動いていないと WebView が空白になる。
コールドスタートの起動確認は、`pnpm dev` を別ターミナルで立ち上げた状態で行うか、`pnpm tauri build` でフロントエンドを同梱したバイナリを作成して実施する。

### 5. 拡張機能の固定識別子と Native Messaging 登録先

[extension/manifest.json](../extension/manifest.json)（MV3、version `0.3.0`、名称 `yt-browser opener`）の固定 ID は `hmfhiknpeefgjdmkjoofahdpkflfnkdl` である。
Windows の登録先レジストリは `HKCU\Software\Google\Chrome\NativeMessagingHosts\io.github.tkg_tamagohan.yt_browser`（`Microsoft\Edge` と `Chromium` も同構成）であり、`%APPDATA%\<app_data_dir>\native-messaging\io.github.tkg_tamagohan.yt_browser.json` を指す。
Linux はユーザー構成ディレクトリ配下の `google-chrome`（必須）、`microsoft-edge`（存在時）、`chromium`（存在時）それぞれの `NativeMessagingHosts/io.github.tkg_tamagohan.yt_browser.json` へ書き出される。

## 事前確認コマンド

実機検証の前段として、検証対象ブランチ上で以下の基準チェックを通しておく。
Windows PowerShell で Python スクリプトを実行するときは、cp932 による文字化けとエンコーディングエラーを防ぐため `$env:PYTHONUTF8="1"` を設定する。

```powershell
$env:PYTHONUTF8="1"
python -m py_compile scripts/*.py
python -m unittest discover -s scripts -p "test_*.py" -v
python scripts/check_docs_consistency.py
python scripts/bump_version.py --check
python scripts/e2e_dev.py --dry-run
```

```powershell
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
pnpm test
pnpm check
pnpm build
```

Windows 環境で WebView2 CDP スモークを実行する手順は [scripts/README.md](../scripts/README.md) を参照する。

## 実機検証チェックリスト

各項目は `PASS`、`FAIL`、`BLOCKED`（環境制約で未実施）のいずれかで判定し、証跡となるコマンド出力または操作結果の記録を残す。

### 共通スモーク

#### `V-01`: Windows WebView2 CDP スモーク

対象は [e2e_dev.py](../scripts/e2e_dev.py) と [e2e_smoke.py](../scripts/e2e_smoke.py) である。
手順として、本番競合がない状態で `e2e_dev.py` 経由の dev インスタンスを起動し、CDP ポートに対して `e2e_smoke.py` を流す。
期待結果は、全スモーク項目が失敗なく完了し、開発サーバー終了後に一時ファイルや不正プロセスが残らないことである。

### Step 1（Phase 82: P0 安定化修正とリーク解消）

#### `V-02`: プレイヤー操作失敗時の選択値ロールバック

対象は `src/lib/PlayerCards.svelte` である。
手順として、動画を再生しているプレイヤーカードで、`画質`、`音声出力`、`字幕` の切替と `終了` の通常動作を確認したあと、mpv 側でコマンド失敗が起きる状況（終了直前の操作や無効値相当）を発生させる。
期待結果として、正常時は各設定が反映され、操作失敗時は `<select>` の表示値が操作前の値へ戻ること。
あわせて、失敗した画質指定が次回以降のオーバーライドとして記憶されないことも確認する。

#### `V-03`: `ui.shortcuts` と非表示行スキップ

対象は `src/lib/shortcuts-logic.ts` と `src/lib/shortcuts.svelte.ts` である。
手順として、`設定` 画面の `ショートカット` で `キーボードショートカットを有効にする` をオンにし、ライブラリ一覧で絞り込み検索をかけて非表示行が発生した状態にする。
そのうえで `j`、`k`、`ArrowDown`、`ArrowUp`、`Enter`、`a`、`q` を押下する。
期待結果は、選択枠 `[data-shortcut-selected]` が可視行 `li.vr[data-video-id]` のみを移動し、フィルタで隠れた行へ飛ばないこと（自動テスト `KS-07` の実画面補完）である。

#### `V-04`: コンテキストメニュー共有化と多重開閉抑止

対象は `src/lib/VideoActions.svelte`、`src/lib/VideoContextMenu.svelte`、`src/lib/VideoActionsMenu.svelte` である。
手順として、動画行の右クリックと `プレイリストに追加`（`＋`）ボタンを連続で別々の行に対してクリックし、さらにメニュー外クリック（`pointerdown`）、`Escape` キー、別ルートへの画面遷移を試す。
期待結果は、画面上で同時に 1 つのメニューしか開かず、外クリック、Esc キー、画面遷移のいずれでもメニューが確実に閉じることである。

#### `V-05`: 設定全項目読み取り、一括画質適用、mpv プロセス終了安全性

対象は `src/routes/settings/+page.svelte`、`src-tauri/src/mpv/devices.rs`、`src-tauri/src/lib.rs` である。
手順として、複数のプレイヤーを起動した状態で `設定` 画面を開き、既定画質を変更して `保存` を押す。
さらに音声出力デバイスの再取得を試したあと、アプリウィンドウを閉じて終了する。
期待結果として、既定画質変更時に `再生中の <count> 台へ適用しました` 通知（一部失敗時は失敗件数通知）が破綻なく表示され、アプリ終了後（`RunEvent::Exit`）にバックグラウンドの `mpv` プロセスがタスク一覧へ残らないこと。

#### `V-06`: `YB_DEV_NO_NM=1` 起動時の本番レジストリ非破壊

対象は `src-tauri/src/lib.rs` と [e2e_dev.py](../scripts/e2e_dev.py) である。
手順として、Windows で本番 `HKCU\Software\Classes\yt-browser\shell\open\command` と Native Messaging ホストの JSON パスを控えておき、オプションなしの `python scripts/e2e_dev.py`（`YB_DEV_NO_NM=1` 付与）を起動して終了する。
期待結果として、debug ログに `YB_DEV_NO_NM=1 のため Native Messaging 登録を省略する` が出力され、終了後も本番側のカスタムスキームと Native Messaging 登録先が書き換わっていないこと。

### Step 2a（Phase 83: 応答性改善、DB ロック対策、拡張機能の耐障害性）

#### `V-07`: 拡張機能の 5000ms タイムアウトとタブ単位エラー表示

対象は `extension/background.js`、`extension/popup.html`、`extension/popup.js` である。
手順 1 として、YouTube の動画ページで拡張ポップアップを開き、`yt-browser で開く`、`再生位置を引き継いで開く`、`キューに追加`、プレイリスト選択 `<select id="plSelect">`、`プレイリストへ追加`、およびページ上の右クリックメニュー（`yt-browser で開く`、`yt-browser のキューに追加`）を実行する。
手順 2 として、ホストが応答しない状態（ホストバイナリ無効化やテスト用遅延等）で実行し、別タブへの切り替えと対象タブでの再実行を行う。
期待結果 1 は、ポップアップ上でプレイリストが `プレイリストを取得中…` の後に実データへ切り替わり（取得不可時は `プレイリストを取得できません`、0 件時は `プレイリストがありません`）、各起動操作が成功することである。
期待結果 2 は、応答不能時に約 `5000ms`（`NATIVE_TIMEOUT_MS`）でタイムアウトし、`#statusMsg` やツールチップ（`前回の失敗:`）、対象タブ限定の赤い `"!"` バッジが表示されることである。
あわせて、別タブにはバッジが出ず、問題解消後に同じタブで操作が成功するとバッジとエラー表示が消えること。

#### `V-08`: 検索、履歴、再開位置ヒントのデバウンスと世代管理

対象は `src/lib/library-page.svelte.ts` と `src/routes/+page.svelte` である。
手順として、ライブラリ検索欄および履歴検索欄に素早く連続して文字列の入力と消去を行う。
さらに開く画面の URL 欄へ、視聴履歴のある動画 URL と別 URL を連続して貼り替える。
期待結果として、`180ms` デバウンスと世代管理により古い検索結果で画面が上書きされないこと。
開く画面の `前回 {position} まで視聴` ヒントも、`150ms` デバウンス後に最新の入力 URL に対応した表示へ落ち着くこと。

#### `V-09`: 初期セットアップ完了と大量件数時の操作性

対象は `src/routes/+page.svelte` とライブラリ一覧である。
手順として、`初期セットアップ` が表示される状態（新規プロファイル等）で `はじめる` からセットアップを完了させる。
さらに登録動画や履歴が多い環境で、一覧のスクロールとフィルタ操作を行う。
期待結果として、初期セットアップ後に通常画面へ遷移して操作でき、一覧スクロールやフィルタ入力で顕著な引っかかりや表示崩れが起きないこと。

### Step 2b（Phase 84: 構造整理と DX 改善）

#### `V-10`: メニュー経路統一と設定通知の重複抑止

対象は `src/lib/video-actions.svelte.ts` と `src/routes/settings/+page.svelte` である。
手順として、行の `＋` ボタン経由と右クリックメニュー経由の双方でプレイリスト追加を試し、また設定画面で同じ操作を連続して実行して通知の出方を見る。
期待結果として、どちらのメニュー経路でも同一の追加挙動となり、同一文言の通知が多重に残らず単一タイマーで自然に消えること。

### Step 2c（Phase 85: Native Messaging ローカル IPC 化）

#### `V-11`: ウォームスタート（起動中アプリへのローカル IPC 即時配送）

対象は `src-tauri/src/native_host.rs` と `src-tauri/src/lib.rs` である。
手順として、本番競合を止めた状態で対象バージョンの `yt-browser` を起動したままにし、Chrome または Edge の拡張機能から `yt-browser で開く`、`再生位置を引き継いで開く`、`キューに追加`、およびプレイリスト取得を実行する。
Windows は `--no-suppress-nm` 付き開発版またはビルド版、Linux はソケットパス制約を踏まえた通常起動を使う。
期待結果として、新規アプリプロセスが二重起動されず、既存の `yt-browser` ウィンドウへ即座に URL が届いて再生やキュー追加が動き、拡張機能側もタイムアウトやハングを起こさないこと。

#### `V-12`: コールドスタート（未起動時フォールバックと標準出力ハング防止）

対象は `native_host.rs` の `deliver_scheme_url_with`（`DB-NM-17` 相当の 3 段配送）である。
手順として、`yt-browser` 本体を完全に終了した状態で Chrome または Edge に Native Messaging ホストが対象バイナリを指すよう登録し、拡張機能から `yt-browser で開く` を押す（開発バイナリ直起動の場合は `devUrl` 1420 の Vite サーバーを別起動しておくか同梱ビルドを用いる）。
期待結果として、第 1 段ローカル IPC 不達後に第 2 段 `try_spawn_binary` がアプリを新規起動し、子プロセスへ引き継がれた `Stdio::null()` により Chrome 側の stdout 待機が詰まらず応答が返ること。
アプリ立ち上がり後に渡された URL の動画が読み込まれることも確認する。

## 検証後の記録と後片付け

1. 検証結果を各項目（`V-01`〜`V-12`）ごとに `PASS` / `FAIL` / `BLOCKED`（未実施理由）と実行環境（OS やブランチコミット）付きで記録し、必要なフォローは [implementation-plan.md](implementation-plan.md) や対象 PR コメントへ残す。
2. Windows 実機で `--no-suppress-nm` や開発用レジストリ登録を行った場合は、検証終了後に必ず本番環境の復旧確認を行う。
   確認対象は `HKCU\Software\Classes\yt-browser\shell\open\command`、`NativeMessagingHosts\io.github.tkg_tamagohan.yt_browser`、別起動した Vite サーバーやテスト用 Chromium プロセス、残存 `mpv` プロセスである。
3. 4 本の積み上げ PR を順にマージする場合は、`main` に近い下流から順に進め、各マージ後は次段 PR のベースブランチを `main` へ向け直すこと。
