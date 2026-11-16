#!/usr/bin/env python3
"""リリース公開直前に dist/ の配布成果物を検証する（仕様決定 AB・AK・DN・DQ）。

release.yml の release ジョブで `rm -f dist/*.sig` を終えた後、
`gh release create` / `gh release upload` の直前に実行する。

検証内容:
  1. `latest.json` が有効な JSON で、`version` が `<tag>`（先頭 v 除去）と一致する
  2. `latest.json` の `platforms` に `linux-x86_64` と `windows-x86_64` があり、
     非空の `signature` と `https://github.com/<repo>/releases/download/<tag>/...`
     形式の `url` を持ち、URL のファイル名に対応する成果物が `dist/` に実在する
  3. `dist/` に配布廃止・添付除外の `.msi` や `.sig` が残っていない
  4. Linux パッケージ（`.deb` と `.rpm`）および拡張 zip（`yt-browser-extension-<tag>.zip`）
     が存在し、拡張 zip 内に `manifest.json`（有効 JSON）があり、`*.test.js` を含まない

使い方: verify_release.py <owner/repo> <tag> <dist_dir>
"""

from __future__ import annotations

import json
import sys
import urllib.parse
import zipfile
from pathlib import Path
from typing import NoReturn, Sequence


def fail(msg: str) -> NoReturn:
    sys.exit(f"リリース成果物検証失敗: {msg}")


def verify_dist(repo: str, tag: str, dist: Path) -> None:
    """dist ディレクトリ内のリリース成果物を検証する。"""
    if not dist.is_dir():
        fail(f"成果物ディレクトリがありません: {dist}")

    version = tag.removeprefix("v").strip()
    if not version:
        fail(f"タグが不正です: {tag!r}")

    # 1. 禁止ファイルの残存チェック（仕様決定 AK）
    forbidden = sorted(
        p.name for p in dist.iterdir()
        if p.is_file() and p.suffix in {".msi", ".sig"}
    )
    if forbidden:
        fail(f"配布除外ファイルが dist に残っています: {', '.join(forbidden)}")

    # 2. latest.json 検証
    latest_path = dist / "latest.json"
    if not latest_path.is_file():
        fail("latest.json が見つかりません")
    try:
        manifest = json.loads(latest_path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as e:
        fail(f"latest.json の JSON が不正です: {e}")

    if manifest.get("version") != version:
        fail(
            f"latest.json の version ({manifest.get('version')!r}) が "
            f"タグ ({version!r}) と一致しません"
        )
    platforms = manifest.get("platforms")
    if not isinstance(platforms, dict):
        fail("latest.json の platforms がオブジェクトではありません")

    expected_prefix = f"https://github.com/{repo}/releases/download/{tag}/"
    for key in ("linux-x86_64", "windows-x86_64"):
        entry = platforms.get(key)
        if not isinstance(entry, dict):
            fail(f"latest.json の platforms.{key} がありません")
        sig = entry.get("signature")
        if not isinstance(sig, str) or not sig.strip():
            fail(f"latest.json の platforms.{key}.signature が空です")
        url = entry.get("url")
        if not isinstance(url, str) or not url.startswith(expected_prefix):
            fail(
                f"latest.json の platforms.{key}.url が期待形式ではありません: {url!r}"
            )
        filename = urllib.parse.unquote(url.removeprefix(expected_prefix))
        if not filename or "/" in filename or not (dist / filename).is_file():
            fail(f"latest.json が参照する成果物が dist にありません: {filename!r}")

    # 3. deb / rpm の存在確認（仕様決定 AB）
    for ext in ("*.deb", "*.rpm"):
        if not list(dist.glob(ext)):
            fail(f"配布パッケージが見つかりません: {ext}")

    # 4. Chrome 拡張 zip の内容確認（仕様決定 DN）
    ext_zip = dist / f"yt-browser-extension-{tag}.zip"
    if not ext_zip.is_file():
        fail(f"拡張 zip が見つかりません: {ext_zip.name}")
    try:
        with zipfile.ZipFile(ext_zip, "r") as zf:
            names = [n for n in zf.namelist() if not n.endswith("/")]
            if "manifest.json" not in names:
                fail(f"{ext_zip.name} に manifest.json がありません")
            try:
                json.loads(zf.read("manifest.json").decode("utf-8"))
            except (UnicodeDecodeError, json.JSONDecodeError) as e:
                fail(f"{ext_zip.name} 内の manifest.json が不正です: {e}")
            test_files = [n for n in names if n.endswith(".test.js")]
            if test_files:
                fail(
                    f"{ext_zip.name} にテストファイルが混入しています: "
                    f"{', '.join(test_files)}"
                )
    except zipfile.BadZipFile as e:
        fail(f"{ext_zip.name} が不正な zip です: {e}")


def main(argv: Sequence[str] | None = None) -> int:
    args = list(sys.argv[1:] if argv is None else argv)
    if len(args) != 3:
        fail("usage: verify_release.py <owner/repo> <tag> <dist_dir>")
    verify_dist(args[0], args[1], Path(args[2]))
    print(f"OK: {args[2]} 内の全リリース成果物と latest.json を検証しました（{args[1]}）")
    return 0


if __name__ == "__main__":
    sys.exit(main())
