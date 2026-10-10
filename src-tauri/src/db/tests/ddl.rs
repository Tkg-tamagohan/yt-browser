use crate::db::*;

// ---------------------------------------------------------------------------
// docs↔コード整合の機械チェック: §8 DDL と適用後スキーマの照合
// （Python 側は scripts/check_docs_consistency.py、計画書 CL-1a）。
// ---------------------------------------------------------------------------

/// `--` 行コメントを文字列リテラル外だけ取り除く。
fn strip_sql_comments(sql: &str) -> String {
    let mut out = String::with_capacity(sql.len());
    let mut chars = sql.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\'' {
            out.push(c);
            while let Some(inner) = chars.next() {
                out.push(inner);
                if inner == '\'' {
                    // '' はリテラル内のエスケープなので読み続ける
                    if chars.peek() == Some(&'\'') {
                        out.push(chars.next().unwrap());
                        continue;
                    }
                    break;
                }
            }
            continue;
        }
        if c == '-' && chars.peek() == Some(&'-') {
            for inner in chars.by_ref() {
                if inner == '\n' {
                    out.push(inner);
                    break;
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

/// 比較用の正規化: コメント除去、引用符外の小文字化、空白畳み込み、
/// `if not exists` 除去、`()` `,` `;` 前後の空白除去、末尾 `;` 除去。
fn normalize_sql(sql: &str) -> String {
    let stripped = strip_sql_comments(sql);
    let mut out = String::with_capacity(stripped.len());
    let mut prev_ws = false;
    let mut chars = stripped.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\'' {
            // 文字列リテラルは大小・空白ともそのまま残す
            out.push(c);
            while let Some(inner) = chars.next() {
                out.push(inner);
                if inner == '\'' {
                    if chars.peek() == Some(&'\'') {
                        out.push(chars.next().unwrap());
                        continue;
                    }
                    break;
                }
            }
            prev_ws = false;
            continue;
        }
        if c.is_whitespace() {
            prev_ws = true;
            continue;
        }
        if prev_ws {
            // 開き `(` の直後と `()` `,` `;` の直前の空白は出さない
            if !"(),;".contains(c) && !out.ends_with('(') {
                out.push(' ');
            }
            prev_ws = false;
        }
        out.extend(c.to_lowercase());
    }
    // sqlite は格納時に if not exists を落とすが、文書側に書かれた場合にも耐える
    let out = out.replace("if not exists ", "");
    out.trim_end_matches(';').trim_end().to_string()
}

/// 正規化済み SQL の `(...)` 本体を最上位カンマで分割する。
/// `PRIMARY KEY (a, b)` のような括弧内と '...' 内のカンマは区切りにしない。
fn split_top_level_commas(body: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut cur = String::new();
    let mut depth = 0i32;
    let mut in_quote = false;
    let mut chars = body.chars().peekable();
    while let Some(c) = chars.next() {
        if in_quote {
            cur.push(c);
            if c == '\'' {
                if chars.peek() == Some(&'\'') {
                    cur.push(chars.next().unwrap());
                    continue;
                }
                in_quote = false;
            }
            continue;
        }
        match c {
            '\'' => {
                in_quote = true;
                cur.push(c);
            }
            '(' => {
                depth += 1;
                cur.push(c);
            }
            ')' => {
                depth -= 1;
                cur.push(c);
            }
            ',' if depth == 0 => {
                items.push(cur.trim().to_string());
                cur.clear();
            }
            _ => cur.push(c),
        }
    }
    if !cur.trim().is_empty() {
        items.push(cur.trim().to_string());
    }
    items
}

/// オブジェクト単位の比較用文字列に変換する。
/// `CREATE [VIRTUAL] TABLE` はヘッダ＋列定義の集合として比較し、
/// それ以外（INDEX / TRIGGER など）は正規化文全体を比較する。
fn normalize_for_compare(sql: &str) -> String {
    let n = normalize_sql(sql);
    if n.starts_with("create table ") || n.starts_with("create virtual table ") {
        if let Some(open) = n.find('(') {
            let head = n[..open].trim_end();
            let close = n.rfind(')').unwrap_or(n.len());
            let mut items = split_top_level_commas(&n[open + 1..close]);
            items.sort();
            return format!("{head}({})", items.join(","));
        }
    }
    n
}

/// CREATE 文からオブジェクト名を取る。
fn ddl_object_name(stmt: &str) -> Option<String> {
    let re = regex::Regex::new(
        r#"(?i)^\s*create\s+(?:virtual\s+)?(?:table|index|unique\s+index|trigger|view)\s+(?:if\s+not\s+exists\s+)?[\"'`]?(\w+)"#,
    )
    .unwrap();
    re.captures(stmt).map(|c| c[1].to_string())
}

/// 文書側の SQL 断片から CREATE 文を (名前, 文) で切り出す。
/// トリガーの BEGIN...END 内の `;` は文の区切りにしない。
fn create_statements(sql: &str) -> Vec<(String, String)> {
    // トークン化（空白で区切られる語、'...' リテラル、() ; , の区切り文字）
    let mut spans: Vec<(usize, usize)> = Vec::new();
    let mut chars = sql.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if c.is_whitespace() {
            continue;
        }
        if c == '\'' {
            let mut end = i + c.len_utf8();
            while let Some((j, d)) = chars.next() {
                end = j + d.len_utf8();
                if d == '\'' {
                    if chars.peek().map(|&(_, p)| p) == Some('\'') {
                        let (j2, d2) = chars.next().unwrap();
                        end = j2 + d2.len_utf8();
                        continue;
                    }
                    break;
                }
            }
            spans.push((i, end));
            continue;
        }
        if "();,".contains(c) {
            spans.push((i, i + c.len_utf8()));
            continue;
        }
        let mut end = i + c.len_utf8();
        while let Some(&(j, d)) = chars.peek() {
            if d.is_whitespace() || "();,'".contains(d) {
                break;
            }
            chars.next();
            end = j + d.len_utf8();
        }
        spans.push((i, end));
    }

    let mut out = Vec::new();
    let mut i = 0;
    while i < spans.len() {
        let (s, e) = spans[i];
        if !sql[s..e].eq_ignore_ascii_case("create") {
            i += 1;
            continue;
        }
        // `;`（括弧・BEGIN...END の外側に限る）までを 1 文とする
        let mut depth = 0i32;
        let mut begin_depth = 0i32;
        let mut end = e;
        let mut j = i;
        while j < spans.len() {
            let (s2, e2) = spans[j];
            end = e2;
            let t = &sql[s2..e2];
            if t == "(" {
                depth += 1;
            } else if t == ")" {
                depth -= 1;
            } else if depth == 0 && t.eq_ignore_ascii_case("begin") {
                begin_depth += 1;
            } else if depth == 0 && t.eq_ignore_ascii_case("end") {
                begin_depth -= 1;
            } else if t == ";" && depth == 0 && begin_depth <= 0 {
                break;
            }
            j += 1;
        }
        if let Some(name) = ddl_object_name(&sql[s..end]) {
            out.push((name, sql[s..end].to_string()));
        }
        i = j + 1;
    }
    out
}

/// design.md の「データベース設計」節にある ```sql ブロックを連結して返す。
fn design_ddl(design: &str) -> String {
    let lines: Vec<&str> = design.lines().collect();
    let mut start = None;
    let mut level = 0usize;
    for (i, l) in lines.iter().enumerate() {
        let heading = l.trim_start_matches('#');
        if l.starts_with('#') && heading.starts_with(' ') {
            let lv = l.len() - heading.len();
            if let Some(s) = start {
                if lv <= level {
                    start = Some(s);
                    level = lv;
                    let _ = i;
                    break;
                }
            } else if heading[1..].contains("データベース設計") {
                start = Some(i);
                level = lv;
            }
        }
    }
    let start = start.expect("design.md に「データベース設計」節が無い");
    let mut end = lines.len();
    for (i, l) in lines.iter().enumerate().skip(start + 1) {
        if l.starts_with('#') {
            let heading = l.trim_start_matches('#');
            let lv = l.len() - heading.len();
            if lv <= level {
                end = i;
                break;
            }
        }
    }
    let mut ddl = String::new();
    let mut in_sql = false;
    for l in &lines[start..end] {
        let t = l.trim();
        if !in_sql && t.starts_with("```") && t.contains("sql") {
            in_sql = true;
            continue;
        }
        if in_sql && t.starts_with("```") {
            in_sql = false;
            continue;
        }
        if in_sql {
            ddl.push_str(l);
            ddl.push('\n');
        }
    }
    ddl
}

/// 設計書 §8 の DDL と、マイグレーション適用後の sqlite_master を照合する
/// （監査対応の機械チェック: scripts/check_docs_consistency.py と対）。
/// `connect_in_memory` は `connect` と同じ migrate 経路を通るため、
/// schema_migrations のブートストラップや ALTER による文の書き換えも
/// そのまま拾える。
/// 文書側は列を意味のまとまりで書くことがあるため（ALTER 追加列を
/// 末尾に置く sqlite の書き換えと常に一致するとは限らない）、
/// CREATE TABLE は列の集合として比較する。
#[test]
fn ddl_matches_design_section8() {
    let db = Db::connect_in_memory().unwrap();
    let conn = db.lock().unwrap();
    let mut stmt = conn
        .prepare("SELECT type, name, sql FROM sqlite_master WHERE sql IS NOT NULL")
        .unwrap();
    let rows: Vec<(String, String, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();

    // FTS5 のシャドウテーブル（<ft>_data 等）と sqlite_ 内部オブジェクトは
    // SQLite の内部生成物なので照合対象から除く
    let virtual_tables: Vec<String> = rows
        .iter()
        .filter(|(ty, _, sql)| ty == "table" && sql.to_lowercase().contains("using fts"))
        .map(|(_, name, _)| name.clone())
        .collect();
    let mut actual: std::collections::BTreeMap<String, String> = std::collections::BTreeMap::new();
    for (_, name, sql) in &rows {
        if name.starts_with("sqlite_") {
            continue;
        }
        if virtual_tables
            .iter()
            .any(|vt| name.starts_with(&format!("{vt}_")))
        {
            continue;
        }
        actual.insert(name.clone(), normalize_for_compare(sql));
    }

    let design = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs/design.md"),
    )
    .expect("docs/design.md が読めない");
    let mut expected: std::collections::BTreeMap<String, String> =
        std::collections::BTreeMap::new();
    for (name, stmt_text) in create_statements(&design_ddl(&design)) {
        expected.insert(name, normalize_for_compare(&stmt_text));
    }

    let mut diffs: Vec<String> = Vec::new();
    for (name, actual_sql) in &actual {
        match expected.get(name) {
            None => diffs.push(format!("{name}: 実スキーマにあって §8 に無い")),
            Some(expected_sql) if expected_sql != actual_sql => diffs.push(format!(
                "{name}: §8 と実スキーマで文が違う\n  doc: {expected_sql}\n  sql: {actual_sql}"
            )),
            _ => {}
        }
    }
    for name in expected.keys() {
        if !actual.contains_key(name) {
            diffs.push(format!("{name}: §8 にあって実スキーマに無い"));
        }
    }
    assert!(
        diffs.is_empty(),
        "§8 DDL と適用後スキーマのずれ:\n{}",
        diffs.join("\n")
    );
}
