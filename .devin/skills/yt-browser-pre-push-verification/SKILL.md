---
name: yt-browser-pre-push-verification
description: yt-browser で push・PR 作成・CI 赤対応・自己レビューをするときの検証ゲートと再発防止集。CI 各ジョブ相当のローカル検証コマンド、「最終検証の後に編集しない」順序規則、実機検証用の一時変更を安全に戻す手順、エラー丸め・入力上限・非同期送出・コマンド登録漏れなど過去の指摘パターンを扱う。push 前の確認、CI 赤の修正、レビュー指摘への対応に使う。
---

# push 前検証ゲートと再発防止

Phase 38（PR #102）で CI が一度赤になり、自己レビューで 5 件の修正が出た。
再発しないための検証ゲート、変更の戻し方、指摘パターンをまとめる。

## CI が実際に走らせるもの（= ローカル検証ゲート）

`.github/workflows/ci.yml` のジョブに対応する。

- consistency: `python3 scripts/check_docs_consistency.py` と
  `python3 scripts/bump_version.py --check`
- frontend: `pnpm check`、`pnpm test`、`pnpm build`
- rust / windows: `cargo fmt --check`、
  `cargo clippy --all-targets -- -D warnings`、`cargo test`

push 前（最後の編集のあと）にローカルで回す:

```bash
cd src-tauri && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test && cd ..
pnpm check && pnpm test && pnpm build
PYTHONUTF8=1 python scripts/check_docs_consistency.py
python scripts/bump_version.py --check
```

Windows の python 出力は cp932 で文字化けするため `PYTHONUTF8=1` を付ける
（出力の可読性のためで、判定挙動は変わらない）。
`extension/` の JS には lint が無いため変更は目視で確認する。

## 順序規則: 最終検証の後に編集しない

今回の CI 赤の直接原因は「検証ゲートを回した後にファイルを戻した」こと。
**編集・生成・revert → 検証ゲート → コミット** の順を固定し、
検証を通したあとの状態からはファイルを一切いじらない。
後片付け（一時変更の revert 含む）は検証の前に済ませる。

## 実機検証用の一時変更の安全な戻し方

CDP/dev ビルド検証では identifier の書き換えや Native Messaging 登録の
抑制など、コミットへ紛れてはいけない一時変更を入れる
（`yt-browser-windows-cdp-testing` スキル参照）。

- 一時変更は実装と同じファイルにできるだけ混在させない。
  Phase 38 では `lib.rs` の identifier 一時変更と `generate_handler!` の
  コマンド登録が同居し、`git checkout -- src-tauri/src/lib.rs` の
  広範囲 revert で登録行ごと消えて consistency ジョブが赤になった
- 戻すときは `git checkout -- <file>` のまとめ戻しではなく、edit ツールや
  `git checkout -p` で一時変更の行だけを戻す
- 戻した直後に `git diff` で残存差分を目視し、「一時変更だけ消えて
  実装変更が残っている」ことを確認する
- 戻したあとは必ず検証ゲートを回し直す。前回検証を通した状態とは
  別物になっている

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

## CI 赤の初動

- `gh run list --branch <branch>` で対象 run を特定する。
  `gh run view --log-failed` は in_progress 中に使えないため、
  `gh run view <id> --json jobs` で失敗ジョブ名を掴んでから
  `gh api repos/{owner}/{repo}/actions/jobs/<job_id>/logs` でログを取る
- consistency ジョブのログは UTF-8 前提。ローカル再現も `PYTHONUTF8=1` で
