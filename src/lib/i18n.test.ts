// i18n のロケール解決とリソース網羅のテスト（FR-81、仕様決定 DF）。
// LN-01: en リソースが ja の全キーを網羅する（フォールバック対象なし）
// LN-02: resolveMessage のロケール別解決・ja フォールバック・未知キー
// LN-03: setLocale / currentLocale の反映と t() のプレースホルダー置換
import { describe, expect, test } from "vitest";
import {
  currentLocale,
  fallbackKeys,
  LOCALES,
  messageKeys,
  resolveMessage,
  setLocale,
  t,
  type MessageKey,
} from "./i18n";

describe("LN-01 en リソースの網羅", () => {
  test("en は ja の全キーを持つ（フォールバック対象なし）", () => {
    expect(fallbackKeys("en")).toEqual([]);
  });

  test("全キーが en で文言へ解決される", () => {
    // 解決値が空でなくキー名そのまま（未解決）でないことを確認する
    expect(messageKeys().length).toBeGreaterThan(0);
    for (const key of messageKeys()) {
      const resolved = resolveMessage("en", key);
      expect(resolved, `${key} の en 文言`).not.toBe(key);
      expect(resolved.trim(), `${key} の en 文言`).not.toBe("");
    }
  });
});

describe("LN-02 resolveMessage の解決", () => {
  test("ロケールごとにその言語の文言を返す", () => {
    expect(resolveMessage("ja", "nav.feed")).toBe("フィード");
    expect(resolveMessage("en", "nav.feed")).toBe("Feed");
    expect(resolveMessage("ja", "settings.ui.language")).toBe("UI 言語");
    expect(resolveMessage("en", "settings.ui.language")).toBe("UI language");
  });

  test("どちらの言語にも無いキーはキー名をそのまま返す", () => {
    // MessageKey 型外の未知キーはキャストで注入する
    const bogus = "nonexistent" as MessageKey;
    expect(resolveMessage("en", bogus)).toBe("nonexistent");
    expect(resolveMessage("ja", bogus)).toBe("nonexistent");
  });
});

describe("LN-03 setLocale と t()", () => {
  test("setLocale が現行ロケールと t() の解決を切り替える", () => {
    try {
      expect(currentLocale()).toBe("ja");
      expect(t("nav.feed")).toBe("フィード");
      setLocale("en");
      expect(currentLocale()).toBe("en");
      expect(t("nav.feed")).toBe("Feed");
    } finally {
      setLocale("ja");
    }
  });

  test("プレースホルダーはどちらの言語でも置換する", () => {
    try {
      setLocale("en");
      expect(t("player.ended", { reason: "eof" })).toBe(
        "Playback ended (eof)",
      );
      setLocale("ja");
      expect(t("player.ended", { reason: "eof" })).toBe(
        "再生が終了しました（eof）",
      );
    } finally {
      setLocale("ja");
    }
  });

  test("LOCALES は ja / en を持つ", () => {
    expect(LOCALES).toContain("ja");
    expect(LOCALES).toContain("en");
  });
});
