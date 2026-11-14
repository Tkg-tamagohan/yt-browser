# scripts/

開発・リリース補助スクリプトの置き場。いずれも Python 3.10+ の標準
ライブラリのみで動き、外部パッケージは要らない。

## スクリプト一覧

| スクリプト | 用途 |
|------------|------|
| `check_docs_consistency.py` | docs ↔ コードの整合チェック（CI の consistency ジョブと同一） |
| `bump_version.py` | バージョン 4 箇所の一括更新（リリース準備用） |
| `make_latest_json.py` | リリースワークフローの `latest.json` 生成 |
| `cdp_client.py` | CDP（Chrome DevTools Protocol）の薄いクライアント（FR-84） |
| `e2e_dev.py` | CDP 付き隔離 dev インスタンスの起動補助（FR-84） |
| `e2e_smoke.py` | dev インスタンスへの DOM smoke チェック（FR-84） |

## CDP 検証基盤（FR-84、仕様決定 DI）

WebView2 は `--remote-debugging-port` 付きで起動すると CDP エンドポイントを
開き、`Runtime.evaluate` による DOM 評価・操作注入・イベント注入が
できる。過去フェーズの実機検証で都度使っていた手順を資産化したもの。

対象は Windows / WebView2 の dev ビルドを前提とする。

### 分離の考え方

本番インストールと同居させるため、dev インスタンスは次の 2 点を分離する。
`e2e_dev.py` が両方を自動で行う。

1. **identifier の分離**：`tauri dev --config <JSON>` で identifier を
   `io.github.tkg-tamagohan.yt-browser-dev` に差し替える。データ
   ディレクトリと single-instance の排他範囲が本番と分かれる
   （dev 専用の空 DB で起動する）
2. **Native Messaging 登録の抑制**：開発ビルドも起動時にホスト定義を
   自己登録する（設計書 §3.4.1。最後に起動したバイナリのパスが有効に
   なる）。抑制しないとホスト定義が dev バイナリを指し、本番側の
   拡張連携が壊れる。`e2e_dev.py` は `src-tauri/src/lib.rs` の
   `native_host::register(...)` 呼び出しを起動前にコメント化し、
   終了時に復元する

### 手順

```sh
# 1. CDP 付き dev インスタンスの起動（このターミナルは占有される）
python scripts/e2e_dev.py                # 既定 port 9222
python scripts/e2e_dev.py --dry-run      # 実施内容の確認だけ

# 2. 別ターミナルで smoke チェック
python scripts/e2e_smoke.py              # SMK-NN の結果表を出す

# 3. 任意の CDP 操作（同じく別ターミナル）
python scripts/cdp_client.py --list-targets
python scripts/cdp_client.py --eval "document.title"
python scripts/cdp_client.py --eval "document.querySelectorAll('ul.item-list > li').length"
python scripts/cdp_client.py --emit-open-url "https://www.youtube.com/watch?v=<ID>"
python scripts/cdp_client.py --emit-queue-url "https://www.youtube.com/watch?v=<ID>"

# 4. 終了：e2e_dev.py のターミナルで Ctrl+C
#    dev プロセスを止めて lib.rs を自動復元する
```

`--emit-open-url` / `--emit-queue-url` は `app://open_url` イベントの注入で、
OS 経由の deep link を通さずに deep link 受信経路全体（イベント →
`deeplink.svelte.ts` の振り分け）を動かせる。動画 URL は再生が走るため、
画面反映だけ見たいときはプレイリスト URL などを使う。

複数の page ターゲットがある環境では `--url-match localhost:1420` で
本体窓を指定する。

### 一時変更の戻し方

`e2e_dev.py` は lib.rs の原文を `src-tauri/src/lib.rs.e2e-backup` に退避し、
終了時（Ctrl+C を含む）に必ず復元する。スクリプト自体を強制終了した
場合は次のどちらかで戻す。

- `git checkout -- src-tauri/src/lib.rs`
- `lib.rs.e2e-backup` の内容を `lib.rs` へ書き戻してバックアップを消す

identifier の差し替えは `--config` の一時 JSON 経由で、設定ファイルは
一切書き換えない。

### 再マウント検知など発展的な使い方

`cdp_client.py` の `CdpSession` は `Runtime.enable` 後のイベントを
`events` キューへ溜める。`Runtime.evaluate` で DOM スナップショットや
MutationObserver の仕掛けを入れれば、リマウント（DOM 差分）監視や
長期観察にも使える。`e2e_smoke.py` は例外イベントの収集例にもなっている。

### 暫定解釈と既知の制限

- 実アプリでの CDP 実行は資産化時点では未検証。`cdp_client.py` は
  モックの CDP サーバで WebSocket ハンドシェイク・フレーム送受・
  コマンド往復まで検証済み、`e2e_dev.py` / `e2e_smoke.py` は起動・
  引数・エラーパスのみ検証済み
- `--emit-open-url` で使う `seq` は `Date.now()` を採用した暫定解釈。
  フロント側は `seq` で配送重複を除くだけなので衝突しない大きな値で足りる
- イベント注入は `core:event:default`（capabilities の `core:default`
  同梱）の emit 権限に依存する。権限が変わったら注入経路も見直す
- smoke の DOM セレクタは現行 UI 構造に合わせた暫定解釈。画面構造を
  変えたら `e2e_smoke.py` の `PAGE_CHECKS` を更新する
- dev identifier のデータディレクトリは初回が空のため、初回起動では
  セットアップウィザード（FR-75）のオーバーレイが出る想定。smoke は
  DOM 参照のみなので閉じなくても動くが、画面を目視するときは閉じる
- Linux の WebKitGTK では CDP ではなく WebKit インスペクタの経路が別に
  要るため本基盤の対象外とする
