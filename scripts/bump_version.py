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
  scripts/bump_version.py 0.3.2      # v0.3.2 でも可
  scripts/bump_version.py --check    # 4 箇所が揃っているか確認するだけ
"""

import json
import re
import subprocess
import sys
from pathlib import Path

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

# (file, 書き換えパターン)。各ファイル先頭の 1 箇所だけを更新する
TEXT_TARGETS = [
    (ROOT / "src-tauri/tauri.conf.json", re.compile(r'^(\s*)"version": "[^"]*"')),
    (ROOT / "package.json", re.compile(r'^(\s*)"version": "[^"]*"')),
    (ROOT / "src-tauri/Cargo.toml", re.compile(r'^(version = ")[^"]*"')),
]


def fail(msg: str) -> None:
    sys.exit(f"bump_version 失敗: {msg}")


def current_versions() -> dict[str, str | None]:
    """4 箇所の現行バージョンを読む。"""
    conf = json.loads((ROOT / "src-tauri/tauri.conf.json").read_text())
    pkg = json.loads((ROOT / "package.json").read_text())
    cargo = None
    for line in (ROOT / "src-tauri/Cargo.toml").read_text().splitlines():
        m = re.match(r'^version = "([^"]*)"', line)
        if m:
            cargo = m.group(1)
            break
    lock = None
    lock_text = (ROOT / "src-tauri/Cargo.lock").read_text()
    m = re.search(r'\[\[package\]\]\nname = "yt-browser"\nversion = "([^"]*)"', lock_text)
    if m:
        lock = m.group(1)
    return {
        "tauri.conf.json": conf.get("version"),
        "package.json": pkg.get("version"),
        "Cargo.toml": cargo,
        "Cargo.lock": lock,
    }


def update_lock() -> None:
    """Cargo.lock を cargo で再生成する。オフラインが失敗したら通常モードへフォールバック。"""
    # yt-browser はワークスペースのローカルパッケージだが、依存解決には
    # crates.io の索引・クレートが要るため、キャッシュの無い新規環境では
    # --offline が失敗する。失敗したらネットワークありで再試行する
    # （通常環境では --offline で完結しネットワークは不要）
    for extra in (["--offline"], []):
        r = subprocess.run(
            ["cargo", "update", *extra, "-p", "yt-browser"],
            cwd=ROOT / "src-tauri",
            capture_output=True,
            text=True,
        )
        if r.returncode == 0:
            return
    fail(
        "Cargo.lock の更新に失敗しました（cargo が無い環境では "
        "Cargo.lock 内 yt-browser エントリの version を手で書き換えてください）\n"
        + (r.stderr or r.stdout)
    )


def bump(version: str) -> None:
    # 途中失敗で不整合を残さないよう、先に全ファイルの原文を退避する
    originals = {p: p.read_text() for p, _ in TEXT_TARGETS}
    try:
        for path, pattern in TEXT_TARGETS:
            lines = originals[path].splitlines(keepends=True)
            for i, line in enumerate(lines):
                m = pattern.match(line)
                if m:
                    if path.name == "Cargo.toml":
                        lines[i] = f'{m.group(1)}{version}"\n'
                    else:
                        lines[i] = f'{m.group(1)}"version": "{version}",\n'
                    break
            else:
                fail(f"version 行が見つかりません: {path.relative_to(ROOT)}")
            path.write_text("".join(lines))
        update_lock()
    except SystemExit:
        for path, text in originals.items():
            path.write_text(text)
        raise


def main() -> None:
    args = sys.argv[1:]
    if args == ["--check"]:
        versions = current_versions()
        for name, v in versions.items():
            print(f"  {name}: {v}")
        if not all(versions.values()) or len(set(versions.values())) != 1:
            fail("バージョンが揃っていません")
        print(f"OK: 4 箇所すべて {versions['Cargo.lock']}")
        return

    if len(args) != 1 or not SEMVER.match(args[0]):
        fail("usage: bump_version.py <X.Y.Z|vX.Y.Z> | --check")
    version = args[0].removeprefix("v")

    bump(version)
    versions = current_versions()
    if set(versions.values()) != {version}:
        fail(f"更新後も揃いません: {versions}")
    print(f"バージョンを {version} に更新しました: {', '.join(versions)}")
    print("この変更をコミットしてから `git tag v" + version + "` でリリースできます")


if __name__ == "__main__":
    main()
