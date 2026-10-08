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
チェック 3（§8 DDL と適用後スキーマの照合）は実マイグレーション経路を
通す必要があるため Rust 側テストとして src-tauri/src/db/tests.rs にある。

見出しへの anchor は節番号ではなく見出し語で行い、節の採番が変わっても
追従できるようにする。表の第 1 セルからバッククォート内の名前を取る規約は
§3.1 / §3.2 / 設定キー表で共通とする。
"""

from __future__ import annotations

import fnmatch
import re
import sys
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DESIGN_MD = ROOT / "docs" / "design.md"
LIB_RS = ROOT / "src-tauri" / "src" / "lib.rs"
I18N_TS = ROOT / "src" / "lib" / "i18n.ts"
SRC_TAURI = ROOT / "src-tauri" / "src"
SRC_FRONTEND = ROOT / "src"
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
    "sponsor.cat.": ("src/routes/settings/+page.svelte", "SPONSOR_CATEGORIES"),
    "filter.target.": ("src/routes/settings/+page.svelte", "FILTER_TARGETS"),
    "filter.kind.": ("src/routes/settings/+page.svelte", "FILTER_KINDS"),
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
    for path in iter_files(SRC_FRONTEND, (".ts", ".svelte")):
        if path == I18N_TS:
            continue
        text = strip_comments(read_text(path), path.suffix)
        for m2 in DOTTED_KEY_RE.finditer(text):
            lit = m2.group(1)
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

TEST_ID_RE = re.compile(r"\b(DB-[A-Z]+-\d+)\b")
TEST_ID_DOC_COMMENT_RE = re.compile(r"^\s*///.*?\b(DB-[A-Z]+-\d+)\b")
TEST_ATTR_RE = re.compile(r"^\s*#\[(?:tokio::)?test\]")


def check_test_ids() -> Findings:
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

    for doc in iter_files(DOCS_DIR, (".md",)):
        for test_id in sorted(set(TEST_ID_RE.findall(read_text(doc)))):
            if test_id not in code_ids:
                f.error(
                    f"テスト ID `{test_id}` が {doc.name} で引用されているが"
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


def main() -> int:
    checks: list[tuple[str, Findings]] = []
    setting_keys = set(extract_setting_keys())

    checks.append(("check 1: コマンド表", check_commands()))
    checks.append(("check 2: イベント表", check_events()))
    checks.append(("check 4: i18n キー", check_i18n(setting_keys)))
    checks.append(("check 5: テスト ID", check_test_ids()))
    checks.append(("check 6: 設定キー表", check_settings(setting_keys)))
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
