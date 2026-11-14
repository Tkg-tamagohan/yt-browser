// 表示設定（ui.* キー）の解釈（FR-76・FR-77・FR-81、仕様決定 DA・DB・DF）。
// Tauri API へ触れない純粋関数だけを置き、vitest から直接検証できるようにする。
// 解釈の正は Rust 側（src-tauri/src/ui.rs の `ui_theme` / `ui_theme_resolved` /
// `ui_density` / `ui_language`）にあり、ここにあるのは UI 側の同一解釈
// （chat-popup-logic.ts と chat/popup.rs の関係と同じ型）。

/// `ui.theme` が取り得るテーマ名。`dark` / `light` / `system`。
export type UiTheme = "dark" | "light" | "system";

/// `ui.density` が取り得る密度名。`comfortable` / `compact`。
export type UiDensity = "comfortable" | "compact";

/// `ui.language` が取り得る UI 言語。`ja` / `en`。
export type UiLanguage = "ja" | "en";

/// 実際に適用する配色（system 解決後）。`dark` / `light`。
export type ResolvedTheme = "dark" | "light";

/// `ui.theme` の保存値をテーマ名に解釈する（FR-76、仕様決定 DA）。
/// `light` / `system` だけを受理し、未設定・その他の値は既定の `dark` へ戻す。
/// 受理集合は大小文字と前後空白を許容する。
/// Rust 側 `ui_theme`（src-tauri/src/ui.rs）と同一解釈。
export function uiTheme(raw: string | null): UiTheme {
  const v = raw?.trim().toLowerCase();
  if (v === "light" || v === "system") return v;
  return "dark";
}

/// `ui.theme` の解釈結果を実際に適用する配色へ解決する（FR-76、仕様決定 DA）。
/// `system` は OS の `prefers-color-scheme`（`prefersLight`）に従う。
/// Rust 側 `ui_theme_resolved`（src-tauri/src/ui.rs）と同一解釈。
export function resolveTheme(
  theme: UiTheme,
  prefersLight: boolean,
): ResolvedTheme {
  if (theme === "light") return "light";
  if (theme === "system" && prefersLight) return "light";
  return "dark";
}

/// `ui.density` の保存値を密度名に解釈する（FR-77、仕様決定 DB）。
/// `compact` だけを受理し、未設定・その他の値は既定の `comfortable` へ戻す。
/// Rust 側 `ui_density`（src-tauri/src/ui.rs）と同一解釈。
export function uiDensity(raw: string | null): UiDensity {
  return raw?.trim().toLowerCase() === "compact" ? "compact" : "comfortable";
}

/// `ui.language` の保存値を UI 言語に解釈する（FR-81、仕様決定 DF）。
/// `en` だけを受理し、未設定・その他の値は既定の `ja` へ戻す。
/// 受理は大小文字と前後空白を許容する。
/// Rust 側 `ui_language`（src-tauri/src/ui.rs）と同一解釈。
export function uiLanguage(raw: string | null): UiLanguage {
  return raw?.trim().toLowerCase() === "en" ? "en" : "ja";
}
