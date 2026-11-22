#!/usr/bin/env python3
"""docs とコードの整合を機械チェックする。

監査項目「docs↔コード整合の機械チェック」の検出器。文書が仕様の正である
前提で、文書とコードの差分を報告する（ずれを直すかどうかの判定は人が行う）。

使い方:
    python3 scripts/check_docs_consistency.py

終了コードは ERROR が 1 件でもあれば 1、それ以外は 0。
依存は Python 標準ライブラリのみで、ローカルと CI（.github/workflows/ci.yml の
consistency ジョブ）の双方から実行できる。

実装しているチェック（計画書の番号）:
    1. design.md §3.1 のコマンド表 ↔ lib.rs の generate_handler! ↔ src/ の invoke()
    2. design.md §3.2 のイベント表 ↔ Rust emit* / フロントエンド listen* の
       イベント名引数
    4. i18n の ja リソースキー ↔ フロントエンドの t() 参照
    5. テスト ID（DB-XX-NN）の docs 引用・コード側重複・#[test] 直下
    6. design.md の設定キー表 ↔ SETTING_* 定数リテラル
    7. ci.yml の集約ジョブ ci-status の needs・if: always()・changes 契約
    8. 要件 ID（FR-N / BG-N / INV-N）の参照 ↔ requirements-definition.md の見出し
    9. 決定記録 ID（仕様決定 / 技術方針）の参照 ↔ decision-records.md の表
    10. 設計書 §N / design.md §N の参照 ↔ design.md の見出し番号
    11. .test.ts のテスト ID（LP-NN 系）の宣言・索引・重複
    12. capabilities の windows glob ↔ コード側の窓ラベル
    13. 拡張・識別子定数（ホスト名・拡張 ID 導出一致・スキーム・バージョン）
    14. docs 間リンク `[..](path)` の参照先の存在
    15. design.md §11 リポジトリ構成ツリーの掲載パスの存在（WARN 限定）
    16. decision-records.md の Markdown 表構文
    17. 実装計画・実装履歴・README のフェーズ整合
    18. テキスト健全性（U+FFFD・競合マーカー）と JSON/XML 構文
    19. Rust モジュールの循環依存と下位共有層からの逆参照
チェック 3（§8 DDL と適用後スキーマの照合）は実マイグレーション経路を
通す必要があるため Rust 側テストとして src-tauri/src/db/tests/ddl.rs にある。

見出しへの anchor は節番号ではなく見出し語で行い、節の採番が変わっても
追従できるようにする。表の第 1 セルからバッククォート内の名前を取る規約は
§3.1 / §3.2 / 設定キー表で共通とする。
"""

from __future__ import annotations

import base64
import fnmatch
import hashlib
import json
import re
import sys
import xml.etree.ElementTree as ET
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DESIGN_MD = ROOT / "docs" / "design.md"
REQUIREMENTS_MD = ROOT / "docs" / "requirements-definition.md"
DECISIONS_MD = ROOT / "docs" / "decision-records.md"
USER_GUIDE_MD = ROOT / "docs" / "user-guide.md"
LIB_RS = ROOT / "src-tauri" / "src" / "lib.rs"
I18N_TS = ROOT / "src" / "lib" / "i18n.ts"
I18N_JA_TS = ROOT / "src" / "lib" / "i18n-ja.ts"
I18N_EN_TS = ROOT / "src" / "lib" / "i18n-en.ts"
I18N_FILES = {I18N_TS, I18N_JA_TS, I18N_EN_TS}
SRC_TAURI = ROOT / "src-tauri" / "src"
SRC_TAURI_TESTS = ROOT / "src-tauri" / "tests"
SRC_FRONTEND = ROOT / "src"
EXTENSION_DIR = ROOT / "extension"
TAURI_CONF = ROOT / "src-tauri" / "tauri.conf.json"
CAPABILITIES_DIR = ROOT / "src-tauri" / "capabilities"
WHEEL_LUA = ROOT / "src-tauri" / "mpv" / "wheel.lua"
WORKFLOWS_DIR = ROOT / ".github" / "workflows"
DOCS_DIR = ROOT / "docs"
SCRIPTS_DIR = ROOT / "scripts"

OK = "OK"
WARN = "WARN"
ERROR = "ERROR"

RUST_UPPER_MODULES = frozenset({"lib", "main", "commands", "error"})
RUST_SHARED_MODULES = frozenset({"db", "model", "util"})


@dataclass
class Findings:
    items: list[tuple[str, str]]  # (level, message)

    def ok(self, message: str) -> None:
        self.items.append((OK, message))

    def warn(self, message: str) -> None:
        self.items.append((WARN, message))

    def error(self, message: str) -> None:
        self.items.append((ERROR, message))

    def worst(self) -> str:
        if any(level == ERROR for level, _ in self.items):
            return ERROR
        if any(level == WARN for level, _ in self.items):
            return WARN
        return OK


