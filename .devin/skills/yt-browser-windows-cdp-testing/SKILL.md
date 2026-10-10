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
- `src-tauri/tauri.conf.json` の `identifier` を末尾に `-dev` を付けた値へ一時変更すると、アプリデータ（`%APPDATA%\<identifier>\`）も別系統になる。
  別 identifier は single-instance も別枠になるため、本番が動いていても dev インスタンスを起動できる。
- `src-tauri/src/lib.rs` の `native_host::register` は Chrome の Native Messaging 登録キーが identifier に依らず共通（HOST_NAME `io.github.tkg_tamagohan.yt_browser` は固定値）のため、dev exe のパスで本番登録を上書きしうる。
  具体的には dev 起動時に `%APPDATA%\<identifier>-dev\native-messaging\` 配下へ書いたマニフェストのパスで `HKCU\Software\Google\Chrome\NativeMessagingHosts\<HOST_NAME>` の値を更新する。
  dev アプリデータを後で削除するとそのパスは実在しなくなり、拡張からの NM 呼び出しが本番側で失敗しうる。
  `if std::env::var_os("YB_DEV_NO_NM").is_none() { native_host::register(...) }` のような環境変数ガードを一時的に差し込み、その変数を立てて起動する。
- スキームのレジストリ登録はインストーラが行い、dev 起動では上書きされない（現行コードは `deep_link().register` を呼ばず `get_current()` で読むだけ）。
  将来コードに `register` 呼び出しが入るとこの前提が崩れるため、その場合は再確認する。
  検証後に `reg query "HKCU\Software\Classes\yt-browser\shell\open\command"` で本番 exe のままかを確認するとよい。
- identifier と NM ガードの変更はコミット対象外の検証専用とし、検証後は `git checkout --` で戻す。

## 隔離インスタンスの起動

debug ビルドは `devUrl`（`http://localhost:1420`）の vite サーバを読むため、先に起動しておく。

```bash
cd src-tauri && cargo build
pnpm dev   # 別シェルで vite を起動
```

CDP ポートは exe の引数ではなく `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS` 環境変数で WebView2 に渡す。

```bash
WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="--remote-debugging-port=9222" YB_DEV_NO_NM=1 ./target/debug/yt-browser.exe
```

起動直後にページが真っ白なら vite の未起動が疑わしい。
vite を起動してから CDP で `location.href = 'http://localhost:1420/'` を評価して読み直せばよい。

## CDP による DOM 評価

Python 側は `websocket-client` パッケージが要る。
`websockets` や `websocket` とは別物なので `pip install --user websocket-client` で入れる。

接続時に Origin ヘッダを送ると `Rejected an incoming WebSocket connection from the http://localhost:9222 origin.` で拒否される。
`websocket.create_connection(url, suppress_origin=True)` で Origin を送らないようにする。

`$TEMP/cdp_eval.py` の雛形は次のとおり。

```python
import json, sys, urllib.request
from websocket import create_connection

port = sys.argv[2] if len(sys.argv) > 2 else "9222"
targets = json.load(urllib.request.urlopen(f"http://localhost:{port}/json/list"))
page = next(t for t in targets if t["type"] == "page")
ws = create_connection(page["webSocketDebuggerUrl"], suppress_origin=True)
ws.send(json.dumps({
    "id": 1, "method": "Runtime.evaluate",
    "params": {"expression": sys.argv[1], "returnByValue": True, "awaitPromise": True},
}))
while True:
    msg = json.loads(ws.recv())
    if msg.get("id") == 1:
        print(json.dumps(msg.get("result", {}).get("result", {}).get("value"), ensure_ascii=False))
        break
ws.close()
```

呼び出しは `python "$TEMP/cdp_eval.py" "<JS 式>"` の形。
返り値が見たい場合は JS 側で `JSON.stringify({...})` を返して観測する。

## deep link 経路の検証

OS のスキーム起動を使わずに deep link ハンドラ以降の経路全体を動かせる。
`deep_link.rs` が投げるのと同じ `app://open_url` イベントを、フロント側から `plugin:event|emit` で注入する。

```js
__TAURI_INTERNALS__.invoke('plugin:event|emit', {
  event: 'app://open_url',
  payload: { seq: 9001, url: 'yt-browser://open?url=' + encodeURIComponent('https://www.youtube.com/playlist?list=...') }
})
```

`seq` は冷起動との区別用の連番で値は何でもよい。
ただしフロント側の `handledSeq` が処理済み seq を記憶するため、同一 seq の再注入は弾かれる。連続して注入するときは毎回 seq を増やす。
注入後は dispatch → URL 分類 → `playlist_import` → トーストと Tauri イベントという本物と同一の経路を通る。

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

- `tasklist //FI "IMAGENAME eq yt-browser.exe" //FO CSV` でプロセスを列挙し、dev インスタンスの PID だけ `taskkill //PID <pid> //F` で止める（本番 PID を巻き込まない）。
- vite は `Get-NetTCPConnection -LocalPort 1420 -State Listen` の `OwningProcess` を taskkill する。
- dev identifier のアプリデータ `%APPDATA%\<identifier>-dev\` をディレクトリごと削除する（DB だけでなく `mpv\wheel.lua` や、NM ガードを入れ忘れた場合に書かれる `native-messaging\` も含む）。
- `reg query "HKCU\Software\Google\Chrome\NativeMessagingHosts\io.github.tkg_tamagohan.yt_browser"` で値が本番 appdata のマニフェストを指しているか確認する。dev 側を指してしまった場合は本番アプリを一度起動すれば `native_host::register` が冪等に本番値へ戻す。
- identifier と NM ガードの一時変更を `git checkout --` で戻し、`git status` で差分が検証前の状態に戻ったことを確認する。

## 補足: gh CLI がなくても GitHub 操作できる

`gh` が無い環境では `printf "protocol=https\nhost=github.com\n\n" | git credential fill` で取れるトークンを `Authorization: Bearer` にして REST API を直接叩ける。
`gh` 本体があって未認証の場合は、そのトークンを `GH_TOKEN` 環境変数に渡せば `gh pr` 系がそのまま動く。
