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
SEMVER = re.compile(r"^v?\d+\.\d+\.\d+([-+][0-9A-Za-z.-]+)?$")

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


def bump(version: str) -> None:
    for path, pattern in TEXT_TARGETS:
        lines = path.read_text().splitlines(keepends=True)
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

    # Cargo.lock は cargo 自身に再生成させる（ハッシュや依存解決を正規に保つ）。
    # yt-browser はワークスペースのローカルパッケージなのでネットワーク不要
    r = subprocess.run(
        ["cargo", "update", "--offline", "-p", "yt-browser"],
        cwd=ROOT / "src-tauri",
        capture_output=True,
        text=True,
    )
    if r.returncode != 0:
        fail(
            "Cargo.lock の更新に失敗しました（cargo が無い環境では "
            "Cargo.lock 内 yt-browser エントリの version を手で書き換えてください）\n"
            + (r.stderr or r.stdout)
        )


def main() -> None:
    args = sys.argv[1:]
    if args == ["--check"]:
        versions = current_versions()
        uniq = set(v for v in versions.values() if v)
        for name, v in versions.items():
            print(f"  {name}: {v}")
        if len(uniq) != 1 or None in versions.values():
            fail("バージョンが揃っていません")
        print(f"OK: 4 箇所すべて {uniq.pop()}")
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
