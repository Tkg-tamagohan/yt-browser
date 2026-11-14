#!/usr/bin/env python3
"""起動中の dev インスタンスへ CDP でアタッチし、主要画面の DOM smoke を実行する
（FR-84、仕様決定 DI）。

前提: `e2e_dev.py` などで `--remote-debugging-port` 付き起動した yt-browser。
実施するのは DOM の存在確認とナビ遷移の往復のみで、再生・取り込み・
設定変更などの状態変更は行わない（キューパネルの開閉だけを除く）。

チェック項目は SMK-NN の ID で管理する:

- SMK-01: CDP ターゲット接続（/json/list → page → WS → 評価が通る）
- SMK-02: アプリシェル `nav.app-nav` の存在
- SMK-03: ホーム `/` の `.play-form`
- SMK-04: フィード `/feed` の `.feed-grid`
- SMK-05: 検索 `/search` の `.search-form`
- SMK-06: ライブラリ `/library` の `main.container.library` と `nav.tabs`
- SMK-07: 設定 `/settings` の `.section-filter`
- SMK-08: `.nav-queue` クリックでキューパネルの包み div の `hidden` が外れ・再クリックで戻る
  （`aside.queue-panel` 自体は常時マウントで display が変わらない）
- SMK-09: 走査中に `Runtime.exceptionThrown` を拾わない（報告のみ・失敗扱いにしない）

使い方:
    python3 scripts/e2e_smoke.py                 # port 9222
    python3 scripts/e2e_smoke.py --port 9333 --timeout 20
    python3 scripts/e2e_smoke.py --url-match localhost:1420

終了コード: 全項目 OK で 0、1 件でも NG で 1、CDP に繋がらない等の
環境エラーで 2。

依存は `scripts/cdp_client.py`（同梱）と標準ライブラリのみ。
実アプリでの実行は未検証。セレクタは現行 UI の構造に合わせた暫定解釈で、
画面構造が変わったときはここを更新する。
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from cdp_client import CdpError, CdpSession, list_targets, pick_page_target  # noqa: E402

# (route, 確認セレクタの JS 式)。式は要素存在で truthy を返す
PAGE_CHECKS = [
    ("SMK-03", "ホーム /", "/", 'document.querySelector("main.container .play-form") !== null'),
    ("SMK-04", "フィード /feed", "/feed", 'document.querySelector(".feed-grid") !== null'),
    ("SMK-05", "検索 /search", "/search", 'document.querySelector(".search-form") !== null'),
    (
        "SMK-06",
        "ライブラリ /library",
        "/library",
        'document.querySelector("main.container.library nav.tabs") !== null',
    ),
    ("SMK-07", "設定 /settings", "/settings", 'document.querySelector(".section-filter") !== null'),
]


def click_nav(session: CdpSession, href: str) -> None:
    """ナビのリンクを JS クリックして SPA 遷移させる。"""
    expr = (
        f'document.querySelector(\'nav.app-nav a[href="{href}"]\')'
        ".click()"
    )
    found = session.evaluate(
        f'document.querySelector(\'nav.app-nav a[href="{href}"]\') !== null'
    )
    if not found:
        raise CdpError(f"ナビに href={href} のリンクが無い")
    session.evaluate(expr)


def main() -> int:
    parser = argparse.ArgumentParser(
        description="dev インスタンスへの DOM smoke チェック（FR-84）"
    )
    parser.add_argument("--port", type=int, default=9222)
    parser.add_argument(
        "--url-match",
        default=None,
        help="複数 page があるとき URL に含まれる文字列で対象を選ぶ"
        "（dev 起動なら localhost:1420 が目印）",
    )
    parser.add_argument(
        "--timeout",
        type=float,
        default=15.0,
        help="画面遷移・要素出現の待ち秒数（既定 15）",
    )
    args = parser.parse_args()

    results: list[tuple[str, str, str]] = []  # (id, 項目, OK/NG/WARN)

    # SMK-01: CDP アタッチ
    try:
        targets = list_targets(args.port)
        target = pick_page_target(targets, args.url_match)
        if target is None:
            raise CdpError(
                f"page ターゲットが無い（targets: {[t.get('type') for t in targets]}）"
            )
        session = CdpSession(target["webSocketDebuggerUrl"])
        session.evaluate("1 + 1")
        results.append(
            ("SMK-01", f"CDP アタッチ（{target.get('title') or target.get('url')}）", "OK")
        )
    except CdpError as e:
        print(f"[ENV] SMK-01 CDP アタッチ: {e}", file=sys.stderr)
        print("dev インスタンスが --remote-debugging-port 付きで"
              "起動しているか確認（scripts/e2e_dev.py）", file=sys.stderr)
        return 2

    try:
        session.command("Runtime.enable")
        t = args.timeout

        def check(smk_id: str, name: str, ok: bool, detail: str = "") -> None:
            results.append((smk_id, name, "OK" if ok else "NG"))
            suffix = f"（{detail}）" if detail and not ok else ""
            print(f"[{'OK' if ok else 'NG'}] {smk_id} {name}{suffix}")

        # SMK-02: アプリシェル
        shell = session.wait_eval(
            'document.querySelector("nav.app-nav") !== null', timeout=t
        )
        check("SMK-02", "アプリシェル nav.app-nav", bool(shell))

        # SMK-03〜07: ナビ遷移と各画面の代表要素
        for smk_id, name, href, expr in PAGE_CHECKS:
            try:
                click_nav(session, href)
                ok = bool(session.wait_eval(expr, timeout=t))
            except CdpError as e:
                ok = False
                print(f"      {smk_id} 遷移中のエラー: {e}")
            check(smk_id, f"{name} の代表要素", ok)

        # SMK-08: キューパネルの開閉
        try:
            has_button = session.evaluate(
                'document.querySelector(".nav-queue") !== null'
            )
            if not has_button:
                raise CdpError(".nav-queue ボタンが無い")
            session.evaluate('document.querySelector(".nav-queue").click()')
            opened = bool(
                session.wait_eval(
                    '(() => { const el = document.querySelector("aside.queue-panel");'
                    ' return el !== null && getComputedStyle(el.parentElement).display !== "none" })()',
                    timeout=t,
                )
            )
            session.evaluate('document.querySelector(".nav-queue").click()')
            closed = session.wait_eval(
                '(() => { const el = document.querySelector("aside.queue-panel");'
                ' return el === null || getComputedStyle(el.parentElement).display === "none" })()',
                timeout=t,
            )
            ok = opened and bool(closed)
            detail = f"open={opened} close={closed}"
        except CdpError as e:
            ok = False
            detail = str(e)
        check("SMK-08", "キューパネルの開閉", ok, detail)

        # SMK-09: 走査中の例外（報告のみ）
        exceptions = session.drain_events("Runtime.exceptionThrown")
        results.append(("SMK-09", "走査中の例外なし", "OK" if not exceptions else "WARN"))
        for ev in exceptions[:5]:
            detail = ev.get("params", {}).get("exceptionDetails", {})
            exc = detail.get("exception") or {}
            print(f"[WARN] SMK-09 exceptionThrown: {exc.get('description', detail.get('text', ''))[:200]}")
    finally:
        session.close()

    print()
    print("| ID | 項目 | 結果 |")
    print("|----|------|------|")
    for smk_id, name, status in results:
        print(f"| {smk_id} | {name} | {status} |")

    ng = [r for r in results if r[2] == "NG"]
    print()
    print(f"{len(results) - len(ng)} / {len(results)} 項目 OK")
    return 1 if ng else 0


if __name__ == "__main__":
    sys.exit(main())
