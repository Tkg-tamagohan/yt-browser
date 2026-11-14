//! 表示設定（`ui.*` キー）の解釈の正（FR-76・FR-77、仕様決定 DA・DB）。
//!
//! 配色と一覧密度はフロント側（`src/lib/ui-prefs.ts` が `<html>` の
//! `data-theme` / `data-density` 属性へ反映し、`src/app.css` の CSS 変数が
//! 実値を決める）だけで使う設定で、バックエンドでは参照しない。
//! `chat/popup.rs` と `chat-popup-logic.ts` の関係と同じく、解釈の正をここに
//! 置き、フロント側の `ui-logic.ts` が同一解釈を持つ（テストで検証）。

/// 設定キー: 配色テーマ（FR-76、仕様決定 DA）。
/// 値は "dark" / "light" / "system"。未設定・その他の値は "dark" として扱う。
/// フロントが settings_get で読む表示側だけの設定でバックエンドでは
/// 参照しないが、設定キー表との両方向照合（check_docs_consistency）の
/// ため定数として保持する。
#[allow(dead_code)]
pub(crate) const SETTING_UI_THEME: &str = "ui.theme";

/// 設定キー: 一覧の表示密度（FR-77、仕様決定 DB）。
/// 値は "comfortable" / "compact"。未設定・その他の値は "comfortable" と
/// して扱う（理由は上と同じ）。
#[allow(dead_code)]
pub(crate) const SETTING_UI_DENSITY: &str = "ui.density";

/// `ui.theme` の保存値をテーマ名に解釈する（FR-76、仕様決定 DA）。
/// "light" / "system" だけを受理し、未設定・その他の値は既定の "dark" へ戻す。
/// 受理集合は大小文字と前後空白を許容する（`live_wait.action` と同じ形）。
#[allow(dead_code)]
pub(crate) fn ui_theme(value: Option<String>) -> &'static str {
    match value {
        Some(v) => match v.trim().to_ascii_lowercase().as_str() {
            "light" => "light",
            "system" => "system",
            _ => "dark",
        },
        None => "dark",
    }
}

/// `ui.theme` の解釈結果を実際に適用する配色へ解決する（FR-76、仕様決定 DA）。
/// "system" は OS の `prefers-color-scheme` に従う。
#[allow(dead_code)]
pub(crate) fn ui_theme_resolved(theme: &str, prefers_light: bool) -> &'static str {
    match theme {
        "light" => "light",
        "system" if prefers_light => "light",
        _ => "dark",
    }
}

/// `ui.density` の保存値を密度名に解釈する（FR-77、仕様決定 DB）。
/// "compact" だけを受理し、未設定・その他の値は既定の "comfortable" へ戻す。
#[allow(dead_code)]
pub(crate) fn ui_density(value: Option<String>) -> &'static str {
    match value {
        Some(v) if v.trim().eq_ignore_ascii_case("compact") => "compact",
        _ => "comfortable",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// DB-UI-01: `ui.theme` の解釈（FR-76、仕様決定 DA）。
    /// light / system だけを受理し、未設定・その他は既定の dark へ戻す。
    #[test]
    fn ui_theme_values() {
        for (v, expected) in [
            ("dark", "dark"),
            ("light", "light"),
            ("system", "system"),
            (" Light ", "light"),
            ("SYSTEM", "system"),
            ("", "dark"),
            ("  ", "dark"),
            ("blue", "dark"),
            ("off", "dark"),
        ] {
            assert_eq!(
                ui_theme(Some(v.to_string())),
                expected,
                "{v:?} は {expected} になるべき"
            );
        }
        assert_eq!(ui_theme(None), "dark");
    }

    /// DB-UI-02: `ui.density` の解釈（FR-77、仕様決定 DB）。
    /// compact だけを受理し、未設定・その他は既定の comfortable へ戻す。
    #[test]
    fn ui_density_values() {
        assert_eq!(ui_density(Some("compact".to_string())), "compact");
        assert_eq!(ui_density(Some(" Compact ".to_string())), "compact");
        for (v, expected) in [
            ("comfortable", "comfortable"),
            ("", "comfortable"),
            ("  ", "comfortable"),
            ("dense", "comfortable"),
            ("wide", "comfortable"),
        ] {
            assert_eq!(
                ui_density(Some(v.to_string())),
                expected,
                "{v:?} は {expected} になるべき"
            );
        }
        assert_eq!(ui_density(None), "comfortable");
    }

    /// DB-UI-03: `ui.theme` の system 解決（FR-76、仕様決定 DA）。
    /// system は prefers-color-scheme へ従い、明示値はそのまま返す。
    #[test]
    fn ui_theme_resolved_values() {
        assert_eq!(ui_theme_resolved("dark", true), "dark");
        assert_eq!(ui_theme_resolved("dark", false), "dark");
        assert_eq!(ui_theme_resolved("light", true), "light");
        assert_eq!(ui_theme_resolved("light", false), "light");
        assert_eq!(ui_theme_resolved("system", true), "light");
        assert_eq!(ui_theme_resolved("system", false), "dark");
    }
}
