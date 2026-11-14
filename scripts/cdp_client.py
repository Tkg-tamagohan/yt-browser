#!/usr/bin/env python3
"""起動中の yt-browser dev インスタンスへ CDP で接続する薄いクライアント（FR-84、仕様決定 DI）。

WebView2 を `--remote-debugging-port` 付きで起動すると CDP（Chrome DevTools
Protocol）の HTTP/WebSocket エンドポイントが開く。このモジュールは標準
ライブラリだけでそのエンドポイントへ接続し、ターゲット列挙・
`Runtime.evaluate`・`Page.navigate`・`app://open_url` イベント注入を提供する。

`e2e_dev.py` が CDP 付き dev インスタンスの起動を、`e2e_smoke.py` がこの
クライアントを使った smoke チェックを担う。単体 CLI としても使える。

使い方:
    python3 scripts/cdp_client.py --port 9222 --list-targets
    python3 scripts/cdp_client.py --port 9222 --eval "document.title"
    python3 scripts/cdp_client.py --port 9222 --eval "document.querySelector('nav.app-nav') !== null"
    python3 scripts/cdp_client.py --port 9222 --emit-open-url "https://www.youtube.com/watch?v=<ID>"

依存は Python 3.10+ の標準ライブラリのみ（外部パッケージ不要）。

暫定解釈（実アプリ未検証の点を含む）:
- `--emit-open-url` は `window.__TAURI_INTERNALS__.invoke("plugin:event|emit", ...)`
  経由で `app://open_url` を発行する。emit には `core:event:default`（
  capabilities の `core:default` に同梱）の権限が要る。
- イベント注入で使う `seq` は `Date.now()` を採用する。フロント側は
  `seq` で配送重複を除去するだけなので、プロセス内の Rust 側採番と
  衝突しない大きな値であればよい。
"""

from __future__ import annotations

import argparse
import base64
import hashlib
import json
import os
import socket
import struct
import sys
import time
import urllib.parse
import urllib.request
from collections import deque
from typing import Any

DEFAULT_PORT = 9222


class CdpError(RuntimeError):
    """CDP 接続・コマンド実行の失敗。"""


# ---------------------------------------------------------------------------
# HTTP エンドポイント（/json/*）
# ---------------------------------------------------------------------------


def http_get_json(port: int, path: str, timeout: float = 5.0) -> Any:
    """`http://127.0.0.1:<port><path>` を GET して JSON を返す。"""
    url = f"http://127.0.0.1:{port}{path}"
    try:
        with urllib.request.urlopen(url, timeout=timeout) as resp:
            return json.loads(resp.read().decode("utf-8"))
    except urllib.error.URLError as e:
        raise CdpError(
            f"CDP の HTTP エンドポイント {url} に接続できない"
            "（dev インスタンスが --remote-debugging-port 付きで"
            f"起動しているか確認）: {e}"
        ) from e
    except (OSError, json.JSONDecodeError) as e:
        raise CdpError(f"CDP HTTP {url} の応答を読めない: {e}") from e


def list_targets(port: int, timeout: float = 5.0) -> list[dict]:
    """`/json/list` のターゲット一覧を返す。"""
    targets = http_get_json(port, "/json/list", timeout)
    if not isinstance(targets, list):
        raise CdpError("/json/list の応答が配列でない")
    return targets


def pick_page_target(
    targets: list[dict], url_contains: str | None = None
) -> dict | None:
    """`type == "page"` のターゲットを選ぶ。

    `url_contains` 指定時は URL にその文字列を含むものを優先する
    （複数 page があるときの本体窓の特定用）。見つからなければ
    最初の page を返し、page が無ければ None。
    """
    pages = [t for t in targets if t.get("type") == "page"]
    if url_contains:
        for t in pages:
            if url_contains in t.get("url", ""):
                return t
    return pages[0] if pages else None


# ---------------------------------------------------------------------------
# WebSocket の最小実装（RFC 6455）
# ---------------------------------------------------------------------------

_GUID = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11"


