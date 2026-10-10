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
    1. design.md §3.1 のコマンド表 ↔ lib.rs の generate_handler!
    2. design.md §3.2 のイベント表 ↔ emit / emit_to / listen の
       第 1 引数リテラル
    4. i18n の ja リソースキー ↔ フロントエンドの t() 参照
    5. テスト ID（DB-XX-NN）の docs 引用・コード側重複・#[test] 直下
    6. design.md の設定キー表 ↔ SETTING_* 定数リテラル
    7. ci.yml の集約ジョブ ci-status の needs ↔ jobs 一覧
    8. 要件 ID（FR-N / BG-N / INV-N）の参照 ↔ requirements-definition.md の見出し
    9. 決定記録 ID（仕様決定 / 技術方針）の参照 ↔ decision-records.md の表
    10. 設計書 §N / design.md §N の参照 ↔ design.md の見出し番号
    11. .test.ts のテスト ID（LP-NN 系）の宣言・索引・重複
    12. capabilities の windows glob ↔ コード側の窓ラベル
    13. 拡張・識別子定数（ホスト名・拡張 ID・スキーム・バージョン）の一致
    14. docs 間リンク `[..](path)` の参照先の存在
    15. design.md §11 リポジトリ構成ツリーの掲載パスの存在（WARN 限定）
チェック 3（§8 DDL と適用後スキーマの照合）は実マイグレーション経路を
通す必要があるため Rust 側テストとして src-tauri/src/db/tests/ddl.rs にある。

