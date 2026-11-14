// 現行の UI 言語を保持するリアクティブ状態（FR-81、仕様決定 DF）。
// i18n.ts の t() がこの値を読むため、ui-prefs.ts が settings://changed で
// 更新した時点で t() を参照する画面が即時に再評価される。
// Tauri API へ触れないよう分離し、vitest からも import できる形にする。

import type { Locale } from "./i18n";

/// 現行ロケール。既定は ja（未設定・その他の値は ui_language の解釈で ja へ戻す）
export const i18nLocale = $state<{ current: Locale }>({ current: "ja" });
