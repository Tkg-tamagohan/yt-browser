#!/usr/bin/env python3
"""CI の変更ファイル判定を行い、各ジョブの実行可否フラグを出力する（仕様決定 DQ）。

GitHub Ruleset の必須ステータスチェック（名前 `CI` / ジョブ `ci-status`）は
ワークフロー自体が起動して `ci-status` が完了しないと永久 Pending になるため、
`on:` の `paths-ignore` ではなく本スクリプトによるジョブ単位スキップを使う。

判定はフェイルセーフを原則とし、すべての変更ファイルがドキュメントや
開発メタファイルの範囲に収まる場合のみビルド・テストジョブをスキップする。
ただし `docs/design.md` の変更時は、§8 の DDL ブロックを照合する
`ddl_matches_design_section8` テストを担保するため `rust` ジョブを有効にする。
変更一覧の取得に失敗した場合や空の場合は安全側へ倒して全ジョブを実行する。

使い方:
  python3 scripts/ci_changes.py --event pull_request --base <sha> --head <sha>
  python3 scripts/ci_changes.py --event push --before <sha> --after <sha>
"""

from __future__ import annotations

import argparse
import os
import subprocess
import sys
from pathlib import Path
from typing import Sequence

ROOT = Path(__file__).resolve().parent.parent
ZERO_SHA = "0" * 40

# 非コードファイル（スキップ判定で安全にコード検証から外せる対象）
NON_CODE_FILES = {
    "README.md",
    "AGENTS.md",
    "LICENSE",
    "scripts/README.md",
}
NON_CODE_PREFIXES = (
    "docs/",
    ".devin/",
    ".vscode/",
)
DESIGN_MD_PATH = "docs/design.md"


def normalize_path(path: str) -> str:
    """パス区切りを POSIX 形式に揃え、先頭の `./` を除く。"""
    p = path.strip().replace("\\", "/")
    while p.startswith("./"):
        p = p[2:]
    return p


def is_doc_or_meta_path(path: str) -> bool:
    """変更ファイルがドキュメントまたは開発メタファイルのみに属するか判定する。"""
    p = normalize_path(path)
    if not p:
        return False
    if p in NON_CODE_FILES:
        return True
    return any(p.startswith(prefix) for prefix in NON_CODE_PREFIXES)


def classify_paths(paths: Sequence[str] | None) -> dict[str, bool]:
    """変更ファイル一覧から各ジョブの実行フラグを返す。

    `paths` が None（差分取得不能）または空列の場合はフェイルセーフとして
    すべてのジョブを True にする。
    """
    if not paths:
        return {
            "run_frontend": True,
            "run_rust": True,
            "run_windows": True,
        }

    normalized = [normalize_path(p) for p in paths if normalize_path(p)]
    if not normalized:
        return {
            "run_frontend": True,
            "run_rust": True,
            "run_windows": True,
        }

    has_non_doc = any(not is_doc_or_meta_path(p) for p in normalized)
    has_design_md = any(p == DESIGN_MD_PATH for p in normalized)

    return {
        "run_frontend": has_non_doc,
        "run_rust": has_non_doc or has_design_md,
        "run_windows": has_non_doc,
    }


def _git(repo_root: Path, args: list[str]) -> subprocess.CompletedProcess[bytes]:
    return subprocess.run(
        ["git", "-c", "core.quotePath=false", *args],
        cwd=repo_root,
        capture_output=True,
    )


def _git_diff_names(repo_root: Path, rev_a: str, rev_b: str) -> list[str] | None:
    res = _git(repo_root, ["diff", "-z", "--name-only", rev_a, rev_b])
    if res.returncode != 0:
        return None
    raw = res.stdout.decode("utf-8", errors="surrogateescape")
    return [p for p in raw.split("\0") if p]


def detect_changed_files(
    *,
    event: str,
    base: str | None = None,
    head: str | None = None,
    before: str | None = None,
    after: str | None = None,
    repo_root: Path = ROOT,
) -> list[str] | None:
    """GitHub イベント情報から変更ファイル一覧を返す。取得不能時は None。"""
    if event == "pull_request":
        if not base or not head:
            return None
        mb = _git(repo_root, ["merge-base", base, head])
        if mb.returncode != 0:
            return None
        merge_base_sha = mb.stdout.decode("utf-8").strip()
        if not merge_base_sha:
            return None
        return _git_diff_names(repo_root, merge_base_sha, head)

    if event == "push":
        if not before or not after or before == ZERO_SHA:
            return None
        # force push や shallow clone で before コミットが欠落している場合は全実行へ倒す
        exists = _git(repo_root, ["cat-file", "-e", f"{before}^{{commit}}"])
        if exists.returncode != 0:
            return None
        return _git_diff_names(repo_root, before, after)

    return None


def write_github_outputs(flags: dict[str, bool], output_path: Path) -> None:
    """GitHub Actions の $GITHUB_OUTPUT へ lowercase boolean 文字列を書き出す。"""
    lines = [f"{k}={'true' if v else 'false'}\n" for k, v in flags.items()]
    with output_path.open("a", encoding="utf-8", newline="\n") as f:
        f.writelines(lines)


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="CI の変更ファイル判定とスキップフラグ出力"
    )
    parser.add_argument("--event", default=os.environ.get("GITHUB_EVENT_NAME", ""))
    parser.add_argument("--base", default=None)
    parser.add_argument("--head", default=None)
    parser.add_argument("--before", default=None)
    parser.add_argument("--after", default=None)
    parser.add_argument(
        "--github-output",
        default=os.environ.get("GITHUB_OUTPUT"),
        help="出力先の GITHUB_OUTPUT ファイルパス",
    )
    args = parser.parse_args(list(argv) if argv is not None else None)

    changed = detect_changed_files(
        event=args.event,
        base=args.base,
        head=args.head,
        before=args.before,
        after=args.after,
    )
    flags = classify_paths(changed)

    if changed is None:
        print("changed files: <unavailable -> fail-safe run all>")
    else:
        print(f"changed files ({len(changed)}):")
        for p in changed[:50]:
            print(f"  - {normalize_path(p)}")
        if len(changed) > 50:
            print(f"  ... (+{len(changed) - 50} more)")

    for k, v in flags.items():
        print(f"{k}={'true' if v else 'false'}")

    if args.github_output:
        write_github_outputs(flags, Path(args.github_output))

    return 0


if __name__ == "__main__":
    sys.exit(main())
