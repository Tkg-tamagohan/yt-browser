#!/usr/bin/env python3
"""アプリのバージョンを一括更新する（リリース準備用）。

書き換え対象は 4 箇所:
  - src-tauri/tauri.conf.json : Tauri が成果物名・updater 判定に使う主バージョン
  - package.json              : フロントエンドのメタデータ
  - src-tauri/Cargo.toml      : Rust パッケージのバージョン（cargo metadata 等が参照）
  - src-tauri/Cargo.lock      : yt-browser エントリ（cargo で正規に再生成する）

引数は semver。先頭に v が付いていてもよい（保存時は除去する）。
`latest.json` 側の v 除去は scripts/make_latest_json.py が行うため、
このスクリプトで更新するのは素の semver だけ。

使い方:
  scripts/bump_version.py 0.3.2                          # v0.3.2 でも可
  scripts/bump_version.py --check                        # 4 箇所が揃っているか確認するだけ
  scripts/bump_version.py --check --expect-tag v0.3.2    # 4 箇所とタグの一致も確認
"""

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path
from typing import NoReturn, Sequence

ROOT = Path(__file__).resolve().parent.parent

# semver.org の正規表現（先頭の v だけ追加で許容）
SEMVER = re.compile(
    r"^v?"
    # \d は Unicode 数字も拾うため ASCII の [0-9] で書く
    r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)"
    r"(?:-((?:0|[1-9][0-9]*|[0-9]*[a-zA-Z-][0-9a-zA-Z-]*)"
    r"(?:\.(?:0|[1-9][0-9]*|[0-9]*[a-zA-Z-][0-9a-zA-Z-]*))*))?"
    r"(?:\+([0-9a-zA-Z-]+(?:\.[0-9a-zA-Z-]+)*))?$"
)

# (file テンプレートパス, 書き換えパターン)。各ファイル先頭の 1 箇所だけを更新する
TARGET_PATTERNS = [
    ("src-tauri/tauri.conf.json", re.compile(r'^(\s*)"version": "[^"]*"')),
    ("package.json", re.compile(r'^(\s*)"version": "[^"]*"')),
    ("src-tauri/Cargo.toml", re.compile(r'^(version = ")[^"]*"')),
]


def fail(msg: str) -> NoReturn:
    sys.exit(f"bump_version 失敗: {msg}")


CARGO_LOCK_PKG_RE = re.compile(
    r'(\[\[package\]\]\r?\nname = "yt-browser"\r?\nversion = )"([^"]*)"'
)


def current_versions_lock(lock_path: Path) -> str | None:
    """Cargo.lock から yt-browser パッケージの現行バージョンを読む。"""
    lock_text = lock_path.read_text(encoding="utf-8")
    m = CARGO_LOCK_PKG_RE.search(lock_text)
    return m.group(2) if m else None


def replace_cargo_lock_version(lock_path: Path, version: str) -> None:
    """cargo update が使えない環境向けに Cargo.lock 内の yt-browser バージョンを書き換える。"""
    with lock_path.open("r", encoding="utf-8", newline="") as f:
        text = f.read()
    new_text, count = CARGO_LOCK_PKG_RE.subn(rf'\g<1>"{version}"', text)
    if count != 1:
        fail(
            f"Cargo.lock 内の yt-browser エントリが一意に見つかりません（一致数: {count}）"
        )
    with lock_path.open("w", encoding="utf-8", newline="") as f:
        f.write(new_text)


def current_versions(root: Path = ROOT) -> dict[str, str | None]:
    """4 箇所の現行バージョンを読む。"""
    conf = json.loads((root / "src-tauri/tauri.conf.json").read_text(encoding="utf-8"))
    pkg = json.loads((root / "package.json").read_text(encoding="utf-8"))
    cargo = None
    for line in (root / "src-tauri/Cargo.toml").read_text(encoding="utf-8").splitlines():
        m = re.match(r'^version = "([^"]*)"', line)
        if m:
            cargo = m.group(1)
            break
    lock = current_versions_lock(root / "src-tauri/Cargo.lock")
    return {
        "tauri.conf.json": conf.get("version"),
        "package.json": pkg.get("version"),
        "Cargo.toml": cargo,
        "Cargo.lock": lock,
    }


