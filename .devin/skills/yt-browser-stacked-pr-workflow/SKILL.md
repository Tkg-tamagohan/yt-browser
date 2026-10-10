---
name: yt-browser-stacked-pr-workflow
description: yt-browser のフェーズ実装でスタックした複数 PR を作り、親の修正を下流へ伝播し、順に main へマージする手順。ベース付け替え、ベースブランチ消失による下流 PR の連鎖クローズと復旧、コンフリクト中に Actions が起動しない罠、fmt 差分の伝播、Devin Review スレッドへの GraphQL 返信と解決を扱う。フェーズ単位の PR 作成・レビュー指摘対応・順次マージを頼まれたときに使用する。指摘のトリアージ方針は共有スキル devin-review-triage に委ねる。
---

# yt-browser のスタック PR とレビュー対応

このリポジトリでは実装をフェーズ単位の PR で進める（AGENTS.md の規約）。
フェーズが先行フェーズのコードに依存するときはブランチを積み重ねるため、修正の伝播とマージ順に特有の手順が要る。

## ブランチの積み方

- ブランチ名は `devin/<エポック秒>-<短いスラッグ>`。
- 後続フェーズは前フェーズのブランチから `git checkout -b` で切り、PR のベースは前フェーズのブランチにする。
  main 起点にすると前フェーズの差分が PR に混入する。
- 一つのブランチに複数フェーズの変更を混ぜない。
  誤って載せた場合は次フェーズへ切り出してからコミットする。

## 親の修正を下流へ伝播する

- 上流ブランチに修正を入れたら、下流ブランチへ順に `git merge <上流ブランチ> --no-edit` で伝播する。
  上流の修正が下流の意味論に絡む場合（例：フィルタキーに `channel_id` を含む分岐）は、伝播しないとマージ時に静かに劣化する。
- `cargo fmt` 由来の差分はベースにあると全下流へ伝播する。
  fmt 違反が見つかったら最上流で直してから伝播し、各下流ブランチで個別に直さない。

## 順次マージ

1. 最上流の PR から順に `gh pr merge <番号> --merge`（このリポジトリはマージコミット方式）。
2. 親がマージされたら、次の PR のベースを `gh pr edit <番号> --base main` で main へ付け替える。
   付け替えで CI が再走るので、pass を待ってからマージする。
3. 付け替え直後の `mergeable` は UNKNOWN になりうる。数秒待ってから再取得する。
4. PR のマージは `gh pr view <番号> --json state` で `MERGED` を確認するか、マージコミットを `git log` で確認してから報告する。
   CI 全 pass や mergeable だけではマージ済みと断定しない。

## ベースブランチ消失による下流クローズ連鎖

このリポジトリでは head ブランチの自動削除が有効であり、マージと同時に head ブランチが消える。
スタック中間の PR をマージすると、そのブランチをベースにしていた下流 PR はベースを失い、GitHub が自動で CLOSED にする（自動で main へ付け替えられるわけではない。本リポジトリで 2026-10 に観察）。
再オープンはベースが消えたままではできないため、まずブランチを復元する。

```bash
# 消えたブランチの先端は、親 PR のマージコミットの第 2 親で取れる
TIP=$(git rev-parse "origin/main^2")
git push origin "$TIP:refs/heads/<削除されたブランチ>"
gh pr reopen <下流PR番号>
gh pr edit <下流PR番号> --base main
# 付け替え後はブランチを消しても下流に影響しないため再削除する
git push origin --delete <削除されたブランチ>
```

この連鎖を避けるには、親をマージする前に下流のベースを main へ付け替えておくか、下流の付け替えが済むまでブランチを消さない順序にする。
リポジトリ設定の自動削除は `--delete-branch` を付けなくても動くため、フラグの有無では防げない点に注意する。

## CI が起動しないときの確認

pull_request トリガーのワークフローは、PR がコンフリクト状態（`mergeable=CONFLICTING`、`mergeStateStatus=DIRTY`）の間はスケジュールされない。
push したのに check-runs が 0 件のときはこれを疑う。

```bash
gh pr view <番号> --json mergeable,mergeStateStatus
gh api repos/<owner>/<repo>/commits/<sha>/check-runs --jq '.check_runs | length'
```

対処はベースブランチを対象ブランチへ取り込んでコンフリクトを解消すること。
ベース側の docs 更新と対象側の実装済み記述が衝突したときは、両者の内容を併記する形で解消する（実装済みの機能を「追加予定」に戻さない）。

## Devin Review スレッドの実操作

スレッド一覧と未解決数の取得：

```bash
gh api graphql -f query='{ repository(owner:"<owner>",name:"<repo>"){ pullRequest(number:<PR>){ reviewThreads(first:50){ nodes{ id isResolved comments(first:1){ nodes{ path line body } } } } } } }'
```

返信と解決はそれぞれ `addPullRequestReviewThreadReply` と `resolveReviewThread` のミューテーション。
重大度はコメント先頭の絵文字（🔴 実害・🟡 不具合の可能性・🔍 確認）で示され、修正 push のたびに再レビューが走り新しいスレッドが追加される。
全スレッドを解決したあとで未解決数を再確認し、0 でなければ次のラウンドに対応する。

## 日本語本文を GraphQL へ渡す

`gh api graphql -f query='...日本語...'` の形で渡すと UTF-8 が壊れて `Bad unicode escape` で失敗する。
Python の subprocess からクエリを組み立てて渡す。

```python
import json, subprocess
body = "返信本文。"
q = ('mutation { addPullRequestReviewThreadReply(input: {pullRequestReviewThreadId: "%s", body: %s}) { comment { id } } }'
     % (tid, json.dumps(body, ensure_ascii=False)))
subprocess.run(["gh", "api", "graphql", "-f", "query=" + q], encoding="utf-8")
```

解決は `resolveReviewThread(input: {threadId: "<PRRT_...>"})`。
`stdout` へ日本語を出すときは環境変数 `PYTHONIOENCODING=utf-8` を付ける。
