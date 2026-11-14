#!/usr/bin/env python3
"""CDP 接続つきの隔離 dev インスタンスを起動する補助（FR-84、仕様決定 DI）。

本番インストールと同居させるため、次の 3 点を分離して `pnpm tauri dev`
を起動する。

1. Native Messaging ホスト登録の抑制
   `src-tauri/src/lib.rs` の `native_host::register(...)` 呼び出しを
   一時的にコメント化する。開発ビルドも通常どおり自己登録を行うため
   （設計書 §3.4.1「最後に起動したバイナリのパスが有効になる」）、
   抑制しないとホスト定義が dev バイナリを指して本番側の拡張連携を
   壊す。元の内容は `src-tauri/src/lib.rs.e2e-backup` に退避し、
   終了時に必ず復元する。

2. identifier の分離
   `tauri dev --config <一時 JSON>` で identifier を
   `io.github.tkg-tamagohan.yt-browser-dev` へ差し替える。アプリの
   データディレクトリと single-instance の排他範囲が本番と分かれ、
   共存起動と dev 専用 DB が得られる。

3. CDP ポートの開放
   `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS` に
   `--remote-debugging-port=<port>` を足して WebView2 の CDP
   エンドポイントを開く。Windows / WebView2 前提。

使い方:
    python3 scripts/e2e_dev.py                # port 9222 で起動
    python3 scripts/e2e_dev.py --port 9333
    python3 scripts/e2e_dev.py --dry-run      # 実施内容だけ表示
    python3 scripts/e2e_dev.py --no-suppress-nm  # NM 登録抑制を掛けない

起動したら別ターミナルで `python3 scripts/e2e_smoke.py --port <port>`。
Ctrl+C で dev プロセスを止めると lib.rs は自動復元される。
スクリプトが強制終了された場合は `git checkout -- src-tauri/src/lib.rs`
か `lib.rs.e2e-backup` から手で戻す。

依存は Python 3.10+ の標準ライブラリのみ。

暫定解釈（実アプリ未検証の点を含む）:
- `tauri dev --config` の JSON マージで identifier だけ差し替える
  運用は CLI の仕様上可能と確認したが、実機でのビルド通過は未検証。
- dev identifier のデータディレクトリは初回が空のため、初回起動では
  セットアップウィザード（FR-75）が出る想定。smoke は DOM 参照のみで
  ウィザードを閉じなくても動く。
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
import urllib.request
from pathlib import Path
from typing import NoReturn

ROOT = Path(__file__).resolve().parent.parent
LIB_RS = ROOT / "src-tauri" / "src" / "lib.rs"
BACKUP = LIB_RS.with_name("lib.rs.e2e-backup")

DEFAULT_PORT = 9222
DEFAULT_DEV_IDENTIFIER = "io.github.tkg-tamagohan.yt-browser-dev"

# 抑制対象の呼び出し（src-tauri/src/lib.rs の setup 内）。
# この 1 文を丸ごとコメント化する。接頭のコメントを目印に二重適用を防ぐ。
NM_REGISTER_ANCHOR = "native_host::register(app.path().config_dir().ok(), &dir);"
NM_PATCH_MARK = "[e2e_dev]"


def fail(msg: str, code: int = 1) -> NoReturn:
    print(f"e2e_dev 失敗: {msg}", file=sys.stderr)
    sys.exit(code)


def port_responds(port: int) -> bool:
    try:
        with urllib.request.urlopen(
            f"http://127.0.0.1:{port}/json/version", timeout=2
        ) as r:
            return r.status == 200
    except OSError:
        return False


def apply_nm_patch() -> None:
    """lib.rs の register 呼び出しをコメント化し、退避ファイルを作る。"""
    text = LIB_RS.read_text(encoding="utf-8")
    if NM_PATCH_MARK in text:
        fail(
            f"{LIB_RS.relative_to(ROOT)} には既に e2e_dev の抑制が掛かっている。"
            "前回の終了処理が走っていない可能性があるため、バックアップか"
            " git checkout で戻してからやり直す"
        )
    if text.count(NM_REGISTER_ANCHOR) != 1:
        fail(
            f"{LIB_RS.relative_to(ROOT)} から抑制対象の"
            f" `{NM_REGISTER_ANCHOR}` が 1 箇所だけ見つからない"
            "（lib.rs の構成が変わった可能性。--no-suppress-nm で回避できるが"
            "ホスト定義の上書きには注意）"
        )
    BACKUP.write_text(text, encoding="utf-8")
    patched = text.replace(
        NM_REGISTER_ANCHOR,
        f"// {NM_PATCH_MARK} 一時抑制（e2e_dev 終了時に自動復元）\n"
        f"            // {NM_REGISTER_ANCHOR}",
    )
    LIB_RS.write_text(patched, encoding="utf-8")
    print(f"[e2e_dev] NM 登録を一時抑制: {LIB_RS.relative_to(ROOT)}"
          f"（退避: {BACKUP.name}）")


def restore_nm_patch() -> None:
    """退避ファイルがあれば lib.rs を元の内容へ戻す。"""
    if BACKUP.is_file():
        LIB_RS.write_text(BACKUP.read_text(encoding="utf-8"), encoding="utf-8")
        BACKUP.unlink()
        print(f"[e2e_dev] {LIB_RS.relative_to(ROOT)} を復元した")


def pnpm_invocation(tail: list[str]) -> tuple[list[str] | str, bool]:
    """`pnpm <tail>` の起動形を返す。(argv またはコマンド文字列, shell 使用か)。

    Windows の pnpm は `pnpm.cmd` シムのため cmd 経由で起動する。引数を
    list2cmdline で組み立てて `pnpm` の PATH 解決に任せる形にし、
    `cmd /c` へ argv リストで渡すと起こり得る引用符剥がし問題を避ける。
    """
    if shutil.which("pnpm") is None:
        fail("pnpm が見つからない（node_modules 未導入なら `pnpm install` も必要）")
    if os.name == "nt":
        return subprocess.list2cmdline(["pnpm", *tail]), True
    return ["pnpm", *tail], False


def write_override_config(identifier: str) -> Path:
    """identifier 差し替え用の --config JSON を一時ファイルに書く。"""
    fd, name = tempfile.mkstemp(
        prefix="yt-browser-e2e-", suffix=".conf.json"
    )
    with os.fdopen(fd, "w", encoding="utf-8") as f:
        json.dump({"identifier": identifier}, f)
    return Path(name)


def kill_tree(proc: subprocess.Popen) -> None:
    """起動した dev プロセスのツリーを止める。"""
    if proc.poll() is not None:
        return
    if os.name == "nt":
        # cmd /c → pnpm → cargo → アプリの子孫まで畳む
        subprocess.run(
            ["taskkill", "/PID", str(proc.pid), "/T", "/F"],
            capture_output=True,
        )
    else:
        try:
            proc.terminate()
            proc.wait(timeout=10)
        except (subprocess.TimeoutExpired, OSError):
            proc.kill()


def main() -> int:
    parser = argparse.ArgumentParser(
        description="CDP 付きの隔離 dev インスタンスを起動する（FR-84）"
    )
    parser.add_argument("--port", type=int, default=DEFAULT_PORT)
    parser.add_argument(
        "--identifier",
        default=DEFAULT_DEV_IDENTIFIER,
        help="dev 用 identifier（既定: 本番 + '-dev'）",
    )
    parser.add_argument(
        "--wait",
        type=float,
        default=600.0,
        help="CDP ポート応答の待ち秒数（初回ビルドは長い。既定 600）",
    )
    parser.add_argument(
        "--no-suppress-nm",
        action="store_true",
        help="Native Messaging 登録の抑制を掛けない（ホスト定義が dev を指すので非推奨）",
    )
    parser.add_argument(
        "--dry-run", action="store_true", help="変更・起動をせず実施内容だけ表示"
    )
    args = parser.parse_args()

    if not LIB_RS.is_file():
        fail(f"{LIB_RS.relative_to(ROOT)} が見つからない")

    print("[e2e_dev] 実施内容:")
    suppress = "あり（lib.rs を一時コメント化）"
    if args.no_suppress_nm:
        suppress = "なし（--no-suppress-nm）"
    print(f"  - NM 登録抑制パッチ: {suppress}")
    print(f"  - identifier 差し替え: {args.identifier}")
    print(
        "  - WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS +="
        f" --remote-debugging-port={args.port}"
    )
    print(f"  - 起動: {pnpm_argv_fallback()} tauri dev --config <一時 JSON>")

    if args.dry_run:
        return 0

    if port_responds(args.port):
        fail(
            f"port {args.port} の CDP エンドポイントは既に応答している。"
            "起動中の dev インスタンスがあるか、別プロセスが占有している。"
            "--port を変えるか既存側へ `e2e_smoke.py` / `cdp_client.py` で接続する",
            code=2,
        )

    env = dict(os.environ)
    extra = f"--remote-debugging-port={args.port}"
    existing = env.get("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS", "")
    env["WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS"] = (
        f"{existing} {extra}".strip()
    )

    cfg: Path | None = None
    proc: subprocess.Popen | None = None
    try:
        cfg = write_override_config(args.identifier)
        cmd, use_shell = pnpm_invocation(
            ["tauri", "dev", "--config", str(cfg)]
        )
        if not args.no_suppress_nm:
            apply_nm_patch()
        proc = subprocess.Popen(cmd, cwd=ROOT, env=env, shell=use_shell)
        print(f"[e2e_dev] dev を起動した（pid {proc.pid}）。CDP 応答を待つ…")

        deadline = time.monotonic() + args.wait
        while not port_responds(args.port):
            if proc.poll() is not None:
                fail(f"dev プロセスが {proc.returncode} で終了した", code=2)
            if time.monotonic() >= deadline:
                fail(f"CDP ポートが {args.wait:.0f} 秒以内に応答しなかった", code=2)
            time.sleep(2)

        print(f"[e2e_dev] CDP 応答あり: http://127.0.0.1:{args.port}/json")
        print("[e2e_dev] 別ターミナルで `python3 scripts/e2e_smoke.py"
              f" --port {args.port}` を実行できる。Ctrl+C で終了")
        proc.wait()
        return proc.returncode or 0
    except KeyboardInterrupt:
        print("\n[e2e_dev] 中断。dev プロセスを止める")
        return 130
    finally:
        if proc is not None:
            kill_tree(proc)
        restore_nm_patch()
        if cfg is not None:
            cfg.unlink(missing_ok=True)


def pnpm_argv_fallback() -> str:
    """--dry-run 表示用（pnpm 未導入でも失敗しないよう which だけ調べる）。"""
    return shutil.which("pnpm") or "pnpm"


if __name__ == "__main__":
    sys.exit(main())