class _WebSocket:
    """CDP 通信用の最小 WebSocket クライアント。

    テキストフレームの送受信・分割メッセージの再接続・ping への pong
    応答・close だけを扱う。ws://（平文）専用。CDP はローカル専用なので
    wss や Origin 検証は扱わない（DevTools 側は Origin ヘッダが無い
    接続を拒否しない前提）。
    """

    def __init__(self, ws_url: str, timeout: float = 30.0):
        parts = urllib.parse.urlsplit(ws_url)
        if parts.scheme != "ws" or not parts.hostname:
            raise CdpError(f"ws:// 以外または不正な URL: {ws_url}")
        self._sock = socket.create_connection(
            (parts.hostname, parts.port or 80), timeout=timeout
        )
        self._buf = bytearray()
        key = base64.b64encode(os.urandom(16)).decode("ascii")
        path = parts.path or "/"
        if parts.query:
            path += "?" + parts.query
        host = parts.hostname
        if parts.port:
            host += f":{parts.port}"
        request = (
            f"GET {path} HTTP/1.1\r\n"
            f"Host: {host}\r\n"
            "Upgrade: websocket\r\n"
            "Connection: Upgrade\r\n"
            f"Sec-WebSocket-Key: {key}\r\n"
            "Sec-WebSocket-Version: 13\r\n"
            "\r\n"
        )
        self._sock.sendall(request.encode("ascii"))
        self._handshake(host, key)

    def _read_exact(self, n: int) -> bytes:
        while len(self._buf) < n:
            chunk = self._sock.recv(65536)
            if not chunk:
                raise CdpError("WebSocket が途中で切断された")
            self._buf += chunk
        out = bytes(self._buf[:n])
        del self._buf[:n]
        return out

    def _handshake(self, host: str, key: str) -> None:
        # 応答ヘッダは \r\n\r\n まで読む。ボディ以降のバイトは
        # _buf に残して最初のフレーム読み取りへ回す
        while b"\r\n\r\n" not in self._buf:
            chunk = self._sock.recv(65536)
            if not chunk:
                raise CdpError("WebSocket ハンドシェイク中に切断された")
            self._buf += chunk
            if len(self._buf) > 16384:
                raise CdpError("WebSocket ハンドシェイク応答が大きすぎる")
        head, _, rest = bytes(self._buf).partition(b"\r\n\r\n")
        self._buf = bytearray(rest)
        lines = head.decode("latin-1").split("\r\n")
        status = lines[0].split(None, 2)
        if len(status) < 2 or status[1] != "101":
            raise CdpError(f"WebSocket アップグレード拒否: {lines[0]}")
        headers = {}
        for line in lines[1:]:
            k, _, v = line.partition(":")
            headers[k.strip().lower()] = v.strip()
        expected = base64.b64encode(
            hashlib.sha1((key + _GUID).encode("ascii")).digest()
        ).decode("ascii")
        if headers.get("sec-websocket-accept") != expected:
            raise CdpError("Sec-WebSocket-Accept が一致しない")

    def send_text(self, text: str) -> None:
        self._send_frame(0x1, text.encode("utf-8"))

    def _send_frame(self, opcode: int, payload: bytes) -> None:
        # クライアント→サーバのフレームは RFC 6455 上マスク必須
        header = bytearray([0x80 | opcode])
        n = len(payload)
        if n < 126:
            header.append(0x80 | n)
        elif n < 65536:
            header.append(0x80 | 126)
            header += struct.pack(">H", n)
        else:
            header.append(0x80 | 127)
            header += struct.pack(">Q", n)
        mask = os.urandom(4)
        header += mask
        masked = bytes(b ^ mask[i % 4] for i, b in enumerate(payload))
        self._sock.sendall(bytes(header) + masked)

    def recv_message(self) -> str:
        """完全なテキストメッセージを 1 つ返す。

        分割フレームは再連結し、ping には pong を返し、close では
        CdpError を投げる。ソケットタイムアウトは socket.timeout の
        まま呼び出し側へ伝播する。
        """
        payload = bytearray()
        while True:
            fin, opcode, data = self._recv_frame()
            if opcode == 0x8:  # close
                self._send_frame(0x8, b"")
                raise CdpError("WebSocket が close された")
            if opcode == 0x9:  # ping → pong
                self._send_frame(0xA, data)
                continue
            if opcode == 0xA:  # pong
                continue
            # 0x0 continuation / 0x1 text / 0x2 binary
            payload += data
            if fin:
                return payload.decode("utf-8")

    def _recv_frame(self) -> tuple[bool, int, bytes]:
        b1, b2 = self._read_exact(2)
        fin = bool(b1 & 0x80)
        opcode = b1 & 0x0F
        masked = bool(b2 & 0x80)
        length = b2 & 0x7F
        if length == 126:
            (length,) = struct.unpack(">H", self._read_exact(2))
        elif length == 127:
            (length,) = struct.unpack(">Q", self._read_exact(8))
        mask = self._read_exact(4) if masked else b""
        data = self._read_exact(length)
        if masked:
            data = bytes(b ^ mask[i % 4] for i, b in enumerate(data))
        return fin, opcode, data

    def set_timeout(self, timeout: float | None) -> None:
        self._sock.settimeout(timeout)

    def close(self) -> None:
        try:
            self._send_frame(0x8, b"")
        except OSError:
            pass
        self._sock.close()