def read_text(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def iter_files(root: Path, suffixes: tuple[str, ...]) -> list[Path]:
    return sorted(
        p for p in root.rglob("*") if p.is_file() and p.suffix in suffixes
    )


def extract_section(text: str, heading_re: str) -> str:
    """見出し語が heading_re に一致する節を返す（同レベル以上の次の見出しまで）。

    一致する節が無ければ空文字列を返す。
    """
    lines = text.splitlines()
    start = None
    level = 0
    for i, line in enumerate(lines):
        m = re.match(r"^(#{1,6})\s+(.*)$", line)
        if not m:
            continue
        if start is None:
            if re.search(heading_re, m.group(2)):
                start = i
                level = len(m.group(1))
        elif len(m.group(1)) <= level:
            return "\n".join(lines[start:i])
    if start is None:
        return ""
    return "\n".join(lines[start:])


def table_rows(section_text: str) -> list[list[str]]:
    """`| ... |` 形式の行をセル列に分解する（区切り行は除外）。

    セル内に `|` を含める記法はない前提（エスケープの `\\|` も使わない）。
    """
    rows = []
    for line in section_text.splitlines():
        s = line.strip()
        if not s.startswith("|"):
            continue
        cells = [c.strip() for c in s.strip("|").split("|")]
        if cells and all(re.fullmatch(r":?-{2,}:?", c) for c in cells):
            continue
        rows.append(cells)
    return rows


def names_in_cell(cell: str) -> list[str]:
    """第 1 セルの `name` トークンを取り、`/` 区切りの複数記載を展開する。

    `playlist_*` のような末尾 `*` はそのまま返し、呼び出し側で glob
    として解釈する。
    """
    names = []
    for tok in re.findall(r"`([^`]+)`", cell):
        for part in tok.split("/"):
            part = part.strip()
            if part:
                names.append(part)
    return names


def strip_comments(text: str, suffix: str) -> str:
    """言語ごとのコメントを取り除く。

    `//` と `/* */`（Svelte は `<!-- -->` も）を対象とし、文字列
    リテラル内の `//` は誤って削らないよう引用符を追跡する。
    Rust（.rs）では `"..."` に加えて `r#*"..."#*` / `br#*"..."#*`
    の生文字列を扱い、`'` は char リテラル（`'x'` / `'\\x'`）のときだけ
    引用符として追跡し、ライフタイム（`'a`、`'_`、`'static`）は
    引用符開始と見なさない。
    """

    def is_raw_string_start(i: int) -> int | None:
        # `r"`、`r#"..."#`、`r##"..."##`、`br#*"..."#*` の開始なら
        # 開き引用符の直後の位置を返す。違えば None。
        m = re.match(r"b?r(#*)\"", text[i:])
        if m is None:
            return None
        return i + m.end() - 1  # `"` の位置

    out: list[str] = []
    i, n = 0, len(text)
    while i < n:
        c = text[i]
        if c == "/" and i + 1 < n and text[i + 1] == "/":
            while i < n and text[i] != "\n":
                i += 1
            continue
        if c == "/" and i + 1 < n and text[i + 1] == "*":
            end = text.find("*/", i + 2)
            i = n if end < 0 else end + 2
            continue
        if suffix == ".rs":
            if c in "br":
                start = is_raw_string_start(i)
                if start is not None:
                    hashes = text[i:start].count("#")
                    closer = '"' + "#" * hashes
                    end = text.find(closer, start + 1)
                    if end < 0:
                        out.append(text[i:])
                        i = n
                        continue
                    out.append(text[i : end + len(closer)])
                    i = end + len(closer)
                    continue
            if c == "'":
                # `'x'` / `'\x'` の形だけ char リテラル（それ以外はライフタイム）
                if i + 2 < n and text[i + 2] == "'" and text[i + 1] != "\\":
                    out.append(text[i : i + 3])
                    i += 3
                    continue
                if i + 3 < n and text[i + 1] == "\\" and text[i + 3] == "'":
                    out.append(text[i : i + 4])
                    i += 4
                    continue
                out.append(c)
                i += 1
                continue
        if c in '"\'`':
            out.append(c)
            i += 1
            while i < n:
                if text[i] == "\\" and i + 1 < n:
                    out.append(text[i : i + 2])
                    i += 2
                    continue
                out.append(text[i])
                if text[i] == c:
                    i += 1
                    break
                i += 1
            continue
        out.append(c)
        i += 1
    text = "".join(out)
    if suffix == ".svelte":
        text = re.sub(r"<!--.*?-->", "", text, flags=re.S)
    return text


# ---------------------------------------------------------------------------
# チェック 1: §3.1 コマンド表 ↔ generate_handler! ↔ src/ の invoke()
# ---------------------------------------------------------------------------


def find_call_open_paren(text: str, ident_end: int, rust: bool = False) -> int | None:
    """関数名直後（任意の空白とジェネリクス `<...>` または `:: <...>` 後）の `(` 位置を返す。

    TypeScript のアロー関数型 `=>` の `>` はジェネリクス閉じ括弧と誤認しない。
    ジェネリクス内の `{ a: string; b: number }` 等に含まれる `;` は波括弧深さで
    許容しつつ、比較演算子 `x < y;` は打ち切る。文字列リテラル内の文字も読み飛ばす。
    """
    i, n = ident_end, len(text)
    while i < n and text[i].isspace():
        i += 1
    if i + 1 < n and text[i : i + 2] == "::":
        i += 2
        while i < n and text[i].isspace():
            i += 1
    if i < n and text[i] == "<":
        depth = 1
        brace = paren = bracket = 0
        i += 1
        while i < n and depth > 0:
            c = text[i]
            if c == "'" and rust:
                if i + 2 < n and text[i + 2] == "'" and text[i + 1] != "\\":
                    i += 3
                    continue
                if i + 3 < n and text[i + 1] == "\\" and text[i + 3] == "'":
                    i += 4
                    continue
                i += 1
                continue
            if c in "\"'`":
                i += 1
                while i < n:
                    if text[i] == "\\" and i + 1 < n:
                        i += 2
                        continue
                    if text[i] == c:
                        break
                    i += 1
            elif c == "{":
                brace += 1
            elif c == "}":
                if brace == 0:
                    return None
                brace -= 1
            elif c == "(":
                paren += 1
            elif c == ")":
                if paren == 0:
                    return None
                paren -= 1
            elif c == "[":
                bracket += 1
            elif c == "]":
                if bracket == 0:
                    return None
                bracket -= 1
            elif c == "<":
                depth += 1
            elif c == ">" and (i == 0 or text[i - 1] != "="):
                depth -= 1
            elif c == ";" and brace == 0 and paren == 0 and bracket == 0 and depth == 1:
                # 比較演算子 `x < y;` をジェネリクスと誤認した場合に打ち切る
                return None
            i += 1
        if depth != 0:
            return None
        while i < n and text[i].isspace():
            i += 1
    if i < n and text[i] == "(":
        return i
    return None


def check_commands(
    design_md: Path = DESIGN_MD,
    lib_rs: Path = LIB_RS,
    src_frontend: Path = SRC_FRONTEND,
) -> Findings:
    f = Findings([])
    section_text = extract_section(read_text(design_md), r"コマンド（フロント")
    if not section_text:
        f.error("design.md に §3.1（コマンド表）の見出しが見つからない")
        return f

    doc_entries: list[tuple[str, bool]] = []  # (名前, glob か)
    for row in table_rows(section_text):
        if not row:
            continue
        for name in names_in_cell(row[0]):
            doc_entries.append((name, "*" in name))

    lib = read_text(lib_rs)
    m = re.search(r"generate_handler!\s*\[(.*?)\]", lib, re.S)
    if not m:
        f.error("lib.rs に generate_handler! ブロックが見つからない")
        return f
    code_names = set(re.findall(r"commands::(\w+)", m.group(1)))

    doc_exact = {n for n, is_glob in doc_entries if not is_glob}
    doc_globs = [n for n, is_glob in doc_entries if is_glob]
    glob_matched: set[str] = set()
    for g in doc_globs:
        glob_matched.update(fnmatch.filter(code_names, g))

    for name in sorted(code_names - doc_exact - glob_matched):
        f.error(f"コマンド `{name}` が generate_handler! にあるが §3.1 に無い")
    for name in sorted(doc_exact - code_names):
        f.error(f"コマンド `{name}` が §3.1 にあるが generate_handler! に無い")
    for g in doc_globs:
        if not fnmatch.filter(code_names, g):
            f.error(f"§3.1 の glob `{g}` に一致するコマンドがコードに無い")

    # フロントエンド本番コードの invoke(...) 呼び出し ↔ generate_handler!
    invoked: set[str] = set()
    dynamic_invokes: list[str] = []
    for path in iter_files(src_frontend, (".ts", ".svelte")):
        if path.name.endswith(".test.ts"):
            continue
        text = strip_comments(read_text(path), path.suffix)
        for mi in re.finditer(r"\binvoke\b", text):
            if mi.start() > 0 and text[mi.start() - 1] == ".":
                continue
            open_paren = find_call_open_paren(text, mi.end())
            if open_paren is None:
                continue
            args = split_call_args(text, open_paren, rust=False)
            if not args:
                continue
            line_no = text.count("\n", 0, mi.start()) + 1
            loc = f"{path.relative_to(ROOT) if path.is_relative_to(ROOT) else path}:{line_no}"
            inner = single_string_literal(args[0])
            if inner is None:
                dynamic_invokes.append(loc)
            else:
                invoked.add(inner)

    for name in sorted(invoked - code_names):
        f.error(f"フロントエンドの invoke `{name}` が generate_handler! に無い")
    for name in sorted(code_names - invoked):
        f.warn(f"コマンド `{name}` が generate_handler! にあるが src/ から invoke されていない")
    for loc in dynamic_invokes:
        f.warn(f"{loc}: invoke のコマンド名が文字列リテラルでなく照合不能")

    if f.worst() == OK:
        f.ok(f"{len(code_names)} 件のコマンド（§3.1 / generate_handler! / src invoke）が一致")
    return f


# ---------------------------------------------------------------------------
# チェック 2: §3.2 イベント表 ↔ emit / emit_to / listen のイベント名引数
# ---------------------------------------------------------------------------

EVENT_NAME_RE = r"[a-z][a-z0-9_]*://[a-z0-9_]+"
# イベント名を取る関数: emit / emit_all / emit_filter / listen /
# listen_any / once は第 1 引数、emit_to は第 2 引数（第 1 引数は
# 送信先ラベル）。trigger は SQL の CREATE TRIGGER と衝突して
# 誤検出になるため対象外。
EVENT_FN_RE = re.compile(
    r"\b(emit|emit_all|emit_filter|listen|listen_any|once|emit_to)\b"
)


def split_call_args(text: str, open_paren: int, rust: bool) -> list[str]:
    """`text[open_paren]` の `(` に対応する呼び出しの引数をトップレベルの
    カンマで分割して返す。

    引数内の文字列リテラルは対として読み飛ばし、入れ子の括弧
    （丸・角・波）は深さを数える。`rust=True` では `'` を char
    リテラル（`'x'` / `'\\x'`）のときだけ引用符として扱い、
    ライフタイムは読み飛ばさない。
    """
    args: list[str] = []
    depth = 1
    start = open_paren + 1
    i = start
    n = len(text)
    while i < n:
        c = text[i]
        if c in "\"'`":
            if c == "'" and rust:
                # char リテラルだけスキップ。'a / 'static はライフタイム
                if i + 2 < n and text[i + 2] == "'":
                    i += 3
                    continue
                if i + 3 < n and text[i + 1] == "\\" and text[i + 3] == "'":
                    i += 4
                    continue
                i += 1
                continue
            i += 1
            while i < n:
                if text[i] == "\\":
                    i += 2
                    continue
                if text[i] == c:
                    break
                i += 1
            i += 1
            continue
        if c in "([{":
            depth += 1
        elif c in ")]}":
            depth -= 1
            if depth == 0:
                args.append(text[start:i])
                return args
        elif c == "," and depth == 1:
            args.append(text[start:i])
            start = i + 1
        i += 1
    return args


def single_string_literal(arg: str) -> str | None:
    """引数式が「単一の補間なし文字列リテラル」ならその中身を返す。

    `"x" + y` のような連結、`player://${x}` のような補間、`"x".into()`
    のようなメソッド呼び出しは None を返す（照合不能として警告される）。
    Rust の `r#"..."#` 生文字列と byte string も受理する。
    イベント名にエスケープ・補間は要らないため、`\\` と `${` を含む
    中身も None とする。
    """
    a = arg.strip()
    m = re.match(r'^(?:b?r|rb)?(#*)(["\'`])', a)
    if not m:
        return None
    quote_at = m.end() - 1
    closing = m.group(2) + m.group(1)  # 引用符 + # の順で閉じる（r#"..."#）
    # 引用符で始まり引用符で終わること（後続の式・連結を許さない）
    if not a.endswith(closing) or quote_at + 1 > len(a) - len(closing):
        return None
    inner = a[quote_at + 1 : len(a) - len(closing)]
    if "${" in inner or "\\" in inner:
        return None
    return inner


def check_events(
    design_md: Path = DESIGN_MD,
    src_tauri: Path = SRC_TAURI,
    src_frontend: Path = SRC_FRONTEND,
) -> Findings:
    f = Findings([])
    section_text = extract_section(read_text(design_md), r"イベント（Rust")
    if not section_text:
        f.error("design.md に §3.2（イベント表）の見出しが見つからない")
        return f

    doc_events: set[str] = set()
    for row in table_rows(section_text):
        if not row:
            continue
        doc_events.update(re.findall(EVENT_NAME_RE, row[0]))

    emitted_events: set[str] = set()
    listened_events: set[str] = set()
    dynamic_calls: list[str] = []
    targets = iter_files(src_tauri, (".rs",)) + [
        p for p in iter_files(src_frontend, (".ts", ".svelte"))
        if not p.name.endswith(".test.ts")
    ]
    for path in targets:
        text = strip_comments(read_text(path), path.suffix)
        rust = path.suffix == ".rs"
        for m in EVENT_FN_RE.finditer(text):
            open_paren = find_call_open_paren(text, m.end(), rust=rust)
            if open_paren is None:
                continue
            fn = m.group(1)
            # イベント名の位置は emit_to だけ第 2 引数
            idx = 1 if fn == "emit_to" else 0
            args = split_call_args(text, open_paren, rust)
            if len(args) <= idx:
                continue
            event_arg = args[idx].strip()
            line_no = text.count("\n", 0, m.start()) + 1
            loc = f"{path.relative_to(ROOT) if path.is_relative_to(ROOT) else path}:{line_no}"
            inner = single_string_literal(event_arg)
            if inner is None:
                dynamic_calls.append(loc)
                continue
            if re.fullmatch(EVENT_NAME_RE, inner):
                if fn.startswith("emit"):
                    emitted_events.add(inner)
                else:
                    listened_events.add(inner)
            else:
                # リテラルなのに `name://` 形式でない = §3.2 に載せられない命名
                dynamic_calls.append(loc)

    code_events = emitted_events | listened_events
    for name in sorted(code_events - doc_events):
        f.error(f"イベント `{name}` がコードにあるが §3.2 に無い")
    for name in sorted(doc_events - code_events):
        f.error(f"イベント `{name}` が §3.2 にあるがコードに無い")
    for name in sorted(emitted_events - listened_events):
        f.error(f"イベント `{name}` が emit されているがフロントエンドで listen されていない")
    for name in sorted(listened_events - emitted_events):
        f.error(f"イベント `{name}` が listen されているがバックエンドから emit されていない")
    for loc in dynamic_calls:
        f.warn(f"{loc}: イベント名が `name://` 形式のリテラルでなく照合不能（イベント名はリテラルで書く）")

    if f.worst() == OK:
        f.ok(f"{len(code_events)} 件のイベント（§3.2 / emit / listen）が一致")
    return f


# ---------------------------------------------------------------------------
# チェック 4: i18n の ja キー ↔ t() 参照
# ---------------------------------------------------------------------------

# t(`prefix.${var}`) のテンプレート参照に対応するドメイン定数。
# 接頭辞ごとの定数配列の全要素にキーがあることを確認する。
DOMAIN_CONSTANTS: dict[str, tuple[str, str]] = {
    "sponsor.cat.": ("src/lib/settings-consts.ts", "SPONSOR_CATEGORIES"),
    "filter.target.": ("src/lib/settings-consts.ts", "FILTER_TARGETS"),
    "filter.kind.": ("src/lib/settings-consts.ts", "FILTER_KINDS"),
}

DOTTED_KEY_RE = re.compile(
    r"""["']([a-zA-Z][a-zA-Z0-9_]*(?:\.[a-zA-Z0-9_]+)+)["']"""
)
TEMPLATE_REF_RE = re.compile(r"\bt\(\s*`([a-zA-Z0-9_.]+)\$\{")


def extract_const_elements(path: Path, name: str) -> list[str] | None:
    """`const NAME = [ "a", "b", ... ]` の要素を返す。見つからなければ None。"""
    text = strip_comments(read_text(path), path.suffix)
    m = re.search(rf"const\s+{name}\s*=\s*\[(.*?)\]", text, re.S)
    if not m:
        return None
    return re.findall(r'"([^"]+)"', m.group(1))


def check_i18n(setting_keys: set[str]) -> Findings:
    f = Findings([])
    i18n_text = read_text(I18N_JA_TS)
    m = re.search(r"const\s+ja\s*=\s*\{(.*?)\}\s*as\s+const", i18n_text, re.S)
    if not m:
        f.error("i18n-ja.ts に `const ja` リソースが見つからない")
        return f
    keys = set(re.findall(r"^\s*\"([^\"]+)\"\s*:", m.group(1), re.M))
    if not keys:
        f.error("i18n-ja.ts の ja リソースからキーを抽出できない")
        return f

    # 静的参照: t("key") / t('key') に限らず、ドット区切りの文字列リテラル
    # 全般を参照として拾う（cond ? "a" : "b" 経由の間接参照を拾うため）。
    # 設定キー（SETTING_* の値）は i18n キーではないので参照から除く。
    static_refs: set[str] = set()
    template_prefixes: set[str] = set()
    # ドメイン名リテラル（"youtube.com" 等）を i18n 参照と誤認しないよう、
    # 末尾セグメントが TLD の文字列は参照から除く。i18n キーは TLD で終わらない
    KNOWN_TLDS = {"com", "net", "org", "be", "io", "dev", "jp", "app"}
    for path in iter_files(SRC_FRONTEND, (".ts", ".svelte")):
        if path in I18N_FILES:
            continue
        text = strip_comments(read_text(path), path.suffix)
        for m2 in DOTTED_KEY_RE.finditer(text):
            lit = m2.group(1)
            if lit.rsplit(".", 1)[-1] in KNOWN_TLDS:
                continue
            if lit not in setting_keys:
                static_refs.add(lit)
        for m3 in TEMPLATE_REF_RE.finditer(text):
            template_prefixes.add(m3.group(1))

    for ref in sorted(static_refs - keys):
        f.error(f"i18n キー `{ref}` が参照されているが ja リソースに無い")

    domain_elements: dict[str, list[str]] = {}
    for prefix in sorted(template_prefixes):
        if not any(k.startswith(prefix) for k in keys):
            f.error(f"テンプレート参照 `{prefix}${{...}}` に一致するキーが無い")
        if prefix in DOMAIN_CONSTANTS:
            rel, const_name = DOMAIN_CONSTANTS[prefix]
            elements = extract_const_elements(ROOT / rel, const_name)
            if elements is None:
                f.error(f"ドメイン定数 {const_name} が {rel} に見つからない")
                continue
            domain_elements[prefix] = elements
            for el in elements:
                if prefix + el not in keys:
                    f.error(f"ドメイン要素 `{el}` のキー `{prefix}{el}` が無い")
        else:
            f.warn(
                f"テンプレート参照 `{prefix}` はドメイン定数の対応表"
                "（DOMAIN_CONSTANTS）に無く、要素網羅を確認できない"
            )

    # ドメイン外の余剰キー（接頭辞は合うが定数配列に無い要素）は警告
    for prefix, elements in domain_elements.items():
        for key in sorted(keys):
            if key.startswith(prefix) and key[len(prefix):] not in elements:
                f.warn(f"キー `{key}` は `{prefix}` 配下だがドメイン定数の要素に無い")

    # 静的・テンプレートのいずれでも到達しないキーは未使用として警告
    def reachable(key: str) -> bool:
        return key in static_refs or any(
            key.startswith(p) for p in template_prefixes
        )

    for key in sorted(k for k in keys if not reachable(k)):
        f.warn(f"キー `{key}` はどこからも参照されていない")

    if f.worst() == OK:
        f.ok(f"{len(keys)} 件のキーが全て参照経路を持つ")
    return f


# ---------------------------------------------------------------------------
# チェック 5: テスト ID（DB-XX-NN）
# ---------------------------------------------------------------------------

TEST_ID_DOC_COMMENT_RE = re.compile(r"^\s*///.*?\b(DB-[A-Z]+-\d+)\b")
TEST_ATTR_RE = re.compile(r"^\s*#\[(?:tokio::)?test\]")
# docs が引用するテスト ID トークン（`DB-LD-05`・`LP-01` 双方の形を拾う）。
# FR-20 / BG-3 / INV-1 / UTF-8 などの非テスト ID は除外プレフィクスで弾く。
DOC_TEST_ID_TOKEN_RE = re.compile(r"\b([A-Z]{2,3}-[A-Z]*-?\d+)\b")
NON_TEST_ID_PREFIXES = {"FR", "BG", "INV", "UTF"}


def check_test_ids(ts_test_ids: set[str]) -> Findings:
    f = Findings([])

    rs_files = iter_files(SRC_TAURI, (".rs",))
    if SRC_TAURI_TESTS.is_dir():
        rs_files += iter_files(SRC_TAURI_TESTS, (".rs",))

    code_ids: dict[str, list[str]] = {}
    for path in rs_files:
        lines = read_text(path).splitlines()
        for i, line in enumerate(lines):
            for test_id in TEST_ID_DOC_COMMENT_RE.findall(line):
                loc = f"{path.relative_to(ROOT)}:{i + 1}"
                code_ids.setdefault(test_id, []).append(loc)
                # ID コメントの直後は `///` の続行を挟んで #[test] /
                # #[tokio::test] が続く規約
                j = i + 1
                while j < len(lines) and lines[j].lstrip().startswith("///"):
                    j += 1
                if j >= len(lines) or not TEST_ATTR_RE.match(lines[j]):
                    f.error(
                        f"{loc}: テスト ID `{test_id}` の直後に "
                        "#[test] / #[tokio::test] が無い"
                    )

    for test_id, locs in sorted(code_ids.items()):
        if len(locs) > 1:
            f.error(f"テスト ID `{test_id}` が重複: {', '.join(locs)}")

    # docs 引用は .rs 側（DB-XX-NN）と .test.ts 側（LP-NN 系）の
    # 両方の ID 集合と照合する。除外プレフィクスに無い形だけ比較する。
    for doc in iter_files(DOCS_DIR, (".md",)):
        for token in sorted(set(DOC_TEST_ID_TOKEN_RE.findall(read_text(doc)))):
            if token.split("-", 1)[0] in NON_TEST_ID_PREFIXES:
                continue
            if token not in code_ids and token not in ts_test_ids:
                f.error(
                    f"テスト ID `{token}` が {doc.name} で引用されているが"
                    "コードに無い"
                )

    # プレフィクスごとの連番の抜けは報告のみ（判定には含めない）
    by_prefix: dict[str, list[int]] = {}
    for test_id in code_ids:
        prefix, num = test_id.rsplit("-", 1)
        by_prefix.setdefault(prefix, []).append(int(num))
    gap_notes = []
    for prefix, nums in sorted(by_prefix.items()):
        expected = set(range(min(nums), max(nums) + 1))
        missing = sorted(expected - set(nums))
        if missing:
            gap_notes.append(
                f"{prefix}: " + ", ".join(f"{n:02d}" for n in missing)
            )
    if gap_notes:
        f.ok("連番の抜け（報告のみ）: " + " / ".join(gap_notes))

    if f.worst() == OK:
        f.ok(f"{len(code_ids)} 件のテスト ID が正しく配置されている")
    return f


# ---------------------------------------------------------------------------
# チェック 6: design.md の設定キー表 ↔ SETTING_* 定数リテラル
# ---------------------------------------------------------------------------

SETTING_CONST_RE = re.compile(r'const\s+(SETTING_\w+)\s*:\s*&str\s*=\s*"([^"]+)"')


def extract_setting_keys() -> dict[str, str]:
    """SETTING_* 定数のキー名 → 定数名の対応を返す。"""
    keys: dict[str, str] = {}
    for path in iter_files(SRC_TAURI, (".rs",)):
        for m in SETTING_CONST_RE.finditer(
            strip_comments(read_text(path), path.suffix)
        ):
            keys[m.group(2)] = f"{path.relative_to(ROOT)} の {m.group(1)}"
    return keys


def check_settings(setting_keys: set[str]) -> Findings:
    f = Findings([])
    section_text = extract_section(read_text(DESIGN_MD), r"設定キー")
    if not section_text:
        f.error("design.md に設定キー表の節（見出しに「設定キー」を含む）が無い")
        return f

    doc_keys: set[str] = set()
    for row in table_rows(section_text):
        if not row:
            continue
        doc_keys.update(names_in_cell(row[0]))

    for key in sorted(setting_keys - doc_keys):
        f.error(f"設定キー `{key}` がコードにあるが設定キー表に無い")
    for key in sorted(doc_keys - setting_keys):
        f.error(f"設定キー `{key}` が設定キー表にあるが SETTING_* 定数に無い")

    if f.worst() == OK:
        f.ok(f"{len(setting_keys)} 件の設定キーが一致")
    return f


# ---------------------------------------------------------------------------
# チェック 7: ci.yml の集約ジョブ ci-status の needs ↔ jobs 一覧
# ---------------------------------------------------------------------------

CI_YML = WORKFLOWS_DIR / "ci.yml"
GATE_JOB_ID = "ci-status"
GATE_JOB_NAME = "CI"


def check_ci_gate(ci_yml: Path = CI_YML) -> Findings:
    f = Findings([])
    if not ci_yml.is_file():
        rel = ci_yml.relative_to(ROOT) if ci_yml.is_relative_to(ROOT) else ci_yml
        f.error(f"{rel} が見つからない")
        return f

    # jobs: 直下の 2 スペースインデントのキーをジョブ ID として拾う。
    # ジョブ属性（runs-on 等）は 4 スペース以上のため誤拾しない。
    job_ids: list[str] = []
    gate_needs: list[str] | None = None
    gate_name: str | None = None
    gate_if: str | None = None
    changes_outputs: set[str] = set()
    referenced_change_outputs: list[tuple[str, str]] = []
    in_jobs = False
    current_job: str | None = None
    in_changes_outputs = False

    for line in read_text(ci_yml).splitlines():
        if re.match(r"^jobs:\s*(?:#.*)?$", line):
            in_jobs = True
            continue
        if not in_jobs or re.match(r"^\s*#", line) or not line.strip():
            continue
        if re.match(r"^\S", line):
            break  # トップレベルキーの復帰 = jobs 節の終わり
        m = re.match(r"^  ([A-Za-z0-9_-]+):\s*(?:#.*)?$", line)
        if m:
            current_job = m.group(1)
            job_ids.append(current_job)
            in_changes_outputs = False
            continue
        if current_job == GATE_JOB_ID:
            m = re.match(r"^    needs:\s*\[([^\]]*)\]\s*(?:#.*)?$", line)
            if m:
                gate_needs = [
                    s.strip() for s in m.group(1).split(",") if s.strip()
                ]
                continue
            m = re.match(r"^    name:\s*(\S+)\s*(?:#.*)?$", line)
            if m:
                gate_name = m.group(1)
                continue
            m = re.match(r"^    if:\s*(.+?)\s*(?:#.*)?$", line)
            if m:
                gate_if = m.group(1)
                continue
        elif current_job == "changes":
            if re.match(r"^    outputs:\s*(?:#.*)?$", line):
                in_changes_outputs = True
                continue
            if re.match(r"^    [A-Za-z0-9_-]+:", line):
                in_changes_outputs = False
            elif in_changes_outputs:
                m = re.match(r"^      ([A-Za-z0-9_-]+):\s*", line)
                if m:
                    changes_outputs.add(m.group(1))
        if current_job is not None and current_job != GATE_JOB_ID:
            for ref in re.findall(r"needs\.changes\.outputs\.([A-Za-z0-9_-]+)", line):
                referenced_change_outputs.append((current_job, ref))

    if GATE_JOB_ID not in job_ids:
        f.error(f"ci.yml に集約ジョブ `{GATE_JOB_ID}` が無い")
        return f
    if gate_name != GATE_JOB_NAME:
        f.error(
            f"`{GATE_JOB_ID}` の name が `{gate_name}` になっている"
            f"（必須チェック名 `{GATE_JOB_NAME}` と一致させる契約）"
        )
    if gate_if != "always()":
        f.error(
            f"`{GATE_JOB_ID}` に `if: always()` が無い（現在: {gate_if!r}）"
        )
    if gate_needs is None:
        f.error(f"`{GATE_JOB_ID}` の needs がインライン `[a, b]` 形式で読めない")
    else:
        expected = {j for j in job_ids if j != GATE_JOB_ID}
        for j in sorted(expected - set(gate_needs)):
            f.error(f"ジョブ `{j}` が `{GATE_JOB_ID}` の needs に無い")
        for j in sorted(set(gate_needs) - expected):
            f.error(f"`{GATE_JOB_ID}` の needs の `{j}` は存在しないジョブ")

    if "changes" in job_ids:
        for job_id, ref in referenced_change_outputs:
            if ref not in changes_outputs:
                f.error(
                    f"ジョブ `{job_id}` が参照する `needs.changes.outputs.{ref}` が "
                    "`changes.outputs` に定義されていない"
                )
        used_refs = {ref for _, ref in referenced_change_outputs}
        for out_key in sorted(changes_outputs - used_refs):
            f.warn(f"`changes.outputs.{out_key}` が他ジョブの if 条件で使われていない")

    if f.worst() == OK:
        f.ok(
            f"{len(job_ids) - 1} 件のジョブが `{GATE_JOB_ID}`（if: always()）の "
            "needs に網羅されている"
        )
    return f


# ---------------------------------------------------------------------------
# チェック 8〜10 共通: 参照の走査対象
# ---------------------------------------------------------------------------

# 要件 ID・決定記録 ID・設計書節番号の参照を拾う対象。
# コードはコメントも本文も走査する（これらの ID はコメントに書く規約）。
def doc_and_code_files() -> list[Path]:
    files = iter_files(DOCS_DIR, (".md",))
    files += [ROOT / "README.md", ROOT / "AGENTS.md"]
    files += iter_files(SRC_FRONTEND, (".ts", ".svelte"))
    files += iter_files(SRC_TAURI, (".rs",))
    files += iter_files(EXTENSION_DIR, (".js",))
    files += [
        p for p in iter_files(SCRIPTS_DIR, (".md", ".py"))
        if p.name != "check_docs_consistency.py"
    ]
    if WORKFLOWS_DIR.is_dir():
        files += iter_files(WORKFLOWS_DIR, (".yml", ".yaml"))
    files.append(WHEEL_LUA)
    return [p for p in files if p.is_file()]


# ---------------------------------------------------------------------------
# チェック 8: 要件 ID（FR-N / BG-N / INV-N）の参照 ↔ requirements-definition.md
# ---------------------------------------------------------------------------

REQ_ID_RE = re.compile(r"\b(FR|BG|INV)-(\d+)\b")
REQ_HEADING_RE = re.compile(r"^###\s+(FR|BG|INV)-(\d+)\b", re.M)


def check_requirement_ids() -> Findings:
    f = Findings([])
    req_text = read_text(REQUIREMENTS_MD)
    defined = {
        f"{m.group(1)}-{m.group(2)}" for m in REQ_HEADING_RE.finditer(req_text)
    }
    if not defined:
        f.error("requirements-definition.md から要件 ID 見出しを抽出できない")
        return f

    refs: dict[str, int] = {}
    for path in doc_and_code_files():
        for line in read_text(path).splitlines():
            # 定義見出し行そのものは参照として数えない
            # （他に参照が無い ID を未参照として検出するため）
            if path == REQUIREMENTS_MD and REQ_HEADING_RE.match(line):
                continue
            for m in REQ_ID_RE.finditer(line):
                rid = f"{m.group(1)}-{m.group(2)}"
                refs[rid] = refs.get(rid, 0) + 1

    for rid in sorted(refs, key=lambda r: (r.split("-")[0], int(r.split("-")[1]))):
        if rid not in defined:
            f.error(f"要件 ID `{rid}` が参照されているが requirements-definition.md に節が無い")
    for rid in sorted(
        defined - set(refs),
        key=lambda r: (r.split("-")[0], int(r.split("-")[1])),
    ):
        f.warn(f"要件 ID `{rid}` は定義済みだがどこからも参照されていない")

    if f.worst() == OK:
        f.ok(f"{len(refs)} 件の要件 ID 参照が定義と一致")
    return f


# ---------------------------------------------------------------------------
# チェック 9: 決定記録 ID（仕様決定 / 技術方針）の参照 ↔ decision-records.md
# ---------------------------------------------------------------------------

# 「仕様決定 A」「仕様決定 AM・AS」「仕様決定 AT と AU」「仕様決定 AC・AH」
# のような単記・連記を 1 塊で拾い、区切りで分割して各 ID を照合する。
# トークンは `-\d` 接尾辞（FR-19・INV-1 など決定 ID でない並記 ID）を
# 許容して後で除外し、後続が英数字の場合はトークンとみなさない
# （「仕様決定 AH、Phase 23」の Phase の P を ID と誤認しないため）。
DECISION_ID_TOKEN = r"[A-Z]+(?:-\d+)?(?![a-zA-Z0-9])"
DECISION_REF_RE = re.compile(
    rf"(仕様決定|技術方針)\s*({DECISION_ID_TOKEN}"
    rf"(?:(?:[・,、]|と)\s*{DECISION_ID_TOKEN})*)"
)


def decision_table_ids(heading: str) -> set[str]:
    """decision-records.md の指定見出しの表から ID 集合を取る。

    行内で `^[A-Z]+$` に一致する最初のセルを ID とする（ヘッダ行の `ID` セルは除く）。
    """
    section_text = extract_section(read_text(DECISIONS_MD), heading)
    ids: set[str] = set()
    for row in table_rows(section_text):
        for cell in row:
            if cell == "ID":
                continue
            if re.fullmatch(r"[A-Z]+", cell):
                ids.add(cell)
                break
    return ids


def check_decision_ids() -> Findings:
    f = Findings([])
    spec_ids = decision_table_ids(r"^仕様決定")
    tech_ids = decision_table_ids(r"^技術方針")
    if not spec_ids or not tech_ids:
        f.error("decision-records.md の「仕様決定」/「技術方針」表を読めない")
        return f

    refs_spec: set[str] = set()
    refs_tech: set[str] = set()
    for path in doc_and_code_files():
        text = read_text(path)
        for m in DECISION_REF_RE.finditer(text):
            ids = re.split(r"[・,、]|と", m.group(2))
            for rid in (i.strip() for i in ids if i.strip()):
                if "-" in rid:
                    # FR-19・INV-1 など、決定 ID ではない並記の要件/テスト ID
                    continue
                if m.group(1) == "仕様決定":
                    refs_spec.add(rid)
                    if rid in tech_ids:
                        f.error(
                            f"「仕様決定 {rid}」と参照されているが {rid} は"
                            f"技術方針表の ID（技術方針 {rid} の誤記か。{path.relative_to(ROOT)}）"
                        )
                    elif rid not in spec_ids:
                        f.error(
                            f"「仕様決定 {rid}」が参照されているが仕様決定表に無い"
                            f"（{path.relative_to(ROOT)}）"
                        )
                else:
                    refs_tech.add(rid)
                    if rid in spec_ids:
                        f.error(
                            f"「技術方針 {rid}」と参照されているが {rid} は"
                            f"仕様決定表の ID（仕様決定 {rid} の誤記か。{path.relative_to(ROOT)}）"
                        )
                    elif rid not in tech_ids:
                        f.error(
                            f"「技術方針 {rid}」が参照されているが技術方針表に無い"
                            f"（{path.relative_to(ROOT)}）"
                        )

    for rid in sorted(spec_ids - refs_spec):
        f.warn(f"仕様決定 {rid} は定義済みだがどこからも参照されていない")
    for rid in sorted(tech_ids - refs_tech):
        f.warn(f"技術方針 {rid} は定義済みだがどこからも参照されていない")

    if f.worst() == OK:
        f.ok(f"仕様決定 {len(refs_spec)} 件・技術方針 {len(refs_tech)} 件の参照が定義と一致")
    return f


# ---------------------------------------------------------------------------
# チェック 10: 設計書 §N / design.md §N の参照 ↔ design.md 見出し番号
# ---------------------------------------------------------------------------

# コード・docs 中の「設計書 §4.5」「design.md §3.4.1」参照。
# 範囲参照 `§6.2〜§7` / `§6.2〜6.3` は終端も照合する。
# 「要件定義 §5」等の非設計書参照は接頭辞必須で拾わない。
SECTION_REF_RE = re.compile(
    r"(?:設計書|design\.md)\s*§\s*(\d+(?:\.\d+)*)"
    r"(?:\s*〜\s*§?\s*(\d+(?:\.\d+)*))?"
)
# design.md 自内の裸 `§N` 参照（接頭辞なし）
DESIGN_SELF_SECTION_RE = re.compile(r"§\s*(\d+(?:\.\d+)*)")
HEADING_NUM_RE = re.compile(r"^#{2,4}\s+(\d+(?:\.\d+)*)", re.M)


def check_design_sections() -> Findings:
    f = Findings([])
    design_text = read_text(DESIGN_MD)
    defined = set(HEADING_NUM_RE.findall(design_text))
    if not defined:
        f.error("design.md から見出し番号を抽出できない")
        return f

    refs: list[tuple[str, str]] = []  # (番号, 出所)
    for path in doc_and_code_files():
        for m in SECTION_REF_RE.finditer(read_text(path)):
            refs.append((m.group(1), f"{path.relative_to(ROOT)}"))
            if m.group(2):
                refs.append((m.group(2), f"{path.relative_to(ROOT)}"))
    # design.md 自内の裸 § 参照（上記で拾った接頭辞付きと重複してもよい）
    for m in DESIGN_SELF_SECTION_RE.finditer(design_text):
        refs.append((m.group(1), "docs/design.md（自内参照）"))

    bad = sorted({num for num, _ in refs if num not in defined})
    for num in bad:
        locs = sorted({src for n, src in refs if n == num})
        f.error(f"§{num} への参照があるが design.md に該当見出しが無い（{', '.join(locs)}）")

    if f.worst() == OK:
        f.ok(f"{len({n for n, _ in refs})} 件の節番号参照が見出しと一致")
    return f


# ---------------------------------------------------------------------------
# チェック 11: .test.ts のテスト ID（LP-NN 系）
# ---------------------------------------------------------------------------

# describe/it/test("LP-01 ..." の先頭 ID を宣言、冒頭コメントの
# `// LP-01: 概要` を索引として拾う。FR-20 等の非テスト ID は
# テスト ID 規約の対象外として除外する。
TS_TEST_DECL_RE = re.compile(
    r"(?:describe|it|test)\s*\(\s*\"([A-Z]{2,3}-\d{2})(?=[\s\":：])"
)
TS_TEST_TITLE_RE = re.compile(
    r"(?:describe|it|test)\s*\(\s*\"([^\"]+)\""
)
TS_TEST_INDEX_RE = re.compile(r"^\s*//\s*([A-Z]{2,3}-\d{2})\s*:", re.M)
TS_TEST_ID_TOKEN_RE = re.compile(r"[A-Z]{2,3}-\d{2}")
TS_TEST_ID_EXCLUDE_PREFIXES = {"FR", "BG", "INV", "DB"}


def iter_test_ts() -> list[Path]:
    """`*.test.ts` と `extension/*.test.js` を返す（Path.suffix は `.ts` 等しか返さないため名前で判定）。"""
    ts_tests = [
        p for p in SRC_FRONTEND.rglob("*")
        if p.is_file() and p.name.endswith(".test.ts")
    ]
    ext_tests = [
        p for p in EXTENSION_DIR.glob("*.test.js")
        if p.is_file()
    ]
    return sorted(ts_tests + ext_tests)


def scan_ts_test_ids() -> tuple[dict[str, list[str]], dict[str, list[str]]]:
    """`.test.ts` / `.test.js` の宣言 ID（describe 先頭）と索引 ID（冒頭コメント）を返す。"""
    decl: dict[str, list[str]] = {}
    index: dict[str, list[str]] = {}
    for path in iter_test_ts():
        rel = str(path.relative_to(ROOT))
        text = read_text(path)
        for m in TS_TEST_DECL_RE.finditer(text):
            decl.setdefault(m.group(1), []).append(rel)
        for m in TS_TEST_INDEX_RE.finditer(text):
            index.setdefault(m.group(1), []).append(rel)
    return decl, index


def check_ts_test_ids() -> Findings:
    f = Findings([])
    decl, index = scan_ts_test_ids()

    for test_id, locs in sorted(decl.items()):
        if len(locs) > 1:
            f.error(f"テスト ID `{test_id}` が重複: {', '.join(locs)}")
    for test_id, locs in sorted(index.items()):
        if len(locs) > 1:
            f.error(f"テスト索引 ID `{test_id}` が重複: {', '.join(locs)}")

    for path in iter_test_ts():
        rel = str(path.relative_to(ROOT))
        text = read_text(path)
        if not TS_TEST_DECL_RE.search(text):
            f.error(f"{rel}: テスト ID 宣言（describe/it/test）が 1 件も無い")
        for m in TS_TEST_TITLE_RE.finditer(text):
            title = m.group(1)
            tokens = [
                t for t in TS_TEST_ID_TOKEN_RE.findall(title)
                if t.split("-", 1)[0] not in TS_TEST_ID_EXCLUDE_PREFIXES
            ]
            for tok in tokens:
                if not title.startswith(tok):
                    f.error(
                        f"{rel}: タイトル `{title}` の ID `{tok}` が先頭に無い"
                        "（テスト ID はタイトル先頭に置く規約）"
                    )

    for test_id in sorted(set(decl) | set(index)):
        decl_locs = decl.get(test_id, [])
        index_locs = index.get(test_id, [])
        if not decl_locs:
            f.warn(f"索引コメントの `{test_id}` に対応する describe/it が無い（{', '.join(index_locs)}）")
        elif not index_locs:
            f.warn(f"`{test_id}` の describe 宣言に対応する索引コメントが無い（{', '.join(decl_locs)}）")
        elif set(decl_locs) != set(index_locs):
            f.error(
                f"`{test_id}` の宣言ファイル（{', '.join(decl_locs)}）と"
                f"索引ファイル（{', '.join(index_locs)}）が一致しない"
            )

    # 連番の抜けは報告のみ（check 5 と同じ扱い）
    by_prefix: dict[str, list[int]] = {}
    for test_id in decl:
        prefix, num = test_id.rsplit("-", 1)
        by_prefix.setdefault(prefix, []).append(int(num))
    gap_notes = []
    for prefix, nums in sorted(by_prefix.items()):
        expected = set(range(min(nums), max(nums) + 1))
        missing = sorted(expected - set(nums))
        if missing:
            gap_notes.append(
                f"{prefix}: " + ", ".join(f"{n:02d}" for n in missing)
            )
    if gap_notes:
        f.ok("連番の抜け（報告のみ）: " + " / ".join(gap_notes))

    if f.worst() == OK:
        f.ok(f"{len(decl)} 件の .test.ts / .test.js テスト ID が規約通り")
    return f


# ---------------------------------------------------------------------------
# チェック 12: capabilities の windows glob ↔ 窓ラベル
# ---------------------------------------------------------------------------

# 関数経由で窓ラベルを生成する生産関数 → 期待するラベル glob の宣言表。
# 関数本体の format!("...{...}...") から `{...}` を `*` に置換した
# glob と照合する。窓生成を増やしたらここへ登録する（未登録の生成は
# 静的に追えないため検出対象外になる）。
WINDOW_LABEL_PRODUCERS: dict[str, str] = {
    "popup_label": "chat-popup-*",
}


def extract_label_glob(fn_name: str) -> str | None:
    """`fn <name>` 本体内の最初の format!("...") から glob を導出する。"""
    for path in iter_files(SRC_TAURI, (".rs",)):
        text = read_text(path)
        m = re.search(rf"fn\s+{re.escape(fn_name)}\b.*?\{{(.*?)\n\}}", text, re.S)
        if not m:
            continue
        f = re.search(r'format!\s*\(\s*"([^"]+)"', m.group(1))
        if not f:
            continue
        return re.sub(r"\{[^}]*\}", "*", f.group(1))
    return None


def check_window_labels() -> Findings:
    f = Findings([])

    cap_globs: set[str] = set()
    for path in sorted(CAPABILITIES_DIR.glob("*.json")):
        data = json.loads(read_text(path))
        cap_globs.update(data.get("windows", []))
    if not cap_globs:
        f.error("capabilities/*.json から windows 指定を抽出できない")
        return f

    code_literals: set[str] = set()
    for path in iter_files(SRC_TAURI, (".rs",)):
        text = strip_comments(read_text(path), ".rs")
        code_literals.update(
            re.findall(r'get_webview_window\s*\(\s*"([^"]+)"', text)
        )
        for m in re.finditer(r"\bemit_to\s*\(", text):
            args = split_call_args(text, m.end() - 1, rust=True)
            if args:
                lit = single_string_literal(args[0])
                if lit:
                    code_literals.add(lit)
        code_literals.update(
            re.findall(
                r"WebviewWindowBuilder::new\s*\(\s*[^,]+,\s*\"([^\"]+)\"",
                text,
            )
        )

    # tauri.conf.json の windows[].label（未指定は Tauri 既定の "main"）
    conf = json.loads(read_text(TAURI_CONF))
    conf_labels = {
        w.get("label", "main") for w in conf.get("app", {}).get("windows", [])
    }

    producer_globs: dict[str, str] = {}
    for fn_name, expected in WINDOW_LABEL_PRODUCERS.items():
        actual = extract_label_glob(fn_name)
        if actual is None:
            f.error(f"窓ラベル生産関数 `{fn_name}` から format! リテラルを読めない")
            continue
        if actual != expected:
            f.error(
                f"`{fn_name}` の生成ラベル `{actual}` が宣言 glob `{expected}` と不一致"
            )
        producer_globs[fn_name] = actual

    produced = code_literals | conf_labels
    for lit in sorted(produced):
        if not any(fnmatch.fnmatch(lit, g) for g in cap_globs):
            f.error(f"窓ラベル `{lit}` に一致する capability の windows glob が無い")
    for g in sorted(producer_globs.values()):
        if g not in cap_globs:
            f.error(f"生成ラベル glob `{g}` が capability の windows に無い")

    for g in sorted(cap_globs):
        hit = g in producer_globs.values() or any(
            fnmatch.fnmatch(lit, g) for lit in produced
        )
        if not hit:
            f.warn(f"capability の windows glob `{g}` に一致する窓生成が見つからない")

    if f.worst() == OK:
        f.ok(f"{len(cap_globs)} 件の capability と窓ラベルが一致")
    return f


# ---------------------------------------------------------------------------
# チェック 13: 拡張・識別子定数の一致
# ---------------------------------------------------------------------------


def derive_extension_id(pubkey_b64: str) -> str:
    """Chrome 拡張 manifest.json の Base64 公開鍵 `key` から 32 文字の拡張 ID を導出する。"""
    digest = hashlib.sha256(base64.b64decode(pubkey_b64)).digest()[:16]
    return "".join(chr(ord("a") + (b >> 4)) + chr(ord("a") + (b & 0xF)) for b in digest)


def check_extension_constants(
    manifest_path: Path | None = None,
    native_host_path: Path | None = None,
) -> Findings:
    f = Findings([])
    conf = json.loads(read_text(TAURI_CONF))
    identifier = conf.get("identifier")
    if not identifier:
        f.error("tauri.conf.json の identifier を読めない")
        return f
    host_name_expected = identifier.replace("-", "_")

    nh_file = native_host_path or (ROOT / "src-tauri" / "src" / "native_host.rs")
    mf_file = manifest_path or (EXTENSION_DIR / "manifest.json")
    native_host = read_text(nh_file)
    background = read_text(EXTENSION_DIR / "background.js")
    deep_link = read_text(ROOT / "src-tauri" / "src" / "deep_link.rs")

    m = re.search(r'HOST_NAME\s*:\s*&str\s*=\s*"([^"]+)"', native_host)
    if not m:
        f.error("native_host.rs の HOST_NAME を読めない")
    elif m.group(1) != host_name_expected:
        f.error(
            f"native_host.rs の HOST_NAME `{m.group(1)}` が identifier の"
            f"`_` 置換形 `{host_name_expected}` と不一致"
        )
    m = re.search(r'HOST_NAME\s*=\s*"([^"]+)"', background)
    if not m:
        f.error("background.js の HOST_NAME を読めない")
    elif m.group(1) != host_name_expected:
        f.error(
            f"background.js の HOST_NAME `{m.group(1)}` が"
            f"`{host_name_expected}` と不一致"
        )
    section_341 = extract_section(read_text(DESIGN_MD), r"^3\.4\.1\b")
    if section_341 and host_name_expected not in section_341:
        f.error(f"design.md §3.4.1 にホスト名 `{host_name_expected}` の記載が無い")

    m = re.search(r'EXTENSION_ID\s*:\s*&str\s*=\s*"([^"]+)"', native_host)
    if not m:
        f.error("native_host.rs の EXTENSION_ID を読めない")
    else:
        ext_id = m.group(1)
        if ext_id not in read_text(DECISIONS_MD):
            f.error(
                f"EXTENSION_ID `{ext_id}` が decision-records.md に記載されていない"
            )
        try:
            manifest = json.loads(read_text(mf_file))
        except Exception as e:
            f.error(f"extension/manifest.json を読めない: {e}")
            manifest = None
        if isinstance(manifest, dict):
            pubkey = manifest.get("key")
            if not isinstance(pubkey, str) or not pubkey.strip():
                f.error("extension/manifest.json に公開鍵 `key` が無い")
            else:
                try:
                    derived = derive_extension_id(pubkey)
                except Exception as e:
                    f.error(f"extension/manifest.json の key から拡張 ID を導出できない: {e}")
                else:
                    if derived != ext_id:
                        f.error(
                            f"extension/manifest.json の公開鍵から導出した ID `{derived}` と "
                            f"native_host.rs の EXTENSION_ID `{ext_id}` が不一致"
                        )
            # manifest.json が参照する拡張ファイルの実在確認
            ext_dir = mf_file.parent
            bg_sw = (manifest.get("background") or {}).get("service_worker")
            if bg_sw and not (ext_dir / bg_sw).is_file():
                f.error(f"manifest.json の service_worker `{bg_sw}` が存在しない")
            popup_file = (manifest.get("action") or {}).get("default_popup")
            if popup_file and not (ext_dir / popup_file).is_file():
                f.error(f"manifest.json の default_popup `{popup_file}` が存在しない")
            for cs in manifest.get("content_scripts") or []:
                for js_rel in cs.get("js") or []:
                    if not (ext_dir / js_rel).is_file():
                        f.error(f"manifest.json の content_scripts `{js_rel}` が存在しない")

    schemes = (
        conf.get("plugins", {})
        .get("deep-link", {})
        .get("desktop", {})
        .get("schemes", [])
    )
    if not schemes:
        f.error("tauri.conf.json の deep-link schemes を読めない")
    for scheme in schemes:
        if f"{scheme}://" not in background:
            f.error(f"スキーム `{scheme}://` が background.js に無い")
        if f"{scheme}:" not in deep_link:
            f.error(f"スキーム `{scheme}` が deep_link.rs に無い")

    # バージョン 4 箇所の一致は bump_version.py の current_versions() を借用
    sys.path.insert(0, str(ROOT / "scripts"))
    try:
        from bump_version import current_versions

        versions = current_versions()
        distinct = {v for v in versions.values()}
        if len(distinct) != 1 or None in distinct:
            f.error(f"バージョンが 4 箇所で不一致: {versions}")
    except Exception as e:  # import 失敗は WARN に留める
        f.warn(f"bump_version.current_versions の読み込みに失敗: {e}")
    finally:
        sys.path.remove(str(ROOT / "scripts"))

    # 補助: user-guide.md の identifier リテラル
    for lit in set(
        re.findall(r"io\.github\.[a-zA-Z0-9_.-]+", read_text(USER_GUIDE_MD))
    ):
        if lit != identifier:
            f.warn(
                f"user-guide.md の `{lit}` が identifier `{identifier}` と不一致"
            )

    if f.worst() == OK:
        f.ok("拡張・識別子定数が全箇所で一致")
    return f


# ---------------------------------------------------------------------------
# チェック 14: docs 間リンク [..](path) の存在
# ---------------------------------------------------------------------------

DOC_LINK_RE = re.compile(r"\[[^\]]*\]\(([^)\s]+)")


def check_doc_links() -> Findings:
    f = Findings([])
    targets = iter_files(DOCS_DIR, (".md",)) + [
        ROOT / "README.md",
        ROOT / "AGENTS.md",
        SCRIPTS_DIR / "README.md",
    ]
    checked = 0
    for path in targets:
        for m in DOC_LINK_RE.finditer(read_text(path)):
            target = m.group(1)
            if target.startswith(("http://", "https://", "#", "mailto:")):
                continue
            # `path "title"` のタイトル部と `#anchor` フラグメントは外す
            target = target.split()[0].split("#", 1)[0]
            if not target:
                continue
            checked += 1
            resolved = (path.parent / target).resolve()
            if not resolved.exists():
                f.error(
                    f"{path.relative_to(ROOT)} のリンク `{m.group(1)}` の"
                    "参照先が存在しない"
                )
    if f.worst() == OK:
        f.ok(f"{checked} 件の docs 間リンクが全て有効")
    return f


# ---------------------------------------------------------------------------
# チェック 15: design.md §11 リポジトリ構成ツリーの掲載パス存在（WARN 限定）
# ---------------------------------------------------------------------------


def check_repo_tree() -> Findings:
    """§11 のツリーは説明図で完全網羅を意図していないため、列挙されたパスの
    存在だけを WARN で報告し、逆方向（実在するが未掲載）は検出しない。
    """
    f = Findings([])
    section_text = extract_section(read_text(DESIGN_MD), r"リポジトリ構成")
    if not section_text:
        f.error("design.md にリポジトリ構成の節が無い")
        return f
    m = re.search(r"```(?:text)?\n(.*?)```", section_text, re.S)
    if not m:
        f.error("リポジトリ構成のツリー（``` ブロック）が見つからない")
        return f

    checked = 0
    stack: list[tuple[int, str]] = []  # (インデント幅, ディレクトリ名)
    for line in m.group(1).splitlines():
        # `#` 以降はコメント、空行は飛ばす
        body = re.sub(r"\s+#.*$", "", line).rstrip()
        stripped = body.strip()
        if not stripped:
            continue
        indent = len(line) - len(line.lstrip())
        is_dir = stripped.endswith("/")
        # `A / B / C` は同階層の並記。パス内の / はスペースを伴わない
        tokens = [
            t.strip().rstrip("/") for t in stripped.split(" / ") if t.strip()
        ]
        for token in tokens:
            if token == "yt-browser":  # ルート行はリポジトリ根自体
                continue
            while stack and stack[-1][0] >= indent:
                stack.pop()
            parts = [name for _, name in stack] + [token]
            rel = "/".join(parts)
            checked += 1
            if not (ROOT / rel).exists():
                f.warn(f"ツリー掲載パス `{rel}` がリポジトリに存在しない")
        if is_dir and tokens and tokens[-1] != "yt-browser":
            stack.append((indent, tokens[-1]))
    if f.worst() == OK:
        f.ok(f"§11 ツリーの {checked} 件の掲載パスが全て存在")
    return f


# ---------------------------------------------------------------------------
# チェック 16: decision-records.md の Markdown 表構文
# ---------------------------------------------------------------------------


def check_decision_tables() -> Findings:
    f = Findings([])
    text = read_text(DECISIONS_MD)
    checked_rows = 0
    for heading in (r"^仕様決定", r"^技術方針"):
        section_text = extract_section(text, heading)
        if not section_text:
            f.error(f"decision-records.md に見出し `{heading}` が無い")
            continue
        in_table = False
        seen_blank = False
        for i, line in enumerate(section_text.splitlines(), 1):
            s = line.strip()
            if s.startswith("|"):
                if seen_blank:
                    f.error(
                        f"decision-records.md（{heading} 節内 +{i} 行目）: "
                        "表の途中に空行があり表が分断されている"
                    )
                    seen_blank = False
                in_table = True
                if s.startswith("||"):
                    f.error(
                        f"decision-records.md（{heading} 節内 +{i} 行目）: "
                        "行頭に余分な `|`（`||`）がある"
                    )
                    continue
                cells = [c.strip() for c in s.strip("|").split("|")]
                if len(cells) != 3:
                    f.error(
                        f"decision-records.md（{heading} 節内 +{i} 行目）: "
                        f"表の列数が 3 ではない（{len(cells)} 列）"
                    )
                    continue
                if not all(re.fullmatch(r":?-{2,}:?", c) for c in cells):
                    checked_rows += 1
            elif in_table and not s:
                seen_blank = True
    if f.worst() == OK:
        f.ok(f"仕様決定・技術方針表の全 {checked_rows} 行が 3 列・空行分断なし")
    return f


# ---------------------------------------------------------------------------
# チェック 17: 実装計画・実装履歴・README のフェーズ整合
# ---------------------------------------------------------------------------

README_MD = ROOT / "README.md"
IMPL_PLAN_MD = DOCS_DIR / "implementation-plan.md"
IMPL_HISTORY_MD = DOCS_DIR / "implementation-history.md"
PHASE_HEADING_RE = re.compile(r"^###\s+Phase\s+(\d+)：", re.M)
README_TOTAL_PHASE_RE = re.compile(r"全フェーズ（Phase\s+0〜(\d+)）")
README_HISTORY_PHASE_RE = re.compile(r"Phase\s+0〜(\d+)\s+の完了済み実装履歴")
PLAN_HISTORY_PHASE_RE = re.compile(r"Phase\s+0〜(\d+)\s+の完了済み記録")


def check_implementation_phases(
    impl_plan_md: Path = IMPL_PLAN_MD,
    impl_history_md: Path = IMPL_HISTORY_MD,
    readme_md: Path = README_MD,
) -> Findings:
    f = Findings([])
    missing_files = [
        p for p in (impl_history_md, impl_plan_md, readme_md) if not p.is_file()
    ]
    if missing_files:
        for p in missing_files:
            rel = p.relative_to(ROOT) if p.is_relative_to(ROOT) else p
            f.error(f"{rel} が見つからない")
        return f

    history_text = read_text(impl_history_md)
    plan_text = read_text(impl_plan_md)
    readme_text = read_text(readme_md)

    history_nums = [int(n) for n in PHASE_HEADING_RE.findall(history_text)]
    plan_nums = [int(n) for n in PHASE_HEADING_RE.findall(plan_text)]

    if not history_nums:
        f.error("implementation-history.md に `### Phase N：` 見出しが無い")
    if not plan_nums:
        f.error("implementation-plan.md に `### Phase N：` 見出しが無い")
    if not history_nums or not plan_nums:
        return f

    counts: dict[int, list[str]] = {}
    for n in history_nums:
        counts.setdefault(n, []).append("implementation-history.md")
    for n in plan_nums:
        counts.setdefault(n, []).append("implementation-plan.md")
    for n, locs in sorted(counts.items()):
        if len(locs) > 1:
            f.error(f"Phase {n} の見出しが重複している（{', '.join(locs)}）")

    if history_nums != sorted(history_nums):
        f.error("implementation-history.md の Phase 見出し順が昇順になっていない")
    if plan_nums != sorted(plan_nums):
        f.error("implementation-plan.md の Phase 見出し順が昇順になっていない")

    all_nums = sorted(counts)
    expected = list(range(0, all_nums[-1] + 1))
    missing = sorted(set(expected) - set(all_nums))
    if missing:
        f.error(f"Phase 見出しに連番の抜けがある: {missing}")

    last_all = all_nums[-1]
    last_hist = max(history_nums)
    first_plan = min(plan_nums)
    if last_hist >= first_plan:
        f.error(
            f"implementation-history.md の末尾 Phase {last_hist} と "
            f"implementation-plan.md の先頭 Phase {first_plan} の境界が逆転している"
        )

    range_checks = [
        (
            README_TOTAL_PHASE_RE,
            readme_text,
            last_all,
            "README.md に `全フェーズ（Phase 0〜N）` の表記が無い",
            "README.md の全フェーズ表記",
            "実見出しの末尾",
        ),
        (
            README_HISTORY_PHASE_RE,
            readme_text,
            last_hist,
            "README.md に `Phase 0〜N の完了済み実装履歴` の表記が無い",
            "README.md の実装履歴表記",
            "implementation-history.md の末尾",
        ),
        (
            PLAN_HISTORY_PHASE_RE,
            plan_text,
            last_hist,
            "implementation-plan.md に `Phase 0〜N の完了済み記録` の表記が無い",
            "implementation-plan.md の完了済み記録表記",
            "implementation-history.md の末尾",
        ),
    ]
    for pattern, text, expected_num, missing_msg, label, target_label in range_checks:
        matches = pattern.findall(text)
        if not matches:
            f.error(missing_msg)
            continue
        for num_str in matches:
            if int(num_str) != expected_num:
                f.error(
                    f"{label} Phase 0〜{num_str} が {target_label} Phase {expected_num} と不一致"
                )

    if f.worst() == OK:
        f.ok(
            f"Phase 0〜{last_all}（履歴 0〜{last_hist} / "
            f"計画 {first_plan}〜{max(plan_nums)} / README 表記）が重複・欠番なく一致"
        )
    return f


# ---------------------------------------------------------------------------
# チェック 18: テキスト健全性（U+FFFD・競合マーカー）と JSON/XML 構文
# ---------------------------------------------------------------------------

CONFLICT_MARKER_RE = re.compile(
    r"^(?:<{7}(?:\s|$)|={7}$|>{7}(?:\s|$))", re.M
)
TEXT_CHECK_SUFFIXES = {
    ".md", ".ts", ".js", ".svelte", ".rs", ".py",
    ".yml", ".yaml", ".json", ".toml", ".html", ".css", ".lua", ".xml",
}
SKIP_DIR_NAMES = {
    "node_modules", "target", ".svelte-kit", ".git", "build", "__pycache__", "gen",
}


def iter_repo_text_files(root: Path = ROOT) -> list[Path]:
    out: list[Path] = []
    for p in sorted(root.rglob("*")):
        if not p.is_file() or p.suffix not in TEXT_CHECK_SUFFIXES:
            continue
        rel_parts = p.relative_to(root).parts
        if any(part in SKIP_DIR_NAMES for part in rel_parts):
            continue
        out.append(p)
    return out


def check_text_and_syntax(root: Path = ROOT) -> Findings:
    f = Findings([])
    text_files = iter_repo_text_files(root)
    checked_text = 0
    checked_structured = 0

    for p in text_files:
        rel = p.relative_to(root)
        try:
            content = p.read_text(encoding="utf-8")
        except UnicodeDecodeError as e:
            f.error(f"{rel}: UTF-8 デコードに失敗（{e}）")
            continue
        checked_text += 1
        if "\ufffd" in content:
            f.error(f"{rel}: 置換文字 U+FFFD（文字化け）が含まれている")
        if CONFLICT_MARKER_RE.search(content):
            f.error(f"{rel}: マージ競合マーカーが残存している")

        # tsconfig.json は JSONC（コメント可）のため純粋 JSON 検査から外す
        if p.suffix == ".json" and p.name != "tsconfig.json":
            checked_structured += 1
            try:
                json.loads(content)
            except json.JSONDecodeError as e:
                f.error(f"{rel}: JSON 構文エラー（{e}）")
        elif p.suffix == ".xml":
            checked_structured += 1
            try:
                ET.fromstring(content)
            except ET.ParseError as e:
                f.error(f"{rel}: XML 構文エラー（{e}）")

    if f.worst() == OK:
        f.ok(
            f"テキスト {checked_text} 件（文字化け・競合なし）・"
            f"JSON/XML {checked_structured} 件の構文が正常"
        )
    return f


# ---------------------------------------------------------------------------
# チェック 19: Rust モジュールの循環依存と層逆参照
# ---------------------------------------------------------------------------

_RUST_CHAR_LIT_RE = re.compile(
    r"b?'(?:\\(?:x[0-9a-fA-F]{2}|u\{[0-9a-fA-F_]+\}|[^'\n])|[^'\\\n])'"
)
_RUST_RAW_STR_RE = re.compile(r'(?:b|c)?r(#*)"')
_RUST_CFG_ATTR_START_RE = re.compile(r"#\s*\[\s*cfg\s*\(")
_RUST_CFG_ATTR_HEAD_RE = re.compile(r"cfg_attr\s*\(")
_RUST_MACRO_EXPORT_ATTR_RE = re.compile(r"macro_export(?:\s*\(.*\))?\Z", re.DOTALL)
_RUST_IDENT_RE = re.compile(r"[A-Za-z_][A-Za-z0-9_]*")
_RUST_RAW_OR_IDENT_RE = re.compile(r"(?:r#)?[A-Za-z_][A-Za-z0-9_]*")
_RUST_CFG_TOKEN_RE = re.compile(
    r'\s*(?:([(),=])|"(?:\\.|[^"\\])*"|([A-Za-z_][A-Za-z0-9_]*))'
)
# std / core / alloc 直下で型・モジュール名前空間に属することが確定している標準モジュール
_RUST_STD_MODULES: frozenset[str] = frozenset({
    "alloc", "any", "arch", "array", "ascii", "backtrace", "borrow", "boxed",
    "cell", "char", "clone", "cmp", "collections", "convert", "default", "env",
    "error", "f32", "f64", "ffi", "fmt", "fs", "future", "hash", "hint",
    "i8", "i16", "i32", "i64", "i128", "io", "isize", "iter", "marker", "mem",
    "net", "num", "ops", "option", "os", "panic", "path", "pin", "prelude",
    "primitive", "process", "ptr", "rc", "result", "slice", "str", "string",
    "sync", "task", "thread", "time", "u8", "u16", "u32", "u64", "u128",
    "usize", "vec",
})


def _is_ident_char(ch: str) -> bool:
    return ch.isalnum() or ch == "_"


def _blank_rust_comments_and_strings(text: str) -> str:
    """Rust ソースのコメントを空白化し、文字列/文字リテラルは引用符のみ残して中身を行数維持で空白化する。"""
    out: list[str] = []
    i = 0
    n = len(text)
    while i < n:
        ch = text[i]
        # 行コメント // ...
        if ch == "/" and i + 1 < n and text[i + 1] == "/":
            while i < n and text[i] != "\n":
                out.append(" ")
                i += 1
            continue
        # ブロックコメント /* ... */（ネスト対応）
        if ch == "/" and i + 1 < n and text[i + 1] == "*":
            depth = 1
            out.extend((" ", " "))
            i += 2
            while i < n and depth > 0:
                if text[i : i + 2] == "/*":
                    depth += 1
                    out.extend((" ", " "))
                    i += 2
                elif text[i : i + 2] == "*/":
                    depth -= 1
                    out.extend((" ", " "))
                    i += 2
                else:
                    out.append("\n" if text[i] == "\n" else " ")
                    i += 1
            continue
        # raw 文字列 (b|c)?r#*"..."#*
        if (
            ch in ("r", "b", "c")
            and (i == 0 or not _is_ident_char(text[i - 1]))
        ):
            raw_m = _RUST_RAW_STR_RE.match(text, i)
            if raw_m is not None:
                closer = '"' + "#" * len(raw_m.group(1))
                end_pos = text.find(closer, raw_m.end())
                limit = n if end_pos < 0 else end_pos + len(closer)
                if limit - i >= 2 and end_pos >= 0:
                    out.append('"')
                    for k in range(i + 1, limit - 1):
                        out.append("\n" if text[k] == "\n" else " ")
                    out.append('"')
                else:
                    for k in range(i, limit):
                        out.append("\n" if text[k] == "\n" else " ")
                i = limit
                continue
        # byte / C / 通常文字列
        if ch == '"' or (
            ch in ("b", "c")
            and i + 1 < n
            and text[i + 1] == '"'
            and (i == 0 or not _is_ident_char(text[i - 1]))
        ):
            lit_start = i
            start = i if ch == '"' else i + 1
            i = start + 1
            closed = False
            while i < n:
                c2 = text[i]
                if c2 == "\\":
                    i += 2 if i + 1 < n else 1
                    continue
                i += 1
                if c2 == '"':
                    closed = True
                    break
            if closed and i - lit_start >= 2:
                out.append('"')
                for k in range(lit_start + 1, i - 1):
                    out.append("\n" if text[k] == "\n" else " ")
                out.append('"')
            else:
                for k in range(lit_start, i):
                    out.append("\n" if text[k] == "\n" else " ")
            continue
        # 文字リテラル ('x', '"', '\u{...}', b'x')。ライフタイム ('a) は維持する
        if ch == "'" or (
            ch == "b"
            and i + 1 < n
            and text[i + 1] == "'"
            and (i == 0 or not _is_ident_char(text[i - 1]))
        ):
            char_m = _RUST_CHAR_LIT_RE.match(text, i)
            if char_m is not None:
                span = char_m.end() - i
                if span >= 2:
                    out.append("'")
                    out.extend(" " for _ in range(span - 2))
                    out.append("'")
                else:
                    out.extend(" " for _ in range(span))
                i = char_m.end()
                continue
        out.append(ch)
        i += 1
    return "".join(out)


class _CfgParseError(Exception):
    pass


def _eval_cfg_non_test(expr: str) -> bool | None:
    """`cfg(...)` 式を `test = False`（他の条件は不定）の 3 値論理で評価する。

    戻り値:
        True  : `test = False` で必ず有効
        False : `test = False` では有効になり得ない（テスト専用）
        None  : 他の条件（OS・feature 等）次第で本番でも有効になり得る
    """
    tokens: list[tuple[str, str]] = []
    pos = 0
    n = len(expr)
    while pos < n:
        if expr[pos].isspace():
            pos += 1
            continue
        m = _RUST_CFG_TOKEN_RE.match(expr, pos)
        if m is None:
            return None
        if m.group(1) is not None:
            tokens.append(("PUNCT", m.group(1)))
        elif m.group(2) is not None:
            tokens.append(("IDENT", m.group(2)))
        else:
            tokens.append(("STR", m.group(0).strip()))
        pos = m.end()

    if not tokens:
        return None

    idx = 0
    tlen = len(tokens)

    def peek() -> tuple[str, str] | None:
        return tokens[idx] if idx < tlen else None

    def consume() -> tuple[str, str]:
        nonlocal idx
        if idx >= tlen:
            raise _CfgParseError()
        tok = tokens[idx]
        idx += 1
        return tok

    def parse_pred() -> bool | None:
        tok = consume()
        if tok[0] != "IDENT":
            raise _CfgParseError()
        name = tok[1]
        nxt = peek()
        if name in ("all", "any") and nxt == ("PUNCT", "("):
            consume()
            vals: list[bool | None] = []
            while True:
                p = peek()
                if p is None:
                    raise _CfgParseError()
                if p == ("PUNCT", ")"):
                    consume()
                    break
                vals.append(parse_pred())
                sep = peek()
                if sep == ("PUNCT", ","):
                    consume()
                elif sep == ("PUNCT", ")"):
                    continue
                else:
                    raise _CfgParseError()
            if name == "all":
                if any(v is False for v in vals):
                    return False
                if all(v is True for v in vals):
                    return True
                return None
            if any(v is True for v in vals):
                return True
            if all(v is False for v in vals):
                return False
            return None

        if name == "not" and nxt == ("PUNCT", "("):
            consume()
            val = parse_pred()
            if peek() == ("PUNCT", ","):
                consume()
            if peek() != ("PUNCT", ")"):
                raise _CfgParseError()
            consume()
            if val is True:
                return False
            if val is False:
                return True
            return None

        if nxt == ("PUNCT", "="):
            consume()
            # 文字列リテラルは前処理で空白化済みの場合があるため値は任意
            val_tok = peek()
            if val_tok is not None and val_tok[0] in ("STR", "IDENT"):
                consume()
            return None

        if nxt == ("PUNCT", "("):
            consume()
            depth = 1
            while depth > 0:
                t = consume()
                if t == ("PUNCT", "("):
                    depth += 1
                elif t == ("PUNCT", ")"):
                    depth -= 1
            return None

        if name == "test":
            return False
        if name == "true":
            return True
        if name == "false":
            return False
        return None

    try:
        res = parse_pred()
        if peek() == ("PUNCT", ","):
            consume()
        if idx != tlen:
            return None
        return res
    except _CfgParseError:
        return None


def _match_rust_angle_bracket(text: str, start: int) -> int | None:
    """`start` の `<` に対応するジェネリクスの閉じ `>` の直後位置を返す。"""
    n = len(text)
    angle_depth = 1
    paren_depth = 0
    bracket_depth = 0
    brace_depth = 0
    k = start + 1
    while k < n:
        c = text[k]
        if c == "(":
            paren_depth += 1
        elif c == ")":
            if paren_depth == 0:
                return None
            paren_depth -= 1
        elif c == "[":
            bracket_depth += 1
        elif c == "]":
            if bracket_depth == 0:
                return None
            bracket_depth -= 1
        elif c == "{":
            brace_depth += 1
        elif c == "}":
            if brace_depth == 0:
                return None
            brace_depth -= 1
        elif paren_depth == 0 and bracket_depth == 0 and brace_depth == 0:
            two = text[k : k + 2]
            if two in ("->", "=>", "<=", "<<"):
                k += 2
                continue
            if c == ";":
                return None
            if c == "<":
                angle_depth += 1
            elif c == ">":
                angle_depth -= 1
                if angle_depth == 0:
                    return k + 1
        k += 1
    return None


def _skip_rust_balanced(
    text: str, pos: int, open_ch: str, close_ch: str
) -> int:
    """`pos` の `open_ch` に対応する `close_ch` の直後位置を返す。"""
    n = len(text)
    if pos >= n or text[pos] != open_ch:
        return pos
    depth = 1
    p = pos + 1
    while p < n and depth > 0:
        c = text[p]
        if c == open_ch:
            depth += 1
        elif c == close_ch:
            depth -= 1
        p += 1
    return p


def _skip_rust_label(text: str, pos: int) -> int:
    """`'label:`（または `'r#label:`）があればその直後の非空白位置を返す。"""
    n = len(text)
    if pos >= n or text[pos] != "'":
        return pos
    lbl_m = _RUST_RAW_OR_IDENT_RE.match(text, pos + 1)
    if lbl_m is None:
        return pos
    after_lbl = lbl_m.end()
    while after_lbl < n and text[after_lbl].isspace():
        after_lbl += 1
    if (
        after_lbl < n
        and text[after_lbl] == ":"
        and text[after_lbl : after_lbl + 2] != "::"
    ):
        p = after_lbl + 1
        while p < n and text[p].isspace():
            p += 1
        return p
    return pos


def _skip_rust_abi_string(text: str, pos: int) -> int:
    """Rust の文字列リテラル（通常文字列・raw 文字列・空白化済み引用符）を読み飛ばす。"""
    n = len(text)
    if pos >= n:
        return pos
    if text[pos] == '"':
        p = pos + 1
        while p < n:
            if text[p] == "\\":
                p += 2 if p + 1 < n else 1
                continue
            if text[p] == '"':
                return p + 1
            p += 1
        return n
    if text[pos] == "r":
        raw_m = _RUST_RAW_STR_RE.match(text, pos)
        if raw_m is not None:
            closer = '"' + "#" * len(raw_m.group(1))
            end_pos = text.find(closer, raw_m.end())
            return n if end_pos < 0 else end_pos + len(closer)
    return pos


def _skip_rust_item_modifiers(text: str, pos: int) -> tuple[int, bool, bool]:
    """Rust アイテム修飾子（`default`/`const`/`async`/`unsafe`/`extern ["ABI"]`）を読み飛ばす。

    戻り値: `(修飾子直後の位置, saw_const, saw_extern)`
    """
    n = len(text)
    p = pos
    saw_const = False
    saw_extern = False
    while p < n:
        while p < n and text[p].isspace():
            p += 1
        m = _RUST_IDENT_RE.match(text, p)
        if m is None:
            break
        w = m.group(0)
        if w in ("default", "const", "async", "unsafe", "extern"):
            if w == "const":
                saw_const = True
            elif w == "extern":
                saw_extern = True
            p = m.end()
            while p < n and text[p].isspace():
                p += 1
            if w == "extern":
                after_abi = _skip_rust_abi_string(text, p)
                if after_abi != p:
                    p = after_abi
                    while p < n and text[p].isspace():
                        p += 1
            continue
        break
    return p, saw_const, saw_extern


def _scan_rust_number_literal(text: str, pos: int) -> int:
    """`pos` から始まる Rust の整数・浮動小数点リテラル（末尾 `.` 含む）の直後位置を返す。"""
    n = len(text)
    p = pos
    if p >= n or not text[p].isdigit():
        return pos
    # 基数接頭辞 0x / 0b / 0o は小数点を持たない
    if text[p] == "0" and p + 1 < n and text[p + 1] in "xXbBoO":
        p += 2
        while p < n and (text[p].isalnum() or text[p] == "_"):
            p += 1
        return p
    # 10 進整数部
    while p < n and (text[p].isdigit() or text[p] == "_"):
        p += 1
    # 小数点部: `1.5`, `1.5e-3_f64`, または末尾ドットの `1.`（`..`・識別子直前を除く）
    if p < n and text[p] == ".":
        nxt = text[p + 1] if p + 1 < n else ""
        if nxt.isdigit():
            p += 1
            while p < n and (text[p].isdigit() or text[p] == "_"):
                p += 1
            if p < n and text[p] in "eE":
                exp = p + 1
                if exp < n and text[exp] in "+-":
                    exp += 1
                if exp < n and (text[exp].isdigit() or text[exp] == "_"):
                    p = exp
                    while p < n and (text[p].isdigit() or text[p] == "_"):
                        p += 1
            while p < n and (text[p].isalnum() or text[p] == "_"):
                p += 1
            return p
        if nxt != "." and nxt != "_" and not nxt.isalpha():
            return p + 1
        return p
    # 小数点なし指数表記: `1e5`, `1e-5_f64`
    if p < n and text[p] in "eE":
        exp = p + 1
        if exp < n and text[exp] in "+-":
            exp += 1
        if exp < n and (text[exp].isdigit() or text[exp] == "_"):
            p = exp
            while p < n and (text[p].isdigit() or text[p] == "_"):
                p += 1
    # 型サフィックス（u8 / usize / f64 等）
    while p < n and (text[p].isalnum() or text[p] == "_"):
        p += 1
    return p


def _split_rust_attr_args(text: str) -> list[str]:
    """Rust 属性括弧内のカンマ区切り引数を括弧・文字列を跨がず分割する。"""
    parts: list[str] = []
    p_depth = 0
    b_depth = 0
    k_depth = 0
    start = 0
    i = 0
    n = len(text)
    while i < n:
        ch = text[i]
        if ch in ('"', "r"):
            next_i = _skip_rust_abi_string(text, i)
            if next_i != i:
                i = next_i
                continue
        if ch == "(":
            p_depth += 1
        elif ch == ")":
            p_depth = max(0, p_depth - 1)
        elif ch == "[":
            b_depth += 1
        elif ch == "]":
            b_depth = max(0, b_depth - 1)
        elif ch == "{":
            k_depth += 1
        elif ch == "}":
            k_depth = max(0, k_depth - 1)
        elif ch == "," and p_depth == 0 and b_depth == 0 and k_depth == 0:
            parts.append(text[start:i])
            start = i + 1
        i += 1
    parts.append(text[start:])
    return parts


def _attr_grants_macro_export(attr_body: str) -> bool:
    """外側属性本文が非テスト構成で `macro_export` を付与し得るか判定する。"""
    body = attr_body.strip()
    if not body:
        return False
    if _RUST_MACRO_EXPORT_ATTR_RE.match(body) is not None:
        return True
    m = _RUST_CFG_ATTR_HEAD_RE.match(body)
    if m is None:
        return False
    open_paren = m.end() - 1
    p = open_paren + 1
    n = len(body)
    depth = 1
    while p < n and depth > 0:
        ch = body[p]
        if ch in ('"', "r"):
            next_p = _skip_rust_abi_string(body, p)
            if next_p != p:
                p = next_p
                continue
        if ch == "(":
            depth += 1
        elif ch == ")":
            depth -= 1
        p += 1
    if depth != 0 or body[p:].strip():
        return False
    inner = body[open_paren + 1 : p - 1]
    args = _split_rust_attr_args(inner)
    if len(args) < 2:
        return False
    predicate = args[0].strip()
    if _eval_cfg_non_test(predicate) is False:
        return False
    return any(_attr_grants_macro_export(arg) for arg in args[1:])


def _classify_rust_annotated_node(text: str, start: int) -> str:
    """属性直後の Rust 構文ノードの終端種別を判定する。

    戻り値:
        "semi_only"     : `;` のみで終端する構文（use / type / static / const 値 / let 等）
        "brace_or_semi" : `{ ... }` または `;` で終端するアイテム・ブロック
        "brace_macro"   : `path! { ... }` の波括弧マクロ呼び出し
        "control_flow"  : `if` / `while` / `for` / `loop` / `match`・ラベル付きブロック（`else` 連鎖対応）
        "expr_or_field" : フィールド・バリアント・引数・match アーム・式文等
    """
    n = len(text)
    p = start

    def skip_ws(pos: int) -> int:
        while pos < n and text[pos].isspace():
            pos += 1
        return pos

    p = skip_ws(p)
    if p >= n:
        return "expr_or_field"
    if text[p] == "{":
        return "brace_or_semi"

    # ラベル付きブロック・ループ `'label: { ... }` / `'label: loop/while/for ...`
    after_label = _skip_rust_label(text, p)
    if after_label != p:
        if after_label < n and text[after_label] == "{":
            return "control_flow"
        lm = _RUST_IDENT_RE.match(text, after_label)
        if lm is not None and lm.group(0) in ("loop", "while", "for"):
            return "control_flow"

    # 可視性修飾子 pub / pub(...) をスキップ
    m = _RUST_IDENT_RE.match(text, p)
    if m is not None and m.group(0) == "pub":
        p2 = skip_ws(m.end())
        if p2 < n and text[p2] == "(":
            p2 = skip_ws(_skip_rust_balanced(text, p2, "(", ")"))
        p = p2

    p, saw_const, saw_extern = _skip_rust_item_modifiers(text, p)

    if p < n and text[p] == "{":
        return "brace_or_semi"

    m = _RUST_IDENT_RE.match(text, p)
    kw = m.group(0) if m is not None else ""
    after_kw = skip_ws(m.end()) if m is not None else p

    if kw == "fn":
        # 関数ポインタ型 `fn(i32)` と関数アイテム `fn name(...)` を区別する
        if _RUST_IDENT_RE.match(text, after_kw) is not None:
            return "brace_or_semi"
        return "expr_or_field"
    if kw in ("struct", "enum", "trait", "impl", "mod"):
        return "brace_or_semi"
    if kw == "union" and _RUST_IDENT_RE.match(text, after_kw) is not None:
        return "brace_or_semi"
    if kw == "macro_rules" and after_kw < n and text[after_kw] == "!":
        return "brace_or_semi"
    if kw in ("use", "type", "static", "let", "return", "break", "continue"):
        return "semi_only"
    if saw_const:
        return "semi_only"
    if saw_extern and kw == "crate":
        return "semi_only"
    if kw in ("if", "while", "for", "loop", "match"):
        return "control_flow"

    # パス付きまたは単独の波括弧マクロ呼び出し `[::] path (! { ... })` を識別する
    mp = skip_ws(start)
    if mp + 1 < n and text[mp : mp + 2] == "::":
        mp = skip_ws(mp + 2)
    seg = _RUST_RAW_OR_IDENT_RE.match(text, mp)
    if seg is not None:
        mp = skip_ws(seg.end())
        while mp + 1 < n and text[mp : mp + 2] == "::":
            mp = skip_ws(mp + 2)
            seg = _RUST_RAW_OR_IDENT_RE.match(text, mp)
            if seg is None:
                mp = -1
                break
            mp = skip_ws(seg.end())
        if mp >= 0 and mp < n and text[mp] == "!":
            mp = skip_ws(mp + 1)
            if mp < n and text[mp] == "{":
                return "brace_macro"
    return "expr_or_field"


def _find_rust_block_expr_end(text: str, start: int) -> int | None:
    """`start` がブロック式（`{}` / `if` / `match` / `loop` / `while` / `for`）で始まる場合、
    条件部やパターン内の `{ ... }` と本体ブロックを区別して式全体の直後位置を返す。"""
    n = len(text)

    def skip_ws(idx: int) -> int:
        while idx < n and text[idx].isspace():
            idx += 1
        return idx

    def skip_outer_attrs(idx: int) -> int:
        idx = skip_ws(idx)
        while idx < n and text[idx : idx + 2] == "#[":
            idx = skip_ws(_skip_rust_balanced(text, idx + 1, "[", "]"))
        return idx

    p = skip_outer_attrs(start)
    # 任意ラベル `'label:` をスキップ
    p = _skip_rust_label(text, p)

    # ブロック修飾子（const / async / move / unsafe / try / gen）をスキップ
    while p < n:
        m = _RUST_IDENT_RE.match(text, p)
        if m is None:
            break
        if m.group(0) in ("const", "async", "move", "unsafe", "try", "gen"):
            p = skip_ws(m.end())
            continue
        break

    if p >= n:
        return None
    if text[p] == "{":
        end = _skip_rust_balanced(text, p, "{", "}")
        return end if end > p and text[end - 1] == "}" else None

    kw_m = _RUST_IDENT_RE.match(text, p)
    if kw_m is None:
        return None
    kw = kw_m.group(0)
    if kw not in ("if", "match", "loop", "while", "for"):
        return None

    p = skip_ws(kw_m.end())
    if kw == "loop":
        p = skip_outer_attrs(p)
        if p >= n or text[p] != "{":
            return None
        end = _skip_rust_balanced(text, p, "{", "}")
        return end if end > p and text[end - 1] == "}" else None

    # `if` / `while` / `for` / `match` のヘッダ部（条件式・パターン・イテレータ）を走査して本体 `{` を特定する
    # head_state:
    #   "for_pat"   : `for <pat> in` の `in` 待ち
    #   "let_pat"   : `if/while let <pat> =` の `=` 待ち
    #   "need_expr" : 演算子・キーワード直後（次の `{` は条件内のブロック式またはマクロ引数）
    #   "have_expr" : 完結した条件式の直後（次の `{` が制御構文の本体ブロック）
    head_state = "for_pat" if kw == "for" else "need_expr"
    in_as_type = False
    body_brace_pos = -1

    while p < n:
        p = skip_ws(p)
        if p >= n:
            break
        c = text[p]
        if c == '"':
            p = _skip_rust_abi_string(text, p)
            if head_state == "need_expr":
                head_state = "have_expr"
            continue
        if c == "'":
            if not in_as_type:
                after_lbl = _skip_rust_label(text, p)
                if after_lbl != p:
                    sub_end = _find_rust_block_expr_end(text, p)
                    if sub_end is not None:
                        p = sub_end
                        if head_state == "need_expr":
                            head_state = "have_expr"
                        continue
                    p = after_lbl
                    continue
            # 空白化済み文字リテラル `'   '` またはライフタイム `'a`
            if p + 1 < n and text[p + 1] in (" ", "'"):
                q = text.find("'", p + 1)
                p = (q + 1) if q >= 0 else n
                if head_state == "need_expr":
                    head_state = "have_expr"
                in_as_type = False
                continue
            lt_m = _RUST_RAW_OR_IDENT_RE.match(text, p + 1)
            p = lt_m.end() if lt_m is not None else p + 1
            continue
        if c == "(":
            p = _skip_rust_balanced(text, p, "(", ")")
            if head_state == "need_expr":
                head_state = "have_expr"
            continue
        if c == "[":
            p = _skip_rust_balanced(text, p, "[", "]")
            if head_state == "need_expr":
                head_state = "have_expr"
            continue
        if c == "{":
            if head_state == "have_expr":
                body_brace_pos = p
                break
            p = _skip_rust_balanced(text, p, "{", "}")
            if head_state == "need_expr":
                head_state = "have_expr"
            in_as_type = False
            continue
        if c in (";", ")", "]", "}"):
            return None
        if c == "#" and text[p : p + 2] == "#[":
            p = _skip_rust_balanced(text, p + 1, "[", "]")
            continue

        ident_m = _RUST_RAW_OR_IDENT_RE.match(text, p)
        if ident_m is not None:
            word = ident_m.group(0)
            if head_state == "for_pat":
                if word == "in":
                    head_state = "need_expr"
                p = ident_m.end()
                continue
            if head_state == "let_pat":
                p = ident_m.end()
                continue
            if word == "let" and kw in ("if", "while") and head_state == "need_expr":
                head_state = "let_pat"
                in_as_type = False
                p = ident_m.end()
                continue
            if in_as_type:
                if word == "for":
                    p = skip_ws(ident_m.end())
                    if p < n and text[p] == "<":
                        angle_end = _match_rust_angle_bracket(text, p)
                        if angle_end is not None:
                            p = angle_end
                    head_state = "need_expr"
                    continue
                if word == "extern":
                    p = skip_ws(ident_m.end())
                    after_abi = _skip_rust_abi_string(text, p)
                    if after_abi != p:
                        p = after_abi
                    head_state = "need_expr"
                    continue
                if word in ("const", "mut", "unsafe", "dyn", "fn", "impl", "as"):
                    head_state = "need_expr"
                    p = ident_m.end()
                    continue
                head_state = "have_expr"
                p = ident_m.end()
                continue
            if word in ("if", "while", "for", "loop", "match"):
                sub_end = _find_rust_block_expr_end(text, p)
                if sub_end is not None:
                    p = sub_end
                    head_state = "have_expr"
                    continue
            if word == "as":
                head_state = "need_expr"
                in_as_type = True
                p = ident_m.end()
                continue
            if word in (
                "in",
                "return",
                "break",
                "continue",
                "yield",
                "ref",
                "mut",
                "move",
                "async",
                "const",
                "unsafe",
                "try",
                "gen",
                "dyn",
                "box",
            ):
                head_state = "need_expr"
            else:
                head_state = "have_expr"
            p = ident_m.end()
            continue

        if c.isdigit():
            p = _scan_rust_number_literal(text, p)
            if head_state == "need_expr":
                head_state = "have_expr"
            in_as_type = False
            continue

        if head_state == "let_pat":
            if text[p : p + 3] == "..=":
                p += 3
                continue
            if (
                c == "="
                and text[p : p + 2] not in ("==", "=>")
                and (p == 0 or text[p - 1] not in ("!", "<", ">", "="))
            ):
                head_state = "need_expr"
                in_as_type = False
            p += 1
            continue
        if head_state == "for_pat":
            p += 1
            continue

        if text[p : p + 2] == "::":
            p += 2
            head_state = "need_expr"
            continue
        if (
            c == "<"
            and p + 1 < n
            and text[p + 1] not in ("<", "=")
            and (in_as_type or head_state == "need_expr")
        ):
            angle_end = _match_rust_angle_bracket(text, p)
            if angle_end is not None:
                p = angle_end
                head_state = "have_expr"
                continue
        if text[p : p + 2] == "->":
            p += 2
            head_state = "need_expr"
            continue
        if in_as_type and head_state == "need_expr":
            if text[p : p + 2] == "&&":
                p += 2
                continue
            if c in ("*", "&"):
                p += 1
                continue
            if c == "!":
                p += 1
                head_state = "have_expr"
                continue
        if text[p : p + 3] == "..=":
            p += 3
            head_state = "need_expr"
            in_as_type = False
            continue
        if text[p : p + 2] == "..":
            # `for x in 0.. { ... }` や `..` の直後は本体 `{` が来得る
            p += 2
            head_state = "have_expr"
            in_as_type = False
            continue
        if c == "?":
            p += 1
            head_state = "have_expr"
            in_as_type = False
            continue
        if text[p : p + 2] in (
            "&&",
            "||",
            "==",
            "!=",
            "<=",
            ">=",
            "<<",
            ">>",
            "+=",
            "-=",
            "*=",
            "/=",
            "%=",
            "&=",
            "|=",
            "^=",
        ):
            p += 2
            head_state = "need_expr"
            in_as_type = False
            continue
        p += 1
        head_state = "need_expr"
        in_as_type = False

    if body_brace_pos < 0:
        return None
    end = _skip_rust_balanced(text, body_brace_pos, "{", "}")
    if end <= body_brace_pos or text[end - 1] != "}":
        return None

    # `if ... { ... } else ...` 連鎖を末尾まで消費する
    if kw == "if":
        after = skip_ws(end)
        else_m = _RUST_IDENT_RE.match(text, after)
        if else_m is not None and else_m.group(0) == "else":
            else_end = _find_rust_block_expr_end(text, else_m.end())
            if else_end is not None:
                return else_end
    return end


def _classify_post_brace_continuation(
    text: str, close_brace_pos: int
) -> tuple[str, int]:
    """`}` 直後の後続トークンから、式・構文が継続するか終端するかを判定する。"""
    n = len(text)
    p = close_brace_pos + 1
    crossed_newline = False
    while p < n and text[p].isspace():
        if text[p] == "\n":
            crossed_newline = True
        p += 1
    if p >= n:
        return ("stop", close_brace_pos + 1)

    ident_m = _RUST_IDENT_RE.match(text, p)
    if ident_m is not None:
        word = ident_m.group(0)
        if word == "else":
            return ("else", ident_m.end())
        if word == "if":
            return ("if", ident_m.end())
        if word == "as":
            return ("continue", ident_m.end())
        return ("stop", close_brace_pos + 1)

    two = text[p : p + 2]
    if two == "=>":
        return ("fat_arrow", p + 2)
    if two == "..":
        return ("range", p + 2)
    if two in (
        "==",
        "!=",
        "<=",
        ">=",
        "&&",
        "||",
        "+=",
        "-=",
        "*=",
        "/=",
        "%=",
        "&=",
        "|=",
        "^=",
        "<<",
        ">>",
    ):
        return ("continue", p + 2)

    ch = text[p]
    if ch == ".":
        return ("dot", p + 1)
    if ch == "?":
        return ("question", p + 1)
    if ch == "=":
        return ("assign", p + 1)
    if ch == "|":
        return ("pipe_nl" if crossed_newline else "pipe", p + 1)
    if ch in ("+", "/", "%", "^", "<", ">"):
        return ("continue", p + 1)
    if not crossed_newline and ch in ("-", "*", "&"):
        return ("continue", p + 1)
    if not crossed_newline and ch in ("[", "("):
        return ("continue", p)
    if ch == ";":
        return ("semi", p + 1)
    if ch == ",":
        return ("comma", p + 1)
    return ("stop", close_brace_pos + 1)


def _has_match_guard_or_pattern_fat_arrow(
    text: str, start: int, in_guard: bool = False
) -> bool:
    """波括弧マクロ直後の `if` / `|` が match アームのガード・OR パターン（トップレベル `=>` に到達）かを判定する。"""
    n = len(text)
    p = start
    b_depth = 0
    p_depth = 0
    k_depth = 0
    while p < n:
        c = text[p]
        if b_depth == 0 and p_depth == 0 and k_depth == 0:
            if text[p : p + 2] == "=>":
                return True
            if c in (";", ","):
                return False
            if (
                not in_guard
                and c == "i"
                and text[p : p + 2] == "if"
                and (
                    p == 0
                    or (
                        not _is_ident_char(text[p - 1])
                        and text[p - 1] not in (".", ":")
                    )
                )
                and not (p >= 2 and text[p - 2 : p] == "r#")
                and (p + 2 >= n or not _is_ident_char(text[p + 2]))
            ):
                in_guard = True
                p += 2
                continue
        if c == "(":
            p_depth += 1
        elif c == ")":
            if p_depth == 0:
                return False
            p_depth -= 1
        elif c == "[":
            k_depth += 1
        elif c == "]":
            if k_depth == 0:
                return False
            k_depth -= 1
        elif c == "{":
            b_depth += 1
        elif c == "}":
            if b_depth == 0:
                return False
            b_depth -= 1
            if b_depth == 0 and p_depth == 0 and k_depth == 0:
                cont_kind, next_pos = _classify_post_brace_continuation(text, p)
                if cont_kind == "fat_arrow":
                    return True
                if not in_guard:
                    if cont_kind == "if":
                        in_guard = True
                        p = next_pos
                        continue
                    if cont_kind in ("pipe", "pipe_nl"):
                        p = next_pos
                        continue
                    return False
                if cont_kind in (
                    "else",
                    "continue",
                    "dot",
                    "question",
                    "assign",
                    "pipe",
                ):
                    p = next_pos
                    continue
                return False
        p += 1
    return False


def _find_rust_annotated_node_end(text: str, start: int) -> int:
    """`#[cfg(...)]` 直後に続く単一 Rust 構文ノードの終端位置を返す。"""
    n = len(text)
    j = start
    # 後続の外側属性 #[...] をスキップ
    while j < n:
        while j < n and text[j].isspace():
            j += 1
        if text[j : j + 2] == "#[":
            j = _skip_rust_balanced(text, j + 1, "[", "]")
            continue
        break

    mode = _classify_rust_annotated_node(text, j)
    brace_depth = 0
    paren_depth = 0
    bracket_depth = 0
    saw_brace = False
    k = j

    if mode == "control_flow":
        block_end = _find_rust_block_expr_end(text, j)
        if block_end is None:
            # 制御構文の本体終端を安全に特定できない場合、後続の本番コードまで誤って除外しない
            return j
        cont_kind, next_pos = _classify_post_brace_continuation(
            text, block_end - 1
        )
        if cont_kind in ("dot", "question", "continue", "assign", "pipe"):
            mode = "expr_or_field"
            k = next_pos
        elif cont_kind in ("semi", "comma"):
            return next_pos
        else:
            return block_end

    def handle_match_arm_body(after_arrow: int) -> tuple[bool, int]:
        """`=>` 直後の match アーム本体を処理する。`(終端確定したか, 次位置)` を返す。"""
        block_end = _find_rust_block_expr_end(text, after_arrow)
        if block_end is None:
            return (False, after_arrow)
        cont_kind, next_pos = _classify_post_brace_continuation(
            text, block_end - 1
        )
        if cont_kind in ("dot", "question"):
            # ブロック式の後置 `.` / `?` 以降は非ブロック式となり `,` または外側 `}` まで継続する
            return (False, next_pos)
        if cont_kind == "comma":
            return (True, next_pos)
        return (True, block_end)

    while k < n:
        c = text[k]
        if (
            mode == "expr_or_field"
            and c == "<"
            and brace_depth == 0
            and paren_depth == 0
            and bracket_depth == 0
            and k + 1 < n
            and text[k + 1] not in ("<", "=")
            and k > j
            and (text[k - 1].isalnum() or text[k - 1] in "_:")
        ):
            angle_end = _match_rust_angle_bracket(text, k)
            if angle_end is not None:
                k = angle_end
                continue

        if (
            mode == "expr_or_field"
            and c == "="
            and k + 1 < n
            and text[k + 1] == ">"
            and brace_depth == 0
            and paren_depth == 0
            and bracket_depth == 0
        ):
            done, next_k = handle_match_arm_body(k + 2)
            if done:
                return next_k
            k = next_k
            continue

        if c == "(":
            paren_depth += 1
        elif c == ")":
            if paren_depth == 0:
                return k
            paren_depth -= 1
        elif c == "[":
            bracket_depth += 1
        elif c == "]":
            if bracket_depth == 0:
                return k
            bracket_depth -= 1
        elif c == "{":
            brace_depth += 1
            saw_brace = True
        elif c == "}":
            if brace_depth == 0:
                # 構造体宣言・構造体リテラル等の外側閉じ括弧は残す
                return k
            brace_depth -= 1
            if (
                brace_depth == 0
                and paren_depth == 0
                and bracket_depth == 0
            ):
                if mode == "brace_or_semi" and saw_brace:
                    return k + 1
                if mode == "brace_macro" and saw_brace:
                    cont_kind, next_pos = _classify_post_brace_continuation(text, k)
                    if cont_kind == "fat_arrow":
                        mode = "expr_or_field"
                        done, next_k = handle_match_arm_body(next_pos)
                        if done:
                            return next_k
                        k = next_k
                        continue
                    if cont_kind in ("if", "pipe", "pipe_nl"):
                        if _has_match_guard_or_pattern_fat_arrow(
                            text, next_pos, in_guard=(cont_kind == "if")
                        ):
                            mode = "expr_or_field"
                            k = next_pos
                            continue
                        return k + 1
                    if cont_kind in ("dot", "question", "continue", "assign"):
                        mode = "expr_or_field"
                        k = next_pos
                        continue
                    if cont_kind in ("semi", "comma"):
                        return next_pos
                    return k + 1
        elif (
            c == ";"
            and brace_depth == 0
            and paren_depth == 0
            and bracket_depth == 0
        ):
            return k + 1
        elif (
            mode == "expr_or_field"
            and c == ","
            and brace_depth == 0
            and paren_depth == 0
            and bracket_depth == 0
        ):
            return k + 1
        k += 1
    return k


def _rewind_preceding_outer_attrs(text: str, floor: int, attr_start: int) -> int:
    """`#[cfg(...)]` 直前に付随する同一アイテムの外側属性 `#[...]`（`#![...]` 除く）の開始位置まで巻き戻す。"""
    strip_start = attr_start
    cur = attr_start
    while cur > floor:
        while cur > floor and text[cur - 1].isspace():
            cur -= 1
        if cur <= floor or text[cur - 1] != "]":
            break
        depth = 1
        p = cur - 2
        while p >= floor and depth > 0:
            if text[p] == "]":
                depth += 1
            elif text[p] == "[":
                depth -= 1
            p -= 1
        if depth != 0:
            break
        # `p + 1` が対応する `[`
        open_bracket = p + 1
        q = open_bracket - 1
        while q >= floor and text[q].isspace():
            q -= 1
        if q >= floor and text[q] == "#":
            # 内側属性 `#![...]` は巻き戻さない
            strip_start = q
            cur = q
            continue
        break
    return strip_start


def _strip_cfg_test_items(text: str) -> str:
    """`test = false` で有効になり得ない `#[cfg(...)]` 付き構文ノードを行数維持で空白化する。"""
    out: list[str] = []
    pos = 0
    n = len(text)
    while pos < n:
        m = _RUST_CFG_ATTR_START_RE.search(text, pos)
        if m is None:
            out.append(text[pos:])
            break
        # `cfg(...)` の対応する閉じ丸括弧と `]` を探索
        expr_start = m.end()
        k = expr_start
        pdepth = 1
        while k < n and pdepth > 0:
            if text[k] == "(":
                pdepth += 1
            elif text[k] == ")":
                pdepth -= 1
            k += 1
        if pdepth != 0:
            out.append(text[pos : m.end()])
            pos = m.end()
            continue
        expr_end = k - 1
        while k < n and text[k].isspace():
            k += 1
        if k >= n or text[k] != "]":
            out.append(text[pos:k])
            pos = k
            continue
        attr_end = k + 1

        # `test = false` で有効になり得る場合は除外せずそのまま残す
        if _eval_cfg_non_test(text[expr_start:expr_end]) is not False:
            out.append(text[pos:attr_end])
            pos = attr_end
            continue

        strip_start = _rewind_preceding_outer_attrs(text, pos, m.start())
        out.append(text[pos:strip_start])
        node_end = _find_rust_annotated_node_end(text, attr_end)
        for idx in range(strip_start, node_end):
            out.append("\n" if text[idx] == "\n" else " ")
        pos = node_end
    return "".join(out)


def _split_top_level_commas(text: str) -> list[str]:
    parts: list[str] = []
    depth = 0
    start = 0
    for idx, ch in enumerate(text):
        if ch == "{":
            depth += 1
        elif ch == "}":
            depth = max(0, depth - 1)
        elif ch == "," and depth == 0:
            parts.append(text[start:idx])
            start = idx + 1
    parts.append(text[start:])
    return parts


def _expand_use_tree(body: str) -> list[tuple[list[str], bool]]:
    """`use` ツリーを `(パスセグメント列, 末尾 {self} 明示フラグ)` の列へ展開する。"""
    paths: list[tuple[list[str], bool]] = []
    for raw_item in _split_top_level_commas(body):
        item = raw_item.strip()
        if not item:
            continue
        open_idx = item.find("{")
        if open_idx >= 0:
            depth = 0
            close_idx = -1
            for k in range(open_idx, len(item)):
                if item[k] == "{":
                    depth += 1
                elif item[k] == "}":
                    depth -= 1
                    if depth == 0:
                        close_idx = k
                        break
            if close_idx > open_idx:
                prefix = [
                    seg.strip()
                    for seg in item[:open_idx].split("::")
                    if seg.strip()
                ]
                inner = item[open_idx + 1 : close_idx]
                for sub_segs, sub_is_self in _expand_use_tree(inner):
                    if sub_segs == ["self"]:
                        if prefix:
                            paths.append((prefix, True))
                    else:
                        paths.append((prefix + sub_segs, sub_is_self))
                continue
        item = re.sub(
            r"\s+as\s+(?:(?:r#)?[A-Za-z_][A-Za-z0-9_]*|_)\s*$", "", item
        ).strip()
        segs = [seg.strip() for seg in item.split("::") if seg.strip()]
        if segs:
            paths.append((segs, False))
    return paths


def _extract_use_bindings_with_paths(
    body: str,
    prefix: list[str] | None = None,
    has_leading_colon: bool = False,
) -> list[tuple[list[str], str, bool, bool]]:
    """`use` ツリーが束縛する `(参照先パスセグメント列, 束縛識別子名, 末尾 {self} か, 先頭 :: 付きか)` の列を抽出する。"""
    bindings: list[tuple[list[str], str, bool, bool]] = []
    base = prefix or []
    for raw_item in _split_top_level_commas(body):
        item = raw_item.strip()
        if not item:
            continue
        item_leading_colon = (
            has_leading_colon if prefix is not None else item.startswith("::")
        )
        open_idx = item.find("{")
        if open_idx >= 0:
            depth = 0
            close_idx = -1
            for k in range(open_idx, len(item)):
                if item[k] == "{":
                    depth += 1
                elif item[k] == "}":
                    depth -= 1
                    if depth == 0:
                        close_idx = k
                        break
            if close_idx > open_idx:
                sub_prefix = base + [
                    seg.strip()
                    for seg in item[:open_idx].split("::")
                    if seg.strip()
                ]
                bindings.extend(
                    _extract_use_bindings_with_paths(
                        item[open_idx + 1 : close_idx],
                        sub_prefix,
                        has_leading_colon=item_leading_colon,
                    )
                )
                continue
        as_m = re.search(r"\s+as\s+((?:r#)?[A-Za-z_][A-Za-z0-9_]*|_)\s*$", item)
        alias: str | None = None
        path_part = item
        if as_m is not None:
            raw_alias = as_m.group(1)
            if raw_alias == "_":
                continue
            alias = raw_alias[2:] if raw_alias.startswith("r#") else raw_alias
            path_part = item[: as_m.start()].strip()
        segs = [seg.strip() for seg in path_part.split("::") if seg.strip()]
        if not segs:
            continue
        is_module_self = False
        if segs == ["self"]:
            if not base:
                continue
            full_segs = list(base)
            is_module_self = True
        else:
            full_segs = base + segs
        if alias is None:
            leaf = full_segs[-1]
            if leaf in {"*", "self", "super", "crate", "_"}:
                continue
            alias = leaf[2:] if leaf.startswith("r#") else leaf
        bindings.append((full_segs, alias, is_module_self, item_leading_colon))
    return bindings


def _extract_use_bound_names(
    body: str, prefix: list[str] | None = None
) -> set[str]:
    """トップレベル `use ...;` がクレート直下に束縛する識別子名を抽出する。"""
    return {
        alias for _, alias, _, _ in _extract_use_bindings_with_paths(body, prefix)
    }


def _extract_rust_root_items_detailed(cleaned: str) -> tuple[set[str], set[str]]:
    """前処理済み `lib.rs` のトップレベルでパス参照可能な非モジュール項目名と、
    そのうち型名前空間に属する項目名の集合を返す。"""
    items: set[str] = set()
    type_items: set[str] = set()
    n = len(cleaned)
    i = 0
    has_macro_export = False

    def skip_ws(pos: int) -> int:
        while pos < n and cleaned[pos].isspace():
            pos += 1
        return pos

    def skip_to_item_end(pos: int, semi_only: bool = False) -> int:
        b_depth = 0
        p_depth = 0
        k_depth = 0
        while pos < n:
            c = cleaned[pos]
            if c == "(":
                p_depth += 1
            elif c == ")":
                p_depth = max(0, p_depth - 1)
            elif c == "[":
                k_depth += 1
            elif c == "]":
                k_depth = max(0, k_depth - 1)
            elif c == "{":
                b_depth += 1
            elif c == "}":
                if b_depth > 0:
                    b_depth -= 1
                    if (
                        not semi_only
                        and b_depth == 0
                        and p_depth == 0
                        and k_depth == 0
                    ):
                        return pos + 1
                else:
                    return pos + 1
            elif c == ";" and b_depth == 0 and p_depth == 0 and k_depth == 0:
                return pos + 1
            pos += 1
        return pos

    while i < n:
        i = skip_ws(i)
        if i >= n:
            break
        if cleaned[i] == ";":
            has_macro_export = False
            i += 1
            continue
        # 属性 #[...] / #![...] を走査し、外側属性の #[macro_export] を記録する
        if cleaned[i : i + 2] == "#[" or cleaned[i : i + 3] == "#![":
            is_inner = cleaned[i : i + 3] == "#!["
            j = i + (3 if is_inner else 2)
            attr_body_start = j
            attr_depth = 1
            while j < n and attr_depth > 0:
                if cleaned[j] == "[":
                    attr_depth += 1
                elif cleaned[j] == "]":
                    attr_depth -= 1
                j += 1
            if not is_inner and attr_depth == 0:
                attr_body = cleaned[attr_body_start : j - 1].strip()
                if _attr_grants_macro_export(attr_body):
                    has_macro_export = True
            i = j
            continue

        item_has_macro_export = has_macro_export
        has_macro_export = False

        p = i
        m = _RUST_IDENT_RE.match(cleaned, p)
        if m is not None and m.group(0) == "pub":
            p = skip_ws(m.end())
            if p < n and cleaned[p] == "(":
                depth = 1
                p += 1
                while p < n and depth > 0:
                    if cleaned[p] == "(":
                        depth += 1
                    elif cleaned[p] == ")":
                        depth -= 1
                    p += 1
                p = skip_ws(p)

        p, saw_const, saw_extern = _skip_rust_item_modifiers(cleaned, p)

        if saw_extern and p < n and cleaned[p] == "{":
            block_end = _skip_rust_balanced(cleaned, p, "{", "}")
            if block_end > p + 1 and cleaned[block_end - 1] == "}":
                sub_items, sub_types = _extract_rust_root_items_detailed(
                    cleaned[p + 1 : block_end - 1]
                )
                items.update(sub_items)
                type_items.update(sub_types)
            i = block_end
            continue

        m = _RUST_RAW_OR_IDENT_RE.match(cleaned, p)
        if m is None:
            i = skip_to_item_end(p)
            continue
        raw_kw = m.group(0)
        kw = raw_kw[2:] if raw_kw.startswith("r#") else raw_kw
        after_kw = skip_ws(m.end())

        if kw == "mod":
            # サブモジュール宣言自体は lib 直下項目に含めない
            i = skip_to_item_end(after_kw)
            continue
        if kw == "use":
            semi = cleaned.find(";", after_kw)
            end = n if semi < 0 else semi + 1
            body = cleaned[after_kw : end - (1 if semi >= 0 else 0)]
            items.update(_extract_use_bound_names(body))
            i = end
            continue
        if saw_extern and kw == "crate":
            semi = cleaned.find(";", after_kw)
            end = n if semi < 0 else semi + 1
            body = cleaned[after_kw : end - (1 if semi >= 0 else 0)].strip()
            bound = _extract_use_bound_names(body)
            items.update(bound)
            type_items.update(bound)
            i = end
            continue
        if kw in ("fn", "struct", "enum", "trait", "type", "union"):
            nm = _RUST_RAW_OR_IDENT_RE.match(cleaned, after_kw)
            if nm is not None:
                name = nm.group(0)
                norm_name = name[2:] if name.startswith("r#") else name
                items.add(norm_name)
                if kw != "fn":
                    type_items.add(norm_name)
            i = skip_to_item_end(after_kw, semi_only=(kw == "type"))
            continue
        if kw == "static":
            nm = _RUST_RAW_OR_IDENT_RE.match(cleaned, after_kw)
            if nm is not None and nm.group(0) == "mut":
                after_kw = skip_ws(nm.end())
                nm = _RUST_RAW_OR_IDENT_RE.match(cleaned, after_kw)
            if nm is not None:
                name = nm.group(0)
                if name != "_":
                    items.add(name[2:] if name.startswith("r#") else name)
            i = skip_to_item_end(after_kw, semi_only=True)
            continue
        if saw_const:
            if kw != "_":
                items.add(kw)
            i = skip_to_item_end(after_kw, semi_only=True)
            continue
        if kw == "macro_rules" and after_kw < n and cleaned[after_kw] == "!":
            # `#[macro_export]` がない `macro_rules!` はクレートルートからのパス参照・`use crate::...` 対象にならない
            if item_has_macro_export:
                nm = _RUST_RAW_OR_IDENT_RE.match(cleaned, skip_ws(after_kw + 1))
                if nm is not None:
                    name = nm.group(0)
                    items.add(name[2:] if name.startswith("r#") else name)
            i = skip_to_item_end(after_kw + 1)
            continue

        i = skip_to_item_end(after_kw)

    return items, type_items


def _extract_rust_root_items(cleaned: str) -> set[str]:
    """前処理済み `lib.rs` のトップレベルでパス参照可能な非モジュール項目名を抽出する。"""
    items, _ = _extract_rust_root_items_detailed(cleaned)
    return items


def _is_guaranteed_type_ns_use(
    segs: list[str],
    alias: str,
    is_module_self: bool,
    has_leading_colon: bool,
    known_modules: set[str] | None,
    lib_root_type_items: set[str] | None,
) -> bool:
    """`use` 束縛が型・モジュール名前空間に属し、同名トップレベルモジュールを確実に遮蔽するか判定する。"""
    if not segs:
        return False
    norm0 = segs[0][2:] if segs[0].startswith("r#") else segs[0]
    norm1 = (
        (segs[1][2:] if segs[1].startswith("r#") else segs[1])
        if len(segs) >= 2
        else ""
    )
    # 同名クレート直下モジュール自身（またはその配下）を指す use は遮蔽扱いしない
    if (not has_leading_colon and len(segs) == 1 and norm0 == alias) or (
        not has_leading_colon
        and norm0 in ("crate", "self")
        and len(segs) >= 2
        and norm1 == alias
    ):
        return False
    if is_module_self:
        return True
    if has_leading_colon and len(segs) == 1:
        return True
    if not has_leading_colon and len(segs) == 1:
        if norm0 in ("std", "core", "alloc"):
            return True
        if known_modules and norm0 in known_modules and norm0 != alias:
            return True
        if lib_root_type_items and norm0 in lib_root_type_items:
            return True
        return False
    if not has_leading_colon and norm0 in ("crate", "self") and len(segs) == 2:
        if known_modules and norm1 in known_modules and norm1 != alias:
            return True
        if lib_root_type_items and norm1 in lib_root_type_items:
            return True
        return False
    if (
        norm0 in ("std", "core", "alloc")
        and len(segs) == 2
        and norm1 in _RUST_STD_MODULES
    ):
        return True
    return False


def _collect_block_local_shadows(
    cleaned: str,
    known_modules: set[str] | None = None,
    lib_root_type_items: set[str] | None = None,
) -> dict[int, set[str]]:
    """`lib.rs` の各スコープ（`-1` = トップレベル、各 `{` 位置 = ブロックスコープ）で
    型・モジュール名前空間において同名トップレベルモジュールを確実に遮蔽する宣言・別名を収集する。"""
    shadows: dict[int, set[str]] = {}
    brace_open_stack: list[int] = [-1]
    assoc_stack: list[bool] = [False]
    paren_depth = 0
    bracket_depth = 0
    pending_assoc_container = False
    n = len(cleaned)
    i = 0

    def is_item_keyword_start(pos: int) -> bool:
        q = pos - 1
        while q >= 0 and cleaned[q].isspace():
            q -= 1
        if q < 0 or cleaned[q] in (";", "{", "}", "]"):
            return True
        if cleaned[q] == ")":
            # `pub(...) trait` 等
            return True
        if _is_ident_char(cleaned[q]):
            s = q
            while s > 0 and _is_ident_char(cleaned[s - 1]):
                s -= 1
            return cleaned[s : q + 1] in ("pub", "unsafe", "default", "auto")
        return False

    while i < n:
        ch = cleaned[i]
        if ch == "(":
            paren_depth += 1
            i += 1
            continue
        if ch == ")":
            paren_depth = max(0, paren_depth - 1)
            i += 1
            continue
        if ch == "[":
            bracket_depth += 1
            i += 1
            continue
        if ch == "]":
            bracket_depth = max(0, bracket_depth - 1)
            i += 1
            continue
        if ch == "{":
            brace_open_stack.append(i)
            assoc_stack.append(pending_assoc_container)
            pending_assoc_container = False
            i += 1
            continue
        if ch == "}":
            if len(brace_open_stack) > 1:
                brace_open_stack.pop()
                assoc_stack.pop()
            pending_assoc_container = False
            i += 1
            continue
        if ch == ";":
            pending_assoc_container = False
            i += 1
            continue
        if (
            (ch.isalpha() or ch == "_")
            and (
                i == 0
                or (
                    not _is_ident_char(cleaned[i - 1])
                    and cleaned[i - 1] not in (".", ":")
                )
            )
            and not (i >= 2 and cleaned[i - 2 : i] == "r#")
        ):
            m = _RUST_IDENT_RE.match(cleaned, i)
            assert m is not None
            ident = m.group(0)
            j = m.end()
            if (
                ident == "impl"
                and paren_depth == 0
                and bracket_depth == 0
                and is_item_keyword_start(i)
            ):
                pending_assoc_container = True
                i = j
                continue
            if ident == "fn" and paren_depth == 0 and bracket_depth == 0:
                pending_assoc_container = False
                i = j
                continue
            if ident == "use" and j < n and cleaned[j].isspace():
                semi = cleaned.find(";", j)
                end = n if semi < 0 else semi + 1
                body = cleaned[j : end - (1 if semi >= 0 else 0)]
                scope_key = brace_open_stack[-1]
                for (
                    segs,
                    alias,
                    is_mod_self,
                    has_leading_colon,
                ) in _extract_use_bindings_with_paths(body):
                    if _is_guaranteed_type_ns_use(
                        segs,
                        alias,
                        is_mod_self,
                        has_leading_colon,
                        known_modules,
                        lib_root_type_items,
                    ):
                        shadows.setdefault(scope_key, set()).add(alias)
                pending_assoc_container = False
                i = end
                continue
            if ident == "extern" and j < n and cleaned[j].isspace():
                k = j
                while k < n and cleaned[k].isspace():
                    k += 1
                cm = _RUST_IDENT_RE.match(cleaned, k)
                if (
                    cm is not None
                    and cm.group(0) == "crate"
                    and cm.end() < n
                    and cleaned[cm.end()].isspace()
                ):
                    semi = cleaned.find(";", cm.end())
                    end = n if semi < 0 else semi + 1
                    body = cleaned[cm.end() : end - (1 if semi >= 0 else 0)]
                    scope_key = brace_open_stack[-1]
                    for _, alias, _, _ in _extract_use_bindings_with_paths(body):
                        shadows.setdefault(scope_key, set()).add(alias)
                    pending_assoc_container = False
                    i = end
                    continue
            if (
                ident == "trait"
                and paren_depth == 0
                and bracket_depth == 0
                and is_item_keyword_start(i)
            ):
                pending_assoc_container = True
            if len(brace_open_stack) > 1 and ident in (
                "struct",
                "enum",
                "trait",
                "type",
                "union",
                "mod",
            ):
                # impl / trait ブロック直下の関連型 `type Foo = ...;` はモジュール名を遮蔽しない
                if ident == "type" and assoc_stack[-1]:
                    i = j
                    continue
                k = j
                while k < n and cleaned[k].isspace():
                    k += 1
                nm = _RUST_RAW_OR_IDENT_RE.match(cleaned, k)
                if nm is not None:
                    raw_name = nm.group(0)
                    name = raw_name[2:] if raw_name.startswith("r#") else raw_name
                    shadows.setdefault(brace_open_stack[-1], set()).add(name)
                    i = nm.end()
                    continue
            i = j
            continue
        i += 1
    return shadows


def _is_module_qualified_after(cleaned: str, end_pos: int) -> bool:
    """識別子直後が `::` に続く非ターボフィッシュ（`::<` 以外）のパス修飾か判定する。"""
    n = len(cleaned)
    p = end_pos
    while p < n and cleaned[p].isspace():
        p += 1
    if cleaned[p : p + 2] != "::":
        return False
    p += 2
    while p < n and cleaned[p].isspace():
        p += 1
    return not (p < n and cleaned[p] == "<")


def _target_from_path_segments(
    segs: list[str],
    is_module_self: bool,
    current_depth: int,
    known_modules: set[str] | None = None,
    shadowed: set[str] | None = None,
) -> tuple[str, str] | None:
    if not segs:
        return None
    if segs[0] == "crate":
        if len(segs) >= 2 and segs[1] not in {"self", "super", "*"}:
            ref_kind = (
                "qualified" if (len(segs) >= 3 or is_module_self) else "bare_use"
            )
            return (segs[1], ref_kind)
        return None
    if current_depth == 0:
        if (
            segs[0] == "self"
            and len(segs) >= 2
            and segs[1] not in {"self", "super", "crate", "*"}
        ):
            ref_kind = (
                "qualified" if (len(segs) >= 3 or is_module_self) else "bare_use"
            )
            return (segs[1], ref_kind)
        if (
            known_modules
            and segs[0] in known_modules
            and (not shadowed or segs[0] not in shadowed)
        ):
            ref_kind = (
                "qualified" if (len(segs) >= 2 or is_module_self) else "bare_use"
            )
            return (segs[0], ref_kind)
    super_count = 0
    while super_count < len(segs) and segs[super_count] == "super":
        super_count += 1
    if (
        super_count > 0
        and super_count == current_depth
        and len(segs) > super_count
        and segs[super_count] not in {"self", "super", "crate", "*"}
    ):
        ref_kind = (
            "qualified"
            if (len(segs) > super_count + 1 or is_module_self)
            else "bare_use"
        )
        return (segs[super_count], ref_kind)
    return None


def _extract_rust_crate_targets(
    cleaned: str,
    file_depth: int,
    known_modules: set[str] | None = None,
    lib_root_type_items: set[str] | None = None,
) -> list[tuple[str, int, str]]:
    """前処理済み Rust コードからクレート内参照先（識別子, 行番号, 参照形態）を抽出する。"""
    targets: list[tuple[str, int, str]] = []
    brace_stack: list[bool] = []
    block_shadows = (
        _collect_block_local_shadows(
            cleaned,
            known_modules=known_modules,
            lib_root_type_items=lib_root_type_items,
        )
        if known_modules
        else {}
    )
    shadow_stack: list[set[str]] = [set(block_shadows.get(-1, ()))]
    pending_inline_mod = False
    line_no = 1
    i = 0
    n = len(cleaned)

    def current_shadowed() -> set[str]:
        if not block_shadows:
            return set()
        merged: set[str] = set()
        for scope_set in shadow_stack:
            merged.update(scope_set)
        return merged

    while i < n:
        ch = cleaned[i]
        if ch == "\n":
            line_no += 1
            i += 1
            continue
        if (ch.isalpha() or ch == "_") and (
            i == 0 or (not _is_ident_char(cleaned[i - 1]) and cleaned[i - 1] != ":")
        ):
            is_raw = i >= 2 and cleaned[i - 2 : i] == "r#"
            m = _RUST_IDENT_RE.match(cleaned, i)
            assert m is not None
            ident = m.group(0)
            j = m.end()
            # 制限付き可視性 `pub(in crate::feed)` / `pub(crate)` 等は祖先モジュール指定のため依存対象から除外する
            if not is_raw and ident == "pub":
                k = j
                while k < n and cleaned[k].isspace():
                    k += 1
                if k < n and cleaned[k] == "(":
                    end_vis = _skip_rust_balanced(cleaned, k, "(", ")")
                    line_no += cleaned[i:end_vis].count("\n")
                    i = end_vis
                    pending_inline_mod = False
                    continue
            if not is_raw and ident == "mod":
                k = j
                while k < n and cleaned[k].isspace():
                    k += 1
                name_m = _RUST_RAW_OR_IDENT_RE.match(cleaned, k)
                if name_m is not None:
                    k2 = name_m.end()
                    while k2 < n and cleaned[k2].isspace():
                        k2 += 1
                    if k2 < n and cleaned[k2] == "{":
                        pending_inline_mod = True
                        line_no += cleaned[i:k2].count("\n")
                        i = k2
                        continue
                pending_inline_mod = False
                i = j
                continue
            if not is_raw and ident == "use" and j < n and cleaned[j].isspace():
                semi = cleaned.find(";", j)
                end = n if semi < 0 else semi + 1
                body = cleaned[j : end - (1 if semi >= 0 else 0)]
                current_depth = file_depth + sum(1 for x in brace_stack if x)
                shadowed = current_shadowed()
                for segs, is_mod_self in _expand_use_tree(body):
                    resolved = _target_from_path_segments(
                        segs, is_mod_self, current_depth, known_modules, shadowed
                    )
                    if resolved is not None:
                        targets.append((resolved[0], line_no, resolved[1]))
                line_no += cleaned[i:end].count("\n")
                i = end
                pending_inline_mod = False
                continue
            if not is_raw and ident == "crate" and cleaned[j : j + 2] == "::":
                next_m = _RUST_IDENT_RE.match(cleaned, j + 2)
                if next_m is not None:
                    target = next_m.group(0)
                    if target not in {"self", "super"}:
                        ref_kind = (
                            "qualified"
                            if _is_module_qualified_after(cleaned, next_m.end())
                            else "bare_path"
                        )
                        targets.append((target, line_no, ref_kind))
                    i = next_m.end()
                    pending_inline_mod = False
                    continue
            if not is_raw and ident == "self" and cleaned[j : j + 2] == "::":
                current_depth = file_depth + sum(1 for x in brace_stack if x)
                next_m = _RUST_IDENT_RE.match(cleaned, j + 2)
                if current_depth == 0 and next_m is not None:
                    target = next_m.group(0)
                    if target not in {"self", "super", "crate"}:
                        ref_kind = (
                            "qualified"
                            if _is_module_qualified_after(cleaned, next_m.end())
                            else "bare_path"
                        )
                        targets.append((target, line_no, ref_kind))
                    i = next_m.end()
                    pending_inline_mod = False
                    continue
            if not is_raw and ident == "super" and cleaned[j : j + 2] == "::":
                super_count = 1
                pos = j + 2
                while cleaned[pos : pos + 7] == "super::":
                    super_count += 1
                    pos += 7
                next_m = _RUST_IDENT_RE.match(cleaned, pos)
                current_depth = file_depth + sum(1 for x in brace_stack if x)
                if next_m is not None and super_count == current_depth:
                    target = next_m.group(0)
                    if target not in {"self", "super", "crate"}:
                        ref_kind = (
                            "qualified"
                            if _is_module_qualified_after(cleaned, next_m.end())
                            else "bare_path"
                        )
                        targets.append((target, line_no, ref_kind))
                i = next_m.end() if next_m is not None else pos
                pending_inline_mod = False
                continue
            if (
                known_modules
                and ident in known_modules
                and _is_module_qualified_after(cleaned, j)
                and (i == 0 or cleaned[i - 1] != ".")
            ):
                current_depth = file_depth + sum(1 for x in brace_stack if x)
                if current_depth == 0 and ident not in current_shadowed():
                    targets.append((ident, line_no, "qualified"))
            pending_inline_mod = False
            i = j
            continue
        if ch == "{":
            brace_stack.append(pending_inline_mod)
            if block_shadows:
                shadow_stack.append(set(block_shadows.get(i, ())))
            pending_inline_mod = False
            i += 1
            continue
        if ch == "}":
            if brace_stack:
                brace_stack.pop()
            if block_shadows and len(shadow_stack) > 1:
                shadow_stack.pop()
            pending_inline_mod = False
            i += 1
            continue
        if not ch.isspace():
            pending_inline_mod = False
        i += 1

    return targets


def _is_rust_test_file(rel: Path) -> bool:
    if "tests" in rel.parts[:-1]:
        return True
    return rel.stem == "tests" or rel.stem.endswith("_tests")


def _find_rust_cycles(adj: dict[str, set[str]]) -> list[tuple[str, ...]]:
    """有向グラフ内の 2 ノード以上の基本循環を最小ノード先頭で列挙する。"""
    nodes = sorted(adj)
    cycles: set[tuple[str, ...]] = set()

    def dfs(start: str, curr: str, path: list[str], on_path: set[str]) -> None:
        for nxt in sorted(adj.get(curr, ())):
            if nxt == start:
                if len(path) >= 2:
                    cycles.add(tuple(path))
            elif nxt > start and nxt not in on_path:
                on_path.add(nxt)
                path.append(nxt)
                dfs(start, nxt, path, on_path)
                path.pop()
                on_path.remove(nxt)

    for start in nodes:
        dfs(start, start, [start], {start})
    return sorted(cycles)


def check_rust_module_cycles(src_tauri: Path = SRC_TAURI) -> Findings:
    """Rust モジュール間の循環依存と層逆参照（下位共有層→ドメイン層・ドメイン層/下位共有層→上位層）を検出する。"""
    f = Findings([])
    if not src_tauri.exists():
        f.error(f"Rust ソースディレクトリが存在しない: {src_tauri}")
        return f

    prod_files: list[tuple[Path, Path, str, int]] = []
    all_top_mods: set[str] = set()
    for path in sorted(src_tauri.rglob("*.rs")):
        rel = path.relative_to(src_tauri)
        if _is_rust_test_file(rel):
            continue
        parts = rel.parts
        top_mod = parts[0] if len(parts) >= 2 else path.stem
        if len(parts) == 1 and path.name in {"lib.rs", "main.rs"}:
            file_depth = 0
        elif path.name == "mod.rs":
            file_depth = len(parts) - 1
        else:
            file_depth = len(parts)
        all_top_mods.add(top_mod)
        prod_files.append((path, rel, top_mod, file_depth))

    known_modules = all_top_mods - {"lib", "main"}
    cleaned_cache: dict[Path, str] = {}
    lib_root_items: set[str] = set()
    lib_root_type_items: set[str] = set()
    lib_path = src_tauri / "lib.rs"
    if lib_path.exists():
        lib_cleaned = _strip_cfg_test_items(
            _blank_rust_comments_and_strings(read_text(lib_path))
        )
        cleaned_cache[lib_path] = lib_cleaned
        raw_root_items, raw_root_type_items = _extract_rust_root_items_detailed(
            lib_cleaned
        )
        lib_root_items = raw_root_items - {"lib", "main"}
        lib_root_type_items = raw_root_type_items - {"lib", "main"}

    adj: dict[str, set[str]] = {}
    reported_reverse: set[tuple[str, str]] = set()
    checked_files = 0

    for path, rel, top_mod, file_depth in prod_files:
        checked_files += 1
        cleaned = cleaned_cache.get(path)
        if cleaned is None:
            raw = read_text(path)
            cleaned = _strip_cfg_test_items(
                _blank_rust_comments_and_strings(raw)
            )
        root_known = known_modules if top_mod == "lib" else None
        root_types = lib_root_type_items if top_mod == "lib" else None
        for raw_dst, line_no, ref_kind in _extract_rust_crate_targets(
            cleaned,
            file_depth,
            known_modules=root_known,
            lib_root_type_items=root_types,
        ):
            is_known_mod = raw_dst in known_modules or raw_dst in RUST_UPPER_MODULES
            is_root_item = top_mod != "main" and raw_dst in lib_root_items
            if is_root_item and not is_known_mod:
                resolved_dsts = ["lib"]
            elif is_root_item and is_known_mod:
                if ref_kind == "bare_path":
                    resolved_dsts = ["lib"]
                elif ref_kind == "bare_use":
                    # `use crate::feed;` は値名前空間（lib 直下項目）と型名前空間（feed モジュール）の双方を取り込むため安全側で両方検査
                    resolved_dsts = ["lib", raw_dst]
                else:
                    resolved_dsts = [raw_dst]
            else:
                resolved_dsts = [raw_dst]

            for dst in resolved_dsts:
                if dst == top_mod:
                    continue
                if top_mod in RUST_UPPER_MODULES:
                    if dst in all_top_mods:
                        adj.setdefault(top_mod, set()).add(dst)
                    continue
                if dst in RUST_UPPER_MODULES:
                    pair = (top_mod, dst)
                    if pair not in reported_reverse:
                        reported_reverse.add(pair)
                        src_layer = (
                            "下位共有層" if top_mod in RUST_SHARED_MODULES else "ドメイン層"
                        )
                        f.error(
                            f"{src_layer} `{top_mod}` が上位層 `{dst}` を"
                            f"逆参照（{rel.as_posix()}:{line_no}）"
                        )
                    continue
                if dst not in known_modules:
                    continue
                if top_mod in RUST_SHARED_MODULES and dst not in RUST_SHARED_MODULES:
                    pair = (top_mod, dst)
                    if pair not in reported_reverse:
                        reported_reverse.add(pair)
                        f.error(
                            f"下位共有層 `{top_mod}` がドメイン層 `{dst}` を"
                            f"逆参照（{rel.as_posix()}:{line_no}）"
                        )
                    continue
                adj.setdefault(top_mod, set()).add(dst)

    for cycle in _find_rust_cycles(adj):
        f.error(f"Rust モジュール循環依存: {' -> '.join([*cycle, cycle[0]])}")

    if f.worst() == OK:
        f.ok(
            f"非テスト Rust {checked_files} ファイルに循環依存・層逆参照なし"
        )
    return f


# ---------------------------------------------------------------------------


def main() -> int:
    checks: list[tuple[str, Findings]] = []
    setting_keys = set(extract_setting_keys())
    ts_decl_ids, _ = scan_ts_test_ids()

    checks.append(("check 1: コマンド表", check_commands()))
    checks.append(("check 2: イベント表", check_events()))
    checks.append(("check 4: i18n キー", check_i18n(setting_keys)))
    checks.append(("check 5: テスト ID", check_test_ids(set(ts_decl_ids))))
    checks.append(("check 6: 設定キー表", check_settings(setting_keys)))
    checks.append(("check 7: CI 集約ジョブ", check_ci_gate()))
    checks.append(("check 8: 要件 ID", check_requirement_ids()))
    checks.append(("check 9: 決定記録 ID", check_decision_ids()))
    checks.append(("check 10: 設計書節参照", check_design_sections()))
    checks.append(("check 11: .test.ts テスト ID", check_ts_test_ids()))
    checks.append(("check 12: 窓ラベルと capability", check_window_labels()))
    checks.append(("check 13: 拡張・識別子定数", check_extension_constants()))
    checks.append(("check 14: docs 間リンク", check_doc_links()))
    checks.append(("check 15: リポジトリ構成ツリー", check_repo_tree()))
    checks.append(("check 16: 決定記録表構文", check_decision_tables()))
    checks.append(("check 17: 実装計画フェーズ番号", check_implementation_phases()))
    checks.append(("check 18: テキスト健全性と構文", check_text_and_syntax()))
    checks.append(("check 19: Rust モジュール循環依存・層方向", check_rust_module_cycles()))
    # チェック 3（§8 DDL ↔ 適用後スキーマ）は src-tauri の
    # ddl_matches_design_section8 テストが担う

    has_error = False
    for title, findings in checks:
        print(f"[{findings.worst()}] {title}")
        for level, message in findings.items:
            print(f"    {level}: {message}")
        has_error = has_error or findings.worst() == ERROR

    return 1 if has_error else 0


if __name__ == "__main__":
    sys.exit(main())
