# AGENTS.md

このリポジトリで作業するエージェントと開発者への規約。

## 文書を仕様の正とする

- 要件定義は `docs/requirements-definition.md`、協議で確定した判断は `docs/decision-records.md`（仕様決定 A 以降の ID で参照する）、実装計画は `docs/implementation-plan.md` を正とする。
- 文書同士が矛盾するときは要件定義書を正として派生文書側を直し、修正したことを報告する。
- 実装と文書が矛盾する場合は、どちらを正とするか確認を求める。

## 変更は PR 経由

- 変更はすべて PR で出し、既定ブランチへの直接 push はしない。
- ブランチ名は `devin/<エポック秒>-<短いスラッグ>` を標準とする。
- 実装は計画書のフェーズ単位で進め、マージしたら計画書のチェックリストを更新する。

## 言語と文体

- コミットメッセージ、PR のタイトルと本文、docs、コードコメントは日本語で書く。
- コード中の識別子は英語とする。
- docs の日本語は `shared-skills:japanese-tech-writing` の規範に従う。

## 秘密情報

- 認証情報やトークンをコードや文書、コミット、ログに平文で書かない。
- 本プロジェクトはアカウント連携を持たないため、ユーザーの認証情報を扱う設計自体を採用しない。

## 実装上の恒久ルール

- 設定画面の保存ヘルパ（`saveToggleSetting`・`saveStringSetting`・`saveOptionalRangeSetting`・`saveSettingBatch` / `BatchEntry`）を呼び出すときは、DB 書込成功直後に発火する必須コールバック `onCommitted` 内で `savedSnapshot` を更新する。ヘルパの戻り値（保存待ち中の画面変更との競合判定）を待ってから `savedSnapshot` を更新してはならない。
- `src-tauri/src/` のモジュール依存方向は「上位層（`lib`・`main`・`commands`・`error`）→ ドメイン層 → 下位共有層（`db`・`model`・`util`）」の 3 層一方向とし、下位共有層からドメイン層・上位層への逆参照、ドメイン層から上位層への逆参照、および 2 ノード以上の循環依存を禁止する（`scripts/check_docs_consistency.py` の check 19 で機械検証する）。ドメイン層間の非循環な一方向参照は許容し、循環が生じる連携は直接参照せずコールバックやトレイトのフック面を定義して `lib.rs` または `commands` から注入する。