def update_lock(version: str, root: Path = ROOT) -> None:
    """Cargo.lock を cargo で再生成する。失敗・cargo 不在時は yt-browser 行だけ直接書き換える。"""
    # yt-browser はワークスペースのローカルパッケージだが、依存解決には
    # crates.io の索引・クレートが要るため、キャッシュの無い新規環境では
    # --offline が失敗する。失敗したらネットワークありで再試行し、
    # それも通らない環境では yt-browser 自体の version 行だけを直接書き換える
    last_err = ""
    for extra in (["--offline"], []):
        try:
            r = subprocess.run(
                ["cargo", "update", *extra, "-p", "yt-browser"],
                cwd=root / "src-tauri",
                capture_output=True,
                text=True,
                encoding="utf-8",
            )
        except FileNotFoundError as e:
            last_err = str(e)
            break
        if r.returncode == 0:
            return
        last_err = r.stderr or r.stdout
    print(
        f"警告: cargo update に失敗したため Cargo.lock を直接更新します: {last_err.strip()}",
        file=sys.stderr,
    )
    replace_cargo_lock_version(root / "src-tauri/Cargo.lock", version)


def bump(version: str, root: Path = ROOT, use_cargo: bool = True) -> None:
    # 途中失敗で不整合を残さないよう、先に全ファイルの原文を改行コードごと退避する
    targets = [(root / rel, pat) for rel, pat in TARGET_PATTERNS]
    originals: dict[Path, str] = {}
    for p, _ in targets:
        with p.open("r", encoding="utf-8", newline="") as f:
            originals[p] = f.read()
    try:
        for path, pattern in targets:
            lines = originals[path].splitlines(keepends=True)
            for i, line in enumerate(lines):
                m = pattern.match(line)
                if m:
                    eol = "\r\n" if line.endswith("\r\n") else "\n"
                    if path.name == "Cargo.toml":
                        lines[i] = f'{m.group(1)}{version}"{eol}'
                    else:
                        lines[i] = f'{m.group(1)}"version": "{version}",{eol}'
                    break
            else:
                fail(f"version 行が見つかりません: {path.relative_to(root)}")
            with path.open("w", encoding="utf-8", newline="") as f:
                f.write("".join(lines))
        if use_cargo:
            update_lock(version, root=root)
        else:
            replace_cargo_lock_version(root / "src-tauri/Cargo.lock", version)
    except SystemExit:
        for path, text in originals.items():
            with path.open("w", encoding="utf-8", newline="") as f:
                f.write(text)
        raise


def check_versions(expect_tag: str | None = None, root: Path = ROOT) -> str:
    """4 箇所の一致および任意のタグ文字列との一致を確認して現行バージョンを返す。"""
    versions = current_versions(root=root)
    for name, v in versions.items():
        print(f"  {name}: {v}")
    if not all(versions.values()) or len(set(versions.values())) != 1:
        fail("バージョンが揃っていません")
    actual = versions["Cargo.lock"]
    assert actual is not None
    if expect_tag is not None:
        if not expect_tag.startswith("v") or not SEMVER.match(expect_tag):
            fail(f"タグ形式が不正です（v<semver> を期待）: {expect_tag!r}")
        expected = expect_tag.removeprefix("v")
        if actual != expected:
            fail(
                f"タグ {expect_tag}（期待 {expected}）とファイルバージョン {actual} が一致しません"
            )
        print(f"OK: 4 箇所すべて {actual}（タグ {expect_tag} と一致）")
    else:
        print(f"OK: 4 箇所すべて {actual}")
    return actual


def main(argv: Sequence[str] | None = None) -> None:
    parser = argparse.ArgumentParser(
        usage="bump_version.py <X.Y.Z|vX.Y.Z> | --check [--expect-tag vX.Y.Z]"
    )
    parser.add_argument("version", nargs="?", help="設定する semver（v 接頭辞可）")
    parser.add_argument(
        "--check", action="store_true", help="4 箇所が揃っているか確認するだけ"
    )
    parser.add_argument(
        "--expect-tag",
        default=None,
        help="--check と併用し、タグ名（例: v0.11.9）と一致するかも確認する",
    )
    args = parser.parse_args(list(argv) if argv is not None else None)

    if args.check:
        if args.version is not None:
            fail("--check とバージョン指定は同時に使えません")
        check_versions(expect_tag=args.expect_tag)
        return

    if args.expect_tag is not None:
        fail("--expect-tag は --check と併用してください")

    if args.version is None or not SEMVER.match(args.version):
        fail("usage: bump_version.py <X.Y.Z|vX.Y.Z> | --check [--expect-tag vX.Y.Z]")
    version = args.version.removeprefix("v")

    bump(version)
    versions = current_versions()
    if set(versions.values()) != {version}:
        fail(f"更新後も揃いません: {versions}")
    print(f"バージョンを {version} に更新しました: {', '.join(versions)}")
    print("この変更をコミットしてから `git tag v" + version + "` でリリースできます")


if __name__ == "__main__":
    main()
