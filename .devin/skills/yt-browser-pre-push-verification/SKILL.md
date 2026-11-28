---
name: yt-browser-pre-push-verification
description: yt-browser で push・PR 作成・CI 赤対応・自己レビューをするときの検証ゲートと再発防止集。CI 各ジョブ相当のローカル検証コマンド、「最終検証の後に編集しない」順序規則、実機検証用の一時変更を安全に戻す手順、エラー丸め・入力上限・非同期送出・コマンド登録漏れなど過去の指摘パターンを扱う。push 前の確認、CI 赤の修正、レビュー指摘への対応に使う。
---

# push 前検証ゲートと再発防止

Phase 38（PR #102）で CI が一度赤になり、自己レビューで 5 件の修正が出た。
再発しないための検証ゲート、変更の戻し方、指摘パターンをまとめる。

## CI が実際に走らせるもの（= ローカル検証ゲート）

`.github/workflows/ci.yml` のジョブに対応する。

- consistency: `python3 -m py_compile scripts/*.py`、
  `python3 -m unittest discover -s scripts -p "test_*.py" -v`、
  `python3 scripts/check_docs_consistency.py`、
  `python3 scripts/bump_version.py --check`、
  `python3 scripts/e2e_dev.py --dry-run`
- frontend: `pnpm check`（`svelte-check` と `check:js`）・`pnpm test`・`pnpm build`
- rust / windows: `cargo fmt --check`、`cargo clippy --locked --all-targets -- -D warnings`、`cargo test --locked --lib`（Windows は Common Controls v6 マニフェスト付与）、`cargo test --locked --bin yt-browser`、`cargo test --locked --doc`、`scripts\test_windows_signing_ci.ps1`

push 前（最後の編集のあと）にローカルで回す（PowerShell 5.1 向け、`pnpm.cmd` で実行ポリシーを回避し、`PYTHONUTF8=1` と `PYTHONIOENCODING="utf-8"` で文字化けを防ぐ）:

- `tauri-build` は Common Controls v6 マニフェストを `[[bin]]` にのみ埋め込むため、`cargo test --locked --lib` では `[char]0x1f` 区切りの `CARGO_ENCODED_RUSTFLAGS`（`/MANIFEST:EMBED` + `Microsoft.Windows.Common-Controls` 6.0.0.0）を付与する（省略すると `0xc0000139` で失敗）。そのまま残すと `--bin yt-browser` でリソース重複（`CVT1100`）を起こすため、`--lib` 直後に必ず `Remove-Item Env:CARGO_ENCODED_RUSTFLAGS -ErrorAction SilentlyContinue` で解除してから `--bin yt-browser` と `--doc` を実行する。
- PowerShell 5.1 は外部コマンド引数中のワイルドカードを展開しないため、`py_compile` は `python -m py_compile (Get-ChildItem scripts\*.py).FullName` として展開済みパスを渡す。

```powershell
$env:PYTHONUTF8="1"; $env:PYTHONIOENCODING="utf-8"
Set-Location src-tauri
cargo fmt --check; if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
cargo clippy --locked --all-targets -- -D warnings; if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
$sep = [char]0x1f
$env:CARGO_ENCODED_RUSTFLAGS = "-Clink-arg=/MANIFEST:EMBED${sep}-Clink-arg=/MANIFESTDEPENDENCY:type='win32' name='Microsoft.Windows.Common-Controls' version='6.0.0.0' processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'"
cargo test --locked --lib; $rc = $LASTEXITCODE
Remove-Item Env:CARGO_ENCODED_RUSTFLAGS -ErrorAction SilentlyContinue
if ($rc -ne 0) { exit $rc }
cargo test --locked --bin yt-browser; if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
cargo test --locked --doc; if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
Set-Location ..
& powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File 'scripts\test_windows_signing_ci.ps1'; if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
pnpm.cmd check; if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
pnpm.cmd test; if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
pnpm.cmd build; if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
python -m py_compile (Get-ChildItem scripts\*.py).FullName; if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
python -m unittest discover -s scripts -p "test_*.py" -v; if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
python scripts/check_docs_consistency.py; if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
python scripts/bump_version.py --check; if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
python scripts/e2e_dev.py --dry-run
```
`extension/shared.js` は `extension/shared.test.js`（`pnpm test` の一部）で単体検証され、`manifest.json`・`popup.html/js`・`./shared.js` の export 整合も `CB-04` で自動検証される。
`extension/*.js`・`svelte.config.js`・`vite.config.js` は `pnpm check`（`check:js`）で `node --check` による構文検証を行う（型検査は無いため、意図や仕様の妥当性は目視でも確認する）。

