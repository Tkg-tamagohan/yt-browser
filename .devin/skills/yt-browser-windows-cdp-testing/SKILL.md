---
name: yt-browser-windows-cdp-testing
description: yt-browser を Windows 上で dev ビルド＋ CDP（WebView2 の remote-debugging-port）経由で実機検証する手順。インストール済み本番インスタンスとの共存（identifier 隔離と Native Messaging 登録抑制）、CDP による DOM 評価、app://open_url 注入による deep link 経路の検証、再マウント検知を扱う。
---

# yt-browser の dev ビルドを Windows 上で CDP 検証する

このリポジトリの開発機にはインストール済み本番ビルドの yt-browser が常駐実行されていることがある。
本機では NSIS per-user 版が `E:\software\yt-browser` にインストールされており、旧 MSI 版が残る環境もありうる。
dev ビルドをそのまま起動すると同一 identifier の single-instance が既存インスタンスへ起動を転送し、deep link のスキーム起動も本番側へ届いてしまう。
そのため実機検証は identifier を変えた隔離インスタンスで行い、UI 検証は CDP で行う。

## 本番インスタンスとの共存

- スキーム登録は `HKCU\Software\Classes\yt-browser\shell\open\command` がインストール済み exe（本機では `E:\software\yt-browser\yt-browser.exe`）を指す。
  per-user インストールなら HKCU、per-machine インストールなら `HKLM\SOFTWARE\Classes\yt-browser\shell\open\command` 側になるため、環境が変わったら両ハイブを `reg query` で確認する（本機では HKCU のみ存在）。
  `yt-browser:` 形式の URL を OS から起動すると本番インスタンスと本番 DB に届くため、検証には使わない。
- 標準の `scripts/e2e_dev.py` は、`tauri dev --config <一時 JSON>` で `identifier` を `io.github.tkg-tamagohan.yt-browser-dev` へ差し替える。
  別 identifier はアプリデータ（`%APPDATA%\<identifier>\`）と single-instance の排他範囲が分かれるため、本番が動いていても dev 専用 DB で共存起動できる（設定ファイル本体は書き換えない）。
- `src-tauri/src/lib.rs` の `native_host::register` は Chrome の Native Messaging 登録キーが identifier に依らず共通（HOST_NAME `io.github.tkg_tamagohan.yt_browser` は固定値）のため、dev exe のパスで本番登録を上書きしうる。
  具体的には dev 起動時に `%APPDATA%\<identifier>-dev\native-messaging\` 配下へ書いたマニフェストのパスで `HKCU\Software\Google\Chrome\NativeMessagingHosts\<HOST_NAME>` の値を更新する。
  dev アプリデータを後で削除するとそのパスは実在しなくなり、拡張からの NM 呼び出しが本番側で失敗しうる。
  `e2e_dev.py` は起動前に `lib.rs` の `native_host::register(...)` 呼び出しをコメント化し、原文を `src-tauri/src/lib.rs.e2e-backup` に退避して終了時に自動復元する（`--no-suppress-nm` で無効化可）。
- スキームのレジストリ登録はインストーラが行い、dev 起動では上書きされない（現行コードは `deep_link().register` を呼ばず `get_current()` で読むだけ）。
  将来コードに `register` 呼び出しが入るとこの前提が崩れるため、その場合は再確認する。
  検証後に `reg query "HKCU\Software\Classes\yt-browser\shell\open\command"` で本番 exe のままかを確認するとよい。
- 強制終了などで `e2e_dev.py` の終了処理が走らなかった場合は、検証後に `lib.rs.e2e-backup` または `git checkout -- src-tauri/src/lib.rs` で復元する。

## 隔離インスタンスの起動

`scripts/e2e_dev.py` を使うと、上記の分離（`--config` 一時 JSON と `lib.rs` 退避と復元）および `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="--remote-debugging-port=<port>"` 付き `pnpm tauri dev` を一括で起動できる。

```bash
python scripts/e2e_dev.py --dry-run          # 実施内容の確認だけ行う
python scripts/e2e_dev.py                    # port 9222 で起動（このシェルは占有される）
python scripts/e2e_dev.py --port 9333        # ポートを変える場合
```

起動後、別ターミナルで主要画面の DOM smoke チェック（`SMK-01`〜`SMK-09`）を流せる。

```bash
python scripts/e2e_smoke.py                  # port 9222 の対象へ実行
python scripts/e2e_smoke.py --port 9333 --timeout 20 --url-match localhost:1420
```

手動でバイナリだけを起動する場合は、`devUrl`（`http://localhost:1420`）の vite サーバを先に起動し、`WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="--remote-debugging-port=9222"` を渡して起動する。
起動直後にページが真っ白なら vite の未起動が疑わしい。
vite を起動してから `python scripts/cdp_client.py --navigate "http://localhost:1420/"` で読み直せばよい。