# ---------------------------------------------------------------------------
# CDP セッション
# ---------------------------------------------------------------------------


class CdpSession:
    """page ターゲットの webSocketDebuggerUrl へ接続した CDP セッション。

    `command()` の応答待ち中に届いたイベントは `events` キューへ
    溜める（`Runtime.enable` 後の exceptionThrown 収集などに使う）。
    """

    def __init__(self, ws_url: str, timeout: float = 30.0):
        self._ws = _WebSocket(ws_url, timeout=timeout)
        self._next_id = 0
        self.events: deque[dict] = deque()

    @classmethod
    def attach(
        cls,
        port: int = DEFAULT_PORT,
        url_contains: str | None = None,
        timeout: float = 30.0,
    ) -> "CdpSession":
        """`--remote-debugging-port` を開いているインスタンスの page へ接続。"""
        targets = list_targets(port)
        target = pick_page_target(targets, url_contains)
        if target is None:
            raise CdpError(
                f"port {port} に page ターゲットが無い"
                f"（targets: {[t.get('type') for t in targets]}）"
            )
        ws_url = target.get("webSocketDebuggerUrl")
        if not ws_url:
            raise CdpError("page ターゲットに webSocketDebuggerUrl が無い")
        return cls(ws_url, timeout=timeout)

    def command(
        self, method: str, params: dict | None = None, timeout: float = 30.0
    ) -> dict:
        self._next_id += 1
        mid = self._next_id
        self._ws.send_text(
            json.dumps({"id": mid, "method": method, "params": params or {}})
        )
        deadline = time.monotonic() + timeout
        while True:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise CdpError(f"{method} の応答がタイムアウト")
            self._ws.set_timeout(remaining)
            try:
                msg = self._ws.recv_message()
            except socket.timeout:
                raise CdpError(f"{method} の応答がタイムアウト") from None
            data = json.loads(msg)
            if data.get("id") == mid:
                if "error" in data:
                    err = data["error"]
                    raise CdpError(
                        f"{method} が CDP エラー: {err.get('code')} {err.get('message')}"
                    )
                return data.get("result", {})
            self.events.append(data)

    def evaluate(self, expression: str, timeout: float = 30.0) -> Any:
        """`Runtime.evaluate`（returnByValue + awaitPromise）の結果値を返す。"""
        result = self.command(
            "Runtime.evaluate",
            {
                "expression": expression,
                "returnByValue": True,
                "awaitPromise": True,
            },
            timeout=timeout,
        )
        if "exceptionDetails" in result:
            detail = result["exceptionDetails"]
            text = detail.get("text", "")
            exc = detail.get("exception") or {}
            desc = exc.get("description") or exc.get("value") or ""
            raise CdpError(f"評価式が例外を送出: {text} {desc}".strip())
        return result.get("result", {}).get("value")

    def navigate(self, url: str, timeout: float = 30.0) -> dict:
        """`Page.navigate` でページ遷移する。"""
        return self.command("Page.navigate", {"url": url}, timeout=timeout)

    def wait_eval(
        self, expression: str, timeout: float = 15.0, interval: float = 0.2
    ) -> Any:
        """評価式が truthy な値を返すまで interval 秒ごとに再評価する。

        タイムアウト時は最後の評価結果（falsy）を返す。評価式自体の
        例外はそのまま伝播する。
        """
        deadline = time.monotonic() + timeout
        while True:
            value = self.evaluate(expression)
            if value:
                return value
            if time.monotonic() >= deadline:
                return value
            time.sleep(interval)

    def emit_open_url(self, url: str, action: str = "open") -> Any:
        """`app://open_url` イベントを注入して deep link 経路を動かす。

        `url` は動画・プレイリストの URL（`yt-browser://<action>?url=` の
        スキーム URL をここで組み立てる）。`seq` には Date.now() を使う。
        """
        scheme = f"yt-browser://{action}?url={urllib.parse.quote(url, safe='')}"
        expression = (
            "window.__TAURI_INTERNALS__.invoke("
            '"plugin:event|emit", '
            + json.dumps(
                {
                    "event": "app://open_url",
                    "payload": {"seq": int(time.time() * 1000), "url": scheme},
                }
            )
            + ').then(() => "emitted", (e) => "error: " + String(e))'
        )
        return self.evaluate(expression)

    def drain_events(self, method: str | None = None) -> list[dict]:
        """溜まったイベントを取り出す。method 指定時はそのイベントだけ残りを戻す。"""
        out: list[dict] = []
        keep: deque[dict] = deque()
        while self.events:
            ev = self.events.popleft()
            if method is None or ev.get("method") == method:
                out.append(ev)
            else:
                keep.append(ev)
        self.events = keep
        return out

    def close(self) -> None:
        self._ws.close()


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------