見出しへの anchor は節番号ではなく見出し語で行い、節の採番が変わっても
追従できるようにする。表の第 1 セルからバッククォート内の名前を取る規約は
§3.1 / §3.2 / 設定キー表で共通とする。
"""

from __future__ import annotations

import fnmatch
import json
import re
import sys
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DESIGN_MD = ROOT / "docs" / "design.md"
REQUIREMENTS_MD = ROOT / "docs" / "requirements-definition.md"
DECISIONS_MD = ROOT / "docs" / "decision-records.md"
USER_GUIDE_MD = ROOT / "docs" / "user-guide.md"
LIB_RS = ROOT / "src-tauri" / "src" / "lib.rs"
I18N_TS = ROOT / "src" / "lib" / "i18n.ts"
SRC_TAURI = ROOT / "src-tauri" / "src"
SRC_FRONTEND = ROOT / "src"
EXTENSION_DIR = ROOT / "extension"
TAURI_CONF = ROOT / "src-tauri" / "tauri.conf.json"
CAPABILITIES_DIR = ROOT / "src-tauri" / "capabilities"
WHEEL_LUA = ROOT / "src-tauri" / "mpv" / "wheel.lua"
DOCS_DIR = ROOT / "docs"

OK = "OK"
WARN = "WARN"
ERROR = "ERROR"


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
# チェック 1: §3.1 コマンド表 ↔ generate_handler!
# ---------------------------------------------------------------------------


def check_commands() -> Findings:
    f = Findings([])
    section_text = extract_section(read_text(DESIGN_MD), r"コマンド（フロント")
    if not section_text:
        f.error("design.md に §3.1（コマンド表）の見出しが見つからない")
        return f

    doc_entries: list[tuple[str, bool]] = []  # (名前, glob か)
    for row in table_rows(section_text):
        if not row:
            continue
        for name in names_in_cell(row[0]):
            doc_entries.append((name, "*" in name))

    lib = read_text(LIB_RS)
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

    if f.worst() == OK:
        f.ok(f"{len(code_names)} 件のコマンドが一致")
    return f


# ---------------------------------------------------------------------------
# チェック 2: §3.2 イベント表 ↔ emit / emit_to / listen のイベント名引数
# ---------------------------------------------------------------------------

EVENT_NAME_RE = r"[a-z][a-z0-9_]*://[a-z0-9_]+"
# listen<T>(...) のジェネリクスや改行を挟む呼び出しを拾うため、
# 関数名と `(` の間は括弧以外なら何でも許す。
# イベント名を取る関数: emit / emit_all / emit_filter / listen /
# listen_any / once は第 1 引数、emit_to は第 2 引数（第 1 引数は
# 送信先ラベル）。trigger は SQL の CREATE TRIGGER と衝突して
# 誤検出になるため対象外。
EVENT_CALL_RE = re.compile(
    r"\b(emit|emit_all|emit_filter|listen|listen_any|once|emit_to)\b[^(\n]{0,60}\("
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


def check_events() -> Findings:
    f = Findings([])
    section_text = extract_section(read_text(DESIGN_MD), r"イベント（Rust")
    if not section_text:
        f.error("design.md に §3.2（イベント表）の見出しが見つからない")
        return f

    doc_events: set[str] = set()
    for row in table_rows(section_text):
        if not row:
            continue
        doc_events.update(re.findall(EVENT_NAME_RE, row[0]))

    code_events: set[str] = set()
    dynamic_calls: list[str] = []
    targets = iter_files(SRC_TAURI, (".rs",)) + iter_files(
        SRC_FRONTEND, (".ts", ".svelte")
    )
    for path in targets:
        text = strip_comments(read_text(path), path.suffix)
        rust = path.suffix == ".rs"
        for m in EVENT_CALL_RE.finditer(text):
            # イベント名の位置は emit_to だけ第 2 引数
            idx = 1 if m.group(1) == "emit_to" else 0
            args = split_call_args(text, m.end() - 1, rust)
            if len(args) <= idx:
                continue
            event_arg = args[idx].strip()
            line_no = text.count("\n", 0, m.start()) + 1
            loc = f"{path.relative_to(ROOT)}:{line_no}"
            inner = single_string_literal(event_arg)
            if inner is None:
                dynamic_calls.append(loc)
                continue
            if re.fullmatch(EVENT_NAME_RE, inner):
                code_events.add(inner)
            else:
                # リテラルなのに `name://` 形式でない = §3.2 に載せられない命名
                dynamic_calls.append(loc)

    for name in sorted(code_events - doc_events):
        f.error(f"イベント `{name}` がコードにあるが §3.2 に無い")
    for name in sorted(doc_events - code_events):
        f.error(f"イベント `{name}` が §3.2 にあるがコードに無い")
    for loc in dynamic_calls:
        f.warn(f"{loc}: イベント名が `name://` 形式のリテラルでなく照合不能（イベント名はリテラルで書く）")

    if f.worst() == OK:
        f.ok(f"{len(code_events)} 件のイベントが一致")
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
    i18n_text = read_text(I18N_TS)
    m = re.search(r"const\s+ja\s*=\s*\{(.*?)\}\s*as\s+const", i18n_text, re.S)
    if not m:
        f.error("i18n.ts に `const ja` リソースが見つからない")
        return f
    keys = set(re.findall(r"^\s*\"([^\"]+)\"\s*:", m.group(1), re.M))
    if not keys:
        f.error("i18n.ts の ja リソースからキーを抽出できない")
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
        if path == I18N_TS:
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

    code_ids: dict[str, list[str]] = {}
    for path in iter_files(SRC_TAURI, (".rs",)):
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

CI_YML = ROOT / ".github" / "workflows" / "ci.yml"
GATE_JOB_ID = "ci-status"
GATE_JOB_NAME = "CI"


def check_ci_gate() -> Findings:
    f = Findings([])
    if not CI_YML.is_file():
        f.error(f"{CI_YML.relative_to(ROOT)} が見つからない")
        return f

    # jobs: 直下の 2 スペースインデントのキーをジョブ ID として拾う。
    # ジョブ属性（runs-on 等）は 4 スペース以上のため誤拾しない。
    job_ids: list[str] = []
    gate_needs: list[str] | None = None
    gate_name: str | None = None
    in_jobs = False
    in_gate = False
    for line in read_text(CI_YML).splitlines():
        if re.match(r"^jobs:\s*$", line):
            in_jobs = True
            continue
        if not in_jobs or re.match(r"^\s*#", line):
            continue
        if re.match(r"^\S", line):
            break  # トップレベルキーの復帰 = jobs 節の終わり
        m = re.match(r"^  ([A-Za-z0-9_-]+):\s*$", line)
        if m:
            in_gate = m.group(1) == GATE_JOB_ID
            job_ids.append(m.group(1))
            continue
        if in_gate:
            m = re.match(r"^    needs:\s*\[(.*)\]\s*$", line)
            if m:
                gate_needs = [
                    s.strip() for s in m.group(1).split(",") if s.strip()
                ]
                continue
            m = re.match(r"^    name:\s*(\S+)\s*$", line)
            if m:
                gate_name = m.group(1)

    if GATE_JOB_ID not in job_ids:
        f.error(f"ci.yml に集約ジョブ `{GATE_JOB_ID}` が無い")
        return f
    if gate_name != GATE_JOB_NAME:
        f.error(
            f"`{GATE_JOB_ID}` の name が `{gate_name}` になっている"
            f"（必須チェック名 `{GATE_JOB_NAME}` と一致させる契約）"
        )
    if gate_needs is None:
        f.error(f"`{GATE_JOB_ID}` の needs がインライン `[a, b]` 形式で読めない")
    else:
        expected = {j for j in job_ids if j != GATE_JOB_ID}
        for j in sorted(expected - set(gate_needs)):
            f.error(f"ジョブ `{j}` が `{GATE_JOB_ID}` の needs に無い")
        for j in sorted(set(gate_needs) - expected):
            f.error(f"`{GATE_JOB_ID}` の needs の `{j}` は存在しないジョブ")

    if f.worst() == OK:
        f.ok(f"{len(job_ids) - 1} 件のジョブが `{GATE_JOB_ID}` の needs に網羅されている")
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

    `|| AT |` のような先頭空セル行があるため、行内で `^[A-Z]+$` に
    一致する最初のセルを ID とする（ヘッダ行の `ID` セルは除く）。
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
    r"(?:describe|it|test)\s*\(\s*\"([A-Z]{2,3}-\d{2})(?=[\s\"])"
)
TS_TEST_TITLE_RE = re.compile(
    r"(?:describe|it|test)\s*\(\s*\"([^\"]+)\""
)
TS_TEST_INDEX_RE = re.compile(r"^\s*//\s*([A-Z]{2,3}-\d{2})\s*:", re.M)
TS_TEST_ID_TOKEN_RE = re.compile(r"[A-Z]{2,3}-\d{2}")
TS_TEST_ID_EXCLUDE_PREFIXES = {"FR", "BG", "INV", "DB"}


def iter_test_ts() -> list[Path]:
    """`*.test.ts` を返す（Path.suffix は `.ts` しか返さないため名前で判定）。"""
    return sorted(
        p for p in SRC_FRONTEND.rglob("*")
        if p.is_file() and p.name.endswith(".test.ts")
    )


def scan_ts_test_ids() -> tuple[dict[str, list[str]], dict[str, list[str]]]:
    """`.test.ts` の宣言 ID（describe 先頭）と索引 ID（冒頭コメント）を返す。"""
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

    for path in iter_test_ts():
        rel = str(path.relative_to(ROOT))
        text = read_text(path)
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

    for test_id, locs in sorted(index.items()):
        if test_id not in decl:
            f.warn(f"索引コメントの `{test_id}` に対応する describe/it が無い（{', '.join(locs)}）")
    for test_id, locs in sorted(decl.items()):
        if test_id not in index:
            f.warn(f"`{test_id}` の describe 宣言に対応する索引コメントが無い（{', '.join(locs)}）")

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
        f.ok(f"{len(decl)} 件の .test.ts テスト ID が規約通り")
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


def check_extension_constants() -> Findings:
    f = Findings([])
    conf = json.loads(read_text(TAURI_CONF))
    identifier = conf.get("identifier")
    if not identifier:
        f.error("tauri.conf.json の identifier を読めない")
        return f
    host_name_expected = identifier.replace("-", "_")

    native_host = read_text(ROOT / "src-tauri" / "src" / "native_host.rs")
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
    elif m.group(1) not in read_text(DECISIONS_MD):
        f.error(
            f"EXTENSION_ID `{m.group(1)}` が decision-records.md に記載されていない"
        )

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
