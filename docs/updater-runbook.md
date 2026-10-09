# 自動更新（updater）運用 Runbook

FR-15 / 仕様決定 AB の導入に必要なユーザー側の作業手順書。署名秘密鍵の GitHub Secrets 登録と、リリースの公開手順を扱う。

## 作業分担

| 担当 | 作業 |
|------|------|
| Devin | tauri-plugin-updater の導入、公開鍵の `tauri.conf.json` 登録、リリースワークフローの tauri-action 化、署名鍵ペアの生成と秘密鍵の引き渡し |
| ユーザー | 秘密鍵の GitHub Secrets 登録、リリースのドラフト公開、秘密鍵ファイルの保管 |

## 秘密鍵の登録（初回のみ）

更新成果物の署名に使う秘密鍵は Devin が `tauri signer generate` で生成し、セッション添付で引き渡す（公開鍵は `src-tauri/tauri.conf.json` の `plugins.updater.pubkey` に登録済み）。受け取った秘密鍵ファイルの中身を、そのまま GitHub Secrets に登録する。

1. 添付の `yt-browser.key`（秘密鍵ファイル）を開き、内容を全文コピーする
2. リポジトリの **Settings → Secrets and variables → Actions → New repository secret** を開く
3. `Name` に `TAURI_SIGNING_PRIVATE_KEY` と入力し、`Secret` に手順 1 の内容を貼って登録する
4. 今回の鍵はパスワード未設定のため `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` の登録は不要（ワークフロー側は未登録＝空として動く）。パスワード付き鍵へ切り替える場合は鍵を再生成し、公開鍵の `tauri.conf.json` 更新と両シークレットの登録をセットで行う

## リリースの作り方（`v*` タグ push）

1. バージョンを上げる（`src-tauri/tauri.conf.json` の `version`、必要に応じて `package.json`）
2. `git tag vX.Y.Z && git push origin vX.Y.Z` で release ワークフローが起動する
3. `ubuntu-latest` と `windows-latest` の両ビルドジョブが成功すると、release ジョブが成果物を集めて `latest.json` を生成し、**ドラフト作成→全アセット添付→公開**まで自動で行う
   - `latest.json` には AppImage（linux-x86_64）と NSIS（windows-x86_64）のエントリだけが入る（deb/rpm/MSI は対象外。`scripts/make_latest_json.py` が生成する）
   - 公開後に `releases/latest/download/latest.json` が有効になり、アプリ側の更新確認が検知できる
   - 片方のビルドジョブが失敗すると release ジョブは走らずリリース自体が作られない（不完全な公開を防ぐ仕組み）

## 確認

- 公開済みリリースがある状態で AppImage / NSIS 版を起動すると、新バージョン検知時に確認ダイアログが出る。承認でダウンロード→適用→再起動まで通る
- 設定画面の「今すぐ確認」ボタンで手動確認もできる
- deb / rpm / MSI 版は自動更新対象外（従来どおり手動更新）

## 注意点

- **秘密鍵を紛失すると更新が配布できなくなる**。`yt-browser.key` はリポジトリへコミットせず、安全な場所に保管する（公開鍵とペアでない鍵で署名してもインストール側で検証失敗になる）
- ドラフトのままのリリースでは `latest.json` が公開 URL に出ないため、公開前は更新確認が「更新なし／確認失敗」に見える（正常）
- CI のビルド成果物検証（署名・latest.json の中身）は release ワークフロー実行時に初めて行われる。初回リリース時はリリースの添付物一覧で `latest.json` と `*.sig` が揃っているか確認する
