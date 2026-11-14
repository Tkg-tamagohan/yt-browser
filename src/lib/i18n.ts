// i18n 基盤（要件定義 §5、FR-81、仕様決定 DF）: UI 文字列をメッセージキーで
// 管理し、言語リソースを差し替え可能にする。
// リソースは ja（正・全キー網羅）と en（英語）。en に無いキーは ja へ
// フォールバックする。現行ロケールは i18n-locale.svelte.ts の $state が
// 保持し、ui-prefs.ts が設定 ui.language（ja/en、既定 ja）を反映する。

import { i18nLocale } from "./i18n-locale.svelte";
import { ja, type MessageKey } from "./i18n-ja";
import { en } from "./i18n-en";

export type { MessageKey };

export const LOCALES = ["ja", "en"] as const;
export type Locale = (typeof LOCALES)[number];

const MESSAGES: Record<Locale, Partial<Record<MessageKey, string>>> = {
  ja,
  en,
};

export function setLocale(locale: Locale): void {
  i18nLocale.current = locale;
}

/// 現行ロケール。フォーマッタなど表示系の言語分岐に使う
export function currentLocale(): Locale {
  return i18nLocale.current;
}

type Params = Record<string, string | number>;

/// 指定ロケールでメッセージキーを解決する純粋関数（t() の解決実体）。
/// locale に無いキーは ja へフォールバックし、それでも無ければキー名を返す。
/// 網羅・フォールバックの検証（LN-NN 系テスト）から直接使える
export function resolveMessage(locale: Locale, key: MessageKey): string {
  return MESSAGES[locale][key] ?? MESSAGES.ja[key] ?? key;
}

/// メッセージキーから表示文字列を得る。`{name}` 形式のプレースホルダーを params で埋める。
/// 現行ロケールに無いキーは ja へフォールバックし、それでも無ければキー名を返す。
/// i18nLocale.current は $state のため、テンプレート内の呼び出しは
/// ロケール変更で即時に再評価される
export function t(key: MessageKey, params?: Params): string {
  const template = resolveMessage(i18nLocale.current, key);
  if (!params) return template;
  return template.replace(/\{(\w+)\}/g, (_, name: string) =>
    String(params[name] ?? `{${name}}`),
  );
}

/// 登録されている全メッセージキー（ja が正）。
/// リソース網羅の検証（LN-NN 系テスト）に使う
export function messageKeys(): MessageKey[] {
  return Object.keys(MESSAGES.ja) as MessageKey[];
}

/// 指定ロケールで未登録（ja へフォールバックする）キーの一覧。
/// リソース網羅の検証（LN-NN 系テスト）に使う
export function fallbackKeys(locale: Locale): MessageKey[] {
  return messageKeys().filter((k) => MESSAGES[locale][k] === undefined);
}

// トーンマッピング選択肢 ID → mpv の --tone-mapping 値。
// ドット入り値（bt.2390 / bt.2446a）を持つエントリだけを置く。
// check_docs_consistency の i18n 参照検査はこのファイルを対象外とするため、
// ドット入りリテラルはここに置き、選択肢側はドット無し ID を使う
// （src/lib/settings-consts.ts の TONE_MAPPINGS を参照）
export const TONE_MAPPING_MPV: Record<string, string> = {
  bt2390: "bt.2390",
  bt2446a: "bt.2446a",
};
