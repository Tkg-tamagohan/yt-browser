# 自動更新（updater）運用 Runbook

FR-15 / 仕様決定 AB の導入に必要なユーザー側の作業手順書。署名秘密鍵の GitHub Secrets 登録と、リリースの公開手順を扱う。

## 作業分担

| 担当 | 作業 |
|------|------|
| Devin | tauri-plugin-updater の導入、公開鍵の `tauri.conf.json` 登録、リリースワークフローの成果物収集と `latest.json` 生成、署名鍵ペアの生成と秘密鍵の引き渡し |
| ユーザー | 秘密鍵の GitHub Secrets 登録、秘密鍵ファイルの保管、タグ発行とリリース内容の確認 |

## 秘密鍵の登録（初回のみ）

更新成果物の署名に使う秘密鍵は Devin が `tauri signer generate` で生成し、セッション添付で引き渡す（公開鍵は `src-tauri/tauri.conf.json` の `plugins.updater.pubkey` に登録済み）。受け取った秘密鍵ファイルの中身を、そのまま GitHub Secrets に登録する。

1. 添付の `yt-browser.key`（秘密鍵ファイル）を開き、内容を全文コピーする
2. リポジトリの **Settings → Secrets and variables → Actions → New repository secret** を開く
3. `Name` に `TAURI_SIGNING_PRIVATE_KEY` と入力し、`Secret` に手順 1 の内容を貼って登録する
4. 今回の鍵はパスワード未設定のため `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` の登録は不要（ワークフロー側は未登録＝空として動く）。パスワード付き鍵へ切り替える場合は鍵を再生成し、公開鍵の `tauri.conf.json` 更新と両シークレットの登録をセットで行う

## リリースの作り方（`v*` タグ push）

1. `scripts/bump_version.py X.Y.Z` でバージョンを一括更新し、コミットする（`tauri.conf.json`・`package.json`・`Cargo.toml`・`Cargo.lock` の 4 箇所を揃える。`--check` で現在の整合を確認できる）
2. `git tag vX.Y.Z && git push origin vX.Y.Z` で release ワークフローが起動する
3. `ubuntu-24.04` と `windows-latest` の両ビルドジョブが成功すると、release ジョブが成果物を集めて `latest.json` を生成し、**ドラフト作成→全アセット添付→公開**まで自動で行う
   - `latest.json` には AppImage（linux-x86_64）と NSIS（windows-x86_64）のエントリだけが入る（deb/rpm は対象外、MSI は配布自体を廃止。`scripts/make_latest_json.py` が生成する）
   - Windows 側は `--bundles nsis` でビルドし MSI は生成しない。署名 `*.sig` は `latest.json` 生成にだけ使い、リリースアセットには添付しない（仕様決定 AK）
   - Chrome 拡張は `extension/` を zip 化した `yt-browser-extension-<タグ名>.zip` として添付される（ストア未公開のため未パッケージ読み込み用。仕様決定 AC）
   - 公開後に `releases/latest/download/latest.json` が有効になり、アプリ側の更新確認が検知できる
   - 片方のビルドジョブが失敗すると release ジョブは走らずリリース自体が作られない（不完全な公開を防ぐ仕組み）

## 確認

- 公開済みリリースがある状態で AppImage / NSIS 版を起動すると、新バージョン検知時に確認ダイアログが出る。承認でダウンロード→適用→再起動まで通る
- 設定画面の「今すぐ確認」ボタンで手動確認もできる
- deb / rpm 版は自動更新対象外（手動更新）。MSI 版は配布していない

## 手動でタグを作成するときの注意点

- **タグ名は `vX.Y.Z`**（`v*` がトリガー）。`latest.json` の `version` にはスクリプトが先頭の `v` を除いた semver を書く
- **タグを打つ前に `scripts/bump_version.py X.Y.Z` でバージョンを更新しておく**（`tauri.conf.json`・`package.json`・`Cargo.toml`・`Cargo.lock` の 4 箇所を一括更新。`--check` で整合を確認できる）。ずれていると成果物名（`*_0.2.0_*` 等）と `latest.json` のバージョンが食い違い、アプリ側の更新判定が誤判定しうる（v0.3.0 タグで version 0.2.0 のままだった例あり）
  - `release.yml` の `preflight` ジョブでも `bump_version.py --check --expect-tag` と署名秘密鍵の存在をビルド前に検査し、公開直前には `scripts/verify_release.py` で配布成果物を検証する（仕様決定 DQ）
