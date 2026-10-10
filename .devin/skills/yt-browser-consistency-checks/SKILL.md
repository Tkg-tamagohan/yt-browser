---
name: yt-browser-consistency-checks
description: yt-browser の scripts/check_docs_consistency.py を拡張・修正する手順。check 関数と Findings の構造、main() への登録、正常系と改竄ネガティブテストによる検証、ID パーサの誤検出対策、Windows cp932 環境でのエンコーディング注意を扱う。文書↔コード整合の機械チェックを追加・調整するときに使用する。
---

# check_docs_consistency.py の拡張

`scripts/check_docs_consistency.py` は docs とコードの整合を機械チェックする検出器で、CI（`.github/workflows/ci.yml` の consistency ジョブ）とローカルの双方から実行される。
依存は Python 標準ライブラリのみで、新しい整合チェックはこのスクリプトに足す。

## 出力と終了コードの規約

- 各 check 関数は `Findings` を返し、`f.ok` / `f.warn` / `f.error` で結果を積む。
- チェックごとの最悪レベルが `[OK]` / `[WARN]` / `[ERROR]` として表示され、ERROR が 1 件でもあれば終了コード 1 になる。
- `[WARN]` は意図済みのベースライン（未使用の要件 ID など、現状を把握したいが違反ではないもの）に使い、CI を止めない。
- 「ズレを直すかどうか」の判定は人が行う前提で、検出器は差分の報告に留める。

## チェックの追加手順

1. `check_xxx() -> Findings` 関数を実装する。
   文書側は `extract_section`（見出し語で節を切り出す）と `table_rows`（表の行取得）を足場にする。
2. `main()` の `checks.append(("check N: 名前", check_xxx()))` に登録する。
   文書側の参照は節番号ではなく見出し語に合わせ、採番のずれに追従できるようにする。
3. 正常系を実行して期待どおり `[OK]` が出ることと終了コード 0 を確認する。
4. 対象文書を一時的に改竄し、想定の `[ERROR]`（または `[WARN]`）が出ることを確認してから復帰する。
   新チェックはこの改竄ネガティブテストを抜くと、実効性のない（何も検出しない）検出器を見逃しうる。

## ID パーサの誤検出対策

ドキュメント本文から ID を拾う正規表現は、次の形で誤検出を起こした実績がある（2026-10 の拡張時）。

- `仕様決定 AH、Phase 23` のような `、` 連記では、"Phase" の先頭 "P" が決定 ID として吸収される。
  大文字トークンは「直後に小文字が続かない」ことを要求し、`Phase` など通常語の先頭文字を弾く。
- 表ヘッダ行の `ID` というセル自体が ID として拾われる。
  ヘッダ行はスキャン対象から外す。
- `FR-2` や `LP-01` のような要件 ID やテスト ID 形は、決定記録 ID の検査では対象外としてスキップする。
  種別の違う ID を同一空間で照合すると未定義扱いの誤報になる。
- コメントやコード中の `仕様決定 XX` 参照も対象に含めるかはチェックごとに決め、対象外なら明示的に除外する。

## Windows cp932 環境での注意

このリポジトリの開発機は Windows で、Python のデフォルトエンコーディングが cp932 になりうる。

- スクリプト内のファイル読み書きは `read_text` / `write_text` に `encoding="utf-8"` を明示する。
  省略すると日本語を含む docs の読み取りで `UnicodeDecodeError` になる（`bump_version.py` で実害があった）。
- 標準出力に `[]` 外の文字（例: `→`）を出すと cp932 のコンソールで `UnicodeEncodeError` になる。
  出力は ASCII に留めるか、実行側で `PYTHONUTF8=1`（または `PYTHONIOENCODING=utf-8`）を立てる。

## CI との関係

`consistency` ジョブは `check_docs_consistency.py` と `scripts/bump_version.py --check` の 2 本を実行する。
チェックを追加して CI を常時赤くしないよう、意図的な現状のずれは `[WARN]` に落とす設計にする。