## 順序規則: 最終検証の後に編集しない

今回の CI 赤の直接原因は「検証ゲートを回した後にファイルを戻した」こと。
**編集・生成・revert → 検証ゲート → コミット** の順を固定し、
検証を通したあとの状態からはファイルを一切いじらない。
後片付け（一時変更の revert 含む）は検証の前に済ませる。

## 実機検証用の一時変更の安全な戻し方

CDP/dev ビルド検証では `scripts/e2e_dev.py` が環境変数 `YB_DEV_NO_NM=1` で Native Messaging 登録を抑止するため、ソースファイルの書き換えや退避ファイル作成は行わない（`yt-browser-windows-cdp-testing` スキル参照）。
スクリプト外で一時変更を加えた場合は、次の手順で安全に戻す。

- 一時変更は実装と同じファイルにできるだけ混在させない。
  Phase 38 では `lib.rs` の identifier 一時変更と `generate_handler!` のコマンド登録が同居し、`git checkout -- src-tauri/src/lib.rs` の広範囲 revert で登録行ごと消えて consistency ジョブが赤になった
- 一時変更を戻すときは、edit ツールや `git checkout -p` で一時変更の行だけを狙って戻す（`git checkout -- <file>` のまとめ戻しは避ける）
- 戻した直後に `git status` と `git diff` を目視し、一時変更が残っておらず実装変更だけになっていることを確認する
- 戻したあとは必ず検証ゲートを回し直す。
  前回検証を通した状態とは別物になっている

## スクリプト生成ファイルは最後に formatter へ通す

Python スクリプト経由で新規 .rs 等を作ったとき、フォーマッタを通さず
コミットして `cargo fmt --check` が落ちた実例がある（rust ジョブ）。
生成・一括編集の直後は `cargo fmt`（対象言語の formatter）を実行して
から検証ゲートへ進む。

## レビューで潰れた再発パターン（Phase 38 自己レビューより）

コード実装・レビュー時に確認する:

- `rusqlite` の `query_row` 結果に `.ok()` や `.unwrap_or(default)` を
  付けて「行無し」を判定しない。行無し以外の DB エラーまで握り潰すため、
  `OptionalExtension::optional()?` で `None` のみを既定値へ倒す
- コマンド引数の文字列フィールドに長さ上限を掛けるときは、タイトル
  だけでなく `channel_title`・`thumbnail_url` など全テキストへ適用する。
  片方だけだと renderer からの入力で DB を肥大化させうる
- メタ補完のような補助処理の失敗で主操作（キュー追加など）を落とさない。
  補完はフォールバックして続行し、主操作側の失敗だけをエラー通知する
- `chrome.runtime.sendMessage` の直後に `window.close()` すると送信側
  コンテキスト消滅でメッセージが落ちうる。後始末は送出のコールバックで
  行う
- 新規 Tauri コマンドを加えたら `generate_handler!` への登録までが
  実装。設計書 §3.1 への記載と登録がずれると consistency ジョブ
  （check 1）が検出するが、検出を当てにせず登録は実装の一部として扱う
- Tauri の `.setup` など Tokio ランタイム外の同期フックから呼ぶ処理に
  `tokio::net` や `tokio::windows::net::named_pipe::ServerOptions` を置くときは、
  生成も `tauri::async_runtime::spawn` 内へ入れ、`#[tokio::test]` だけでなく
  非 async の `#[test]`（必要なら別スレッド）から本番初期化関数を直接呼んで
  reactor 不在パニックが起きないことを検証する

## CI 赤の初動

- `gh run list --branch <branch>` で対象 run を特定する。
  `gh run view --log-failed` は in_progress 中に使えないため、
  `gh run view <id> --json jobs` で失敗ジョブ名を掴んでから
  `gh api repos/{owner}/{repo}/actions/jobs/<job_id>/logs` でログを取る
- consistency ジョブのログは UTF-8 前提。ローカル再現も `PYTHONUTF8=1` で