- **タグは修正を含んだコミットに打つ**。ビルドと release ジョブはタグの指すコミットをチェックアウトするため、修正が main へマージ済みでも古いコミットのタグを打つと古いワークフロー・スクリプトで実行される。Actions の「失敗したジョブの再実行（Re-run）」も同じコミットを見るため、ワークフローやスクリプトの修正は再実行では反映されない
- **タグの張り替え**: 修正後に同じバージョン名で出し直す場合は `git tag -d vX.Y.Z && git push origin :refs/tags/vX.Y.Z` で削除してから再打ち、または `git tag -f vX.Y.Z && git push -f origin vX.Y.Z`。出し直しが面倒なら次のタグ（`vX.Y.(Z+1)`）を打つ方が安全
- **途中失敗でリリースが作られていた場合**: 再実行時に `gh release upload --clobber` でアセットが差し替え添付されるので手動削除は不要（ドラフトのまま残った場合も公開状態に揃える）。配布廃止済みの `.msi` と添付対象外の `.sig` が旧実行で残っていても、再実行時にワークフロー側で削除して新規作成と同じ配布内容に揃える（仕様決定 AK）

## 注意点

- `extension/manifest.json` の `version` はアプリ版と独立した採番で、`scripts/bump_version.py` の更新対象外
- **秘密鍵を紛失すると更新が配布できなくなる**。`yt-browser.key` はリポジトリへコミットせず、安全な場所に保管する（公開鍵とペアでない鍵で署名してもインストール側で検証失敗になる）
- リリースの公開はワークフローが自動で行う（既存運用と同じ。ドラフト作成→アセット添付→公開の順で、添付途中の不完全な公開は出ない）。ビルドジョブが途中で失敗した場合はリリース自体が作られないので、直してからタグを打ち直す
- 公開前やビルド失敗時は `latest.json` が公開 URL に出ないため、更新確認は「更新なし／確認失敗」に見える（正常）
- CI のビルド成果物検証（署名・latest.json の中身）は release ワークフロー実行時に初めて行われる。初回リリース時はリリースの添付物一覧で `latest.json` があり、その `platforms` に linux-x86_64 と windows-x86_64 の両エントリ（各 `signature` 付き）が入っているか確認する（`*.sig` 自体はアセットに添付しない。仕様決定 AK）

## Windows Authenticode コード署名（自己署名証明書）

Windows 配布物（`yt-browser.exe`、NSIS アンインストーラ、および `*-setup.exe`）には、手元 Windows 端末の監視ツール等へ署名者ベースの信頼を維持するため、自己署名証明書による Authenticode 署名と RFC 3161 タイムスタンプを付与する（仕様決定 DW）。

### GitHub Secrets とワークフロー連携

1. `CODE_SIGN_PFX_BASE64`（`.pfx` の Base64 文字列）と `CODE_SIGN_PFX_PASSWORD`（PFX パスワード）を GitHub リポジトリの Actions Secrets へ登録する。
   秘密値を画面出力やチャット欄へ残さないよう、`gh secret set` の標準入力パイプで登録する。
2. `release.yml` の `build`（`windows-latest`）は `scripts/build_and_sign_windows.ps1` を呼び出し、PFX の証明書数と秘密鍵をメモリ上（`EphemeralKeySet`）で事前検証してから `Cert:\CurrentUser\My` へ非エクスポートでインポートし、`.pfx` とパスワード環境変数を即消去したうえで、一時 `--config` JSON から `scripts/sign_windows.ps1` を `bundle.windows.signCommand` へ注入する。
   これによりアプリ本体・アンインストーラ・NSIS インストーラへ順に署名され、署名済みインストーラに対して Tauri updater 用の `*.exe.sig` が生成される。
3. 署名後は `scripts/verify_windows_signature.ps1` が署名監査ログ（失敗・未完了・アンインストーラ捕捉漏れゼロ）と本体・捕捉済みアンインストーラ・セットアップの `signtool verify /pa /v`（SHA256 ハッシュ確認）および `Get-AuthenticodeSignature` を検証し、`finally` と `if: always()`（`scripts/cleanup_windows_signing.ps1`）で一時ファイルと証明書ストアを消去する。

### 事前検証と手元端末での確認

- タグ公開前に署名ビルドだけを検証するときは、GitHub Actions の `release` ワークフローを `workflow_dispatch`（既定 `windows_only: true`）で起動し、公開ジョブを動かさずに署名済み Artifact を取得する。
- 手元 Windows 端末では、秘密鍵を含まない公開証明書（`.cer`）だけを管理者 PowerShell で `Cert:\LocalMachine\Root` へ登録（`HasPrivateKey: False`）し、配置済み実行ファイルに対して `Get-AuthenticodeSignature` で `Status = Valid` を確認する。
- 証明書の有効期限（5 年）を更新する際は新しい自己署名証明書を再発行し、GitHub Secrets の 2 項目、手元端末の `Cert:\LocalMachine\Root`、および監視ツールのルートサムプリント設定を差し替える。