def main() -> int:
    parser = argparse.ArgumentParser(
        description="起動中の dev インスタンスへ CDP で接続する（FR-84）"
    )
    parser.add_argument("--port", type=int, default=DEFAULT_PORT)
    parser.add_argument(
        "--url-match",
        default=None,
        help="複数 page があるとき URL に含まれる文字列で対象を選ぶ"
        "（dev 起動なら localhost:1420）",
    )
    parser.add_argument(
        "--timeout", type=float, default=30.0, help="コマンド応答の秒数"
    )
    ops = parser.add_mutually_exclusive_group(required=True)
    ops.add_argument(
        "--list-targets", action="store_true", help="ターゲット一覧を表示"
    )
    ops.add_argument("--eval", metavar="EXPR", help="式を評価して結果を表示")
    ops.add_argument("--navigate", metavar="URL", help="Page.navigate で遷移")
    ops.add_argument(
        "--emit-open-url",
        metavar="URL",
        help="app://open_url イベントを注入（deep link 相当）",
    )
    ops.add_argument(
        "--emit-queue-url",
        metavar="URL",
        help="app://open_url を queue アクションで注入（一時キューへ追加）",
    )
    args = parser.parse_args()

    try:
        if args.list_targets:
            for t in list_targets(args.port):
                print(
                    f"{t.get('type'):>10}  {t.get('title', '')}  {t.get('url', '')}"
                )
            return 0

        session = CdpSession.attach(
            args.port, url_contains=args.url_match, timeout=args.timeout
        )
        try:
            if args.eval is not None:
                print(json.dumps(session.evaluate(args.eval, args.timeout), ensure_ascii=False))
            elif args.navigate is not None:
                session.navigate(args.navigate, args.timeout)
                print(f"navigated: {args.navigate}")
            elif args.emit_open_url is not None:
                print(session.emit_open_url(args.emit_open_url, action="open"))
            elif args.emit_queue_url is not None:
                print(session.emit_open_url(args.emit_queue_url, action="queue"))
        finally:
            session.close()
        return 0
    except CdpError as e:
        print(f"error: {e}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
