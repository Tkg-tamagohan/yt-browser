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
- 本プロジェクトはアカウント連携や OAuth 認証を持たない。
- ユーザーが任意設定する外部 API キー（`youtube.api_key` 等）はローカル保護設定キーに含めてエクスポート除外およびインポート時ローカル保持とし、URL クエリへ載せず機微ヘッダ（`X-Goog-Api-Key`）のみで送信し、イベント送出と診断出力で伏せ字化する。

## 実装上の恒久ルール

- 設定画面の保存ヘルパ（`saveToggleSetting`・`saveStringSetting`・`saveOptionalRangeSetting`・`saveSettingBatch` / `BatchEntry`）を呼び出すときは、DB 書込成功直後に発火する必須コールバック `onCommitted` 内で `savedSnapshot` を更新する。ヘルパの戻り値（保存待ち中の画面変更との競合判定）を待ってから `savedSnapshot` を更新してはならない。
- `src-tauri/src/` のモジュール依存方向は「上位層（`lib`・`main`・`commands`・`error`）→ ドメイン層 → 下位共有層（`db`・`model`・`util`）」の 3 層一方向とし、下位共有層からドメイン層・上位層への逆参照、ドメイン層から上位層への逆参照、および 2 ノード以上の循環依存を禁止する（`scripts/check_docs_consistency.py` の check 19 で機械検証する）。ドメイン層間の非循環な一方向参照は許容し、循環が生じる連携は直接参照せずコールバックやトレイトのフック面を定義して `lib.rs` または `commands` から注入する。
- `src/lib/library-page.svelte.ts` 周辺でプレイリストの非同期操作（同期、追加読み込み、削除、Undo 復元、並べ替え、作成、削除）を追加または変更するときは、単発の boolean ではなく相互排他（状態ロジックと UI `disabled` の双方）、エンティティ更新世代（`playlistMutSeq`）とスナップショット世代（`listsSnapshotSeq`）、直列化再取得（`refreshPlaylists`）、およびページングカーソル（`plTailPos`、`plHasMore`、`plTailDirty`）のダーティ管理と初期化を維持する。
- Rust 側で DB ロック外（`.await` や遅延 Undo スナップショット）をまたいで整数行 ID 基準の DB 書き込みを行うときは、ロック解放前に `Db::import_epoch` を取得し、最終書き込みトランザクション内で一致を再検証して不一致時は `DbError::StaleSync` 等で中断する。