## CDP による DOM 評価

`scripts/cdp_client.py` は Python 3.10+ の標準ライブラリのみで動き、外部パッケージ（`websocket-client` など）の追加は要らない。
参考として、WebView2 は接続時に Origin ヘッダを送ると `Rejected an incoming WebSocket connection from the http://localhost:9222 origin.` で拒否するため、`cdp_client.py` 内の WebSocket 実装も Origin を送らず接続している。

```bash
python scripts/cdp_client.py --list-targets
python scripts/cdp_client.py --eval "document.title"
python scripts/cdp_client.py --eval "JSON.stringify({items: document.querySelectorAll('ul.item-list > li').length})"
python scripts/cdp_client.py --url-match localhost:1420 --eval "document.querySelector('nav.app-nav') !== null"
```

複数 page ターゲットがある環境では `--url-match localhost:1420` で本体窓を指定する。
`--eval` の返り値が見たい場合は JS 側で `JSON.stringify({...})` を返して観測する。

## deep link 経路の検証

OS のスキーム起動を使わずに deep link ハンドラ以降の経路全体を動かせる。
`scripts/cdp_client.py` の `--emit-open-url` / `--emit-queue-url` を使うと、`deep_link.rs` が投げるのと同じ `app://open_url` イベントを `window.__TAURI_INTERNALS__.invoke("plugin:event|emit", ...)` 経由で注入する（`core:event:default` の emit 権限を使う）。

```bash
python scripts/cdp_client.py --emit-open-url "https://www.youtube.com/playlist?list=..."
python scripts/cdp_client.py --emit-queue-url "https://www.youtube.com/watch?v=<ID>"
```

`seq` には `Date.now()` を渡しており、冷起動との区別およびフロント側の `handledSeq` による重複排除を通過する。
注入後は dispatch → URL 分類 → `playlist_import` や再生またはキュー追加 → トーストと Tauri イベントという本物と同一の経路を通る。
動画 URL の `--emit-open-url` は再生が走るため、画面反映だけ見たいときはプレイリスト URL などを使う。

## 再マウントとイベント駆動更新の区別

一覧更新が「ページの再マウントによる再取得」ではなく「イベント購読による更新」で起きたことを示すには、事前にマーカーを立てておく。

```js
window.__preImport = 'yes'
```

取り込み反映後に `window.__preImport` が残っていれば同一インスタンスのまま更新された証拠になる。
ページが再マウント・リロードされると window ごと消える。

## Svelte の bind:value への値注入

`.value = x` だけでは Svelte 側の state が更新されない。
値を入れたあとに input イベントを発火させる。

```js
const inp = document.querySelectorAll('.pl-new')[1].querySelectorAll('input');
inp[0].value = 'https://www.youtube.com/playlist?list=...';
inp[0].dispatchEvent(new Event('input', { bubbles: true }));
```

## dev DB へのシード

dev インスタンスの DB は `%APPDATA%\<identifier>-dev\yt-browser.db`（`lib.rs` で `app_data_dir()/yt-browser.db` に接続）。
ページングやバックフィルの検証に大量データが要るときは、dev インスタンスを一度起動してスキーマを作らせ、停止してから Python の `sqlite3` で行を挿入する。
フィード行は `videos` テーブルに載るが、`feed_list_filtered` は `channels` との JOIN と `v.ingested = 1` を必須条件にするため、素の INSERT だけではフィード一覧に出ない。生 INSERT では次を押さえる。

