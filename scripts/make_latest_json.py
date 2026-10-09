#!/usr/bin/env python3
"""updater の latest.json を生成する（FR-15、仕様決定 AB）。

対象は AppImage（linux-x86_64）と NSIS（windows-x86_64）のみ。
deb / rpm / MSI 用のプラットフォームキーは書かないので、それらの
バンドル形態からの check() はマッチする更新を持たず手動更新運用に留まる。

並列に走った各 OS ビルドの成果物を release ジョブで一括に載せるため、
両 OS のエントリが必ず揃ったマニフェストになる
（tauri-action 任せにすると各ジョブが最新の release アセットを読み
書き換えするため、片方のエントリが失われる競合がある）。

使い方: make_latest_json.py <owner/repo> <tag> <dist_dir> > latest.json
"""

import json
import sys
import urllib.parse
from datetime import datetime, timezone
from pathlib import Path

# updater 対象の成果物パターン → latest.json のプラットフォームキー
PLATFORMS = {
    "linux-x86_64": "*.AppImage.tar.gz",
    "windows-x86_64": "*-setup.exe",
}


def fail(msg: str) -> None:
    sys.exit(f"latest.json 生成失敗: {msg}")


def main() -> None:
    if len(sys.argv) != 4:
        fail("usage: make_latest_json.py <owner/repo> <tag> <dist_dir>")
    repo, tag, dist = sys.argv[1], sys.argv[2], Path(sys.argv[3])
    if not dist.is_dir():
        fail(f"成果物ディレクトリがありません: {dist}")

    platforms = {}
    for key, pattern in PLATFORMS.items():
        bundles = sorted(dist.glob(pattern))
        if not bundles:
            fail(f"{key} の成果物が見つかりません: {pattern}")
        bundle = bundles[0]
        sig = bundle.with_name(bundle.name + ".sig")
        if not sig.is_file():
            fail(
                f"署名ファイルがありません: {sig.name}"
                "（TAURI_SIGNING_PRIVATE_KEY 未設定または署名失敗）"
            )
        platforms[key] = {
            "signature": sig.read_text().strip(),
            "url": (
                f"https://github.com/{repo}/releases/download/{tag}/"
                f"{urllib.parse.quote(bundle.name)}"
            ),
        }

    manifest = {
        "version": tag,
        "notes": "",
        "pub_date": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "platforms": platforms,
    }
    json.dump(manifest, sys.stdout, ensure_ascii=False, indent=2)
    sys.stdout.write("\n")


if __name__ == "__main__":
    main()