- `channels` 側の購読行を UC 形 `channel_id` で先に用意する（JOIN 条件）。
- `videos` 行では `ingested = 1` を立てる（migration v7 で追加された `NOT NULL DEFAULT 0` のカラムで、既定値のままだと一覧から除外される）。
- `is_read` を明示する（未読として表示したいなら `0`）。
- `kind` には CHECK 制約があり、`'video'`、`'short'`、`'live'`、`'upcoming'` のいずれかしか入らない。

```python
import sqlite3, os
db = os.path.expandvars(r"%APPDATA%\<identifier>-dev\yt-browser.db")
con = sqlite3.connect(db)
# channels に購読行を足してから、ingested=1 / is_read / kind（CHECK 制約内）を指定して videos へ feed 行を挿入する
```

`list_feed` は `published_at` 降順・NULL 末尾・`video_id` 昇順のカーソルページングなので、シードでは `published_at` をばらつかせるとページ境界の挙動を確認できる。
検証が終わったら dev identifier 側のアプリデータを削除して戻す。

## 複数画面に跨る検証での注意

`location.href` によるフルリロードは JS 側のセッション限定状態（実行中キュー、イベント購読の世代など）を破棄する。
状態を跨いだ検証（例：キュー実行中に別画面でキューへ追加）ではフルリロードを使わず、SvelteKit のクライアント側ナビゲーションで遷移する。
CDP からはアプリ内リンク要素の `click()`、または `__TAURI_INTERNALS__` 経由で同等の画面遷移を起こせる。

## 孤児 mpv プロセス

前回の dev インスタンス由来の mpv が残ることがある。
named pipe は `\\.\pipe\yt-browser-mpv-*` で見えるが、exec の quoting でバックスラッシュが潰れるため列挙スクリプトはファイルに書いて実行する。

```powershell
# list_pipes.ps1
Get-ChildItem \\.\pipe\ | Where-Object Name -like 'yt-browser-mpv-*' | Select-Object -ExpandProperty Name
```

`tasklist //FI "IMAGENAME eq mpv.exe" //FO CSV` と突き合わせ、dev 側のものだけを `taskkill //PID <pid> //F` で止める。

## 再生検証時の音量

ユーザー運用として、再生検証は音量を最小限にして行う。
再生開始直後に mpv IPC で `volume` を `0` にするか、UI の音量スライダを最小にしてから検証する。

## Tauri コマンドの直接呼び出し

UI 経路を通さずにコマンドを検証したいときは `__TAURI_INTERNALS__.invoke` で直接叩ける。

```js
__TAURI_INTERNALS__.invoke('play_video', { videoId: '...', resume: false })
__TAURI_INTERNALS__.invoke('chat_start', { videoId: '...', instanceId: 1 })
```

`play_video` の `pip` を省略した場合は `pip.default`（既定 PiP）の設定が使われる。

## 後片付け

- `scripts/e2e_dev.py` を Ctrl+C で止めると dev プロセスツリーを終了し、`src-tauri/src/lib.rs.e2e-backup` から `lib.rs` を自動復元する。
- スクリプトを強制終了した場合、または手動起動を使った場合は、`lib.rs.e2e-backup` からの書き戻し（または `git checkout -- src-tauri/src/lib.rs`）を行い、残ったプロセスだけを PID 指定で止める（`tasklist //FI "IMAGENAME eq yt-browser.exe" //FO CSV` で確認し、`taskkill //PID <pid> //F` で本番 PID を巻き込まないよう止める）。
- dev identifier のアプリデータ `%APPDATA%\io.github.tkg-tamagohan.yt-browser-dev\` を必要に応じて削除する。
- `reg query "HKCU\Software\Google\Chrome\NativeMessagingHosts\io.github.tkg_tamagohan.yt_browser"` で値が本番 appdata のマニフェストを指しているか確認する。
  dev 側を指してしまった場合は本番アプリを一度起動すれば `native_host::register` が冪等に本番値へ戻す。
- `git status` と `git diff` を確認し、一時変更や `lib.rs.e2e-backup` が作業ツリーに残っていないことを確かめる。

## 補足: gh CLI がなくても GitHub 操作できる

`gh` が無い環境では `printf "protocol=https\nhost=github.com\n\n" | git credential fill` で取れるトークンを `Authorization: Bearer` にして REST API を直接叩ける。
`gh` 本体があって未認証の場合は、そのトークンを `GH_TOKEN` 環境変数に渡せば `gh pr` 系がそのまま動く。
