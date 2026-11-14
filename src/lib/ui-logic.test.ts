// 表示設定（ui.*）の解釈テスト（FR-76・FR-77・FR-81、仕様決定 DA・DB・DF）。
// UI-01: uiTheme の受理集合（light / system のみ、既定は dark）
// UI-02: uiDensity の受理集合（compact のみ、既定は comfortable）
// UI-03: resolveTheme の system 解決（prefers-color-scheme 追従）
// UI-04: uiLanguage の受理集合（en のみ、既定は ja）
import { describe, expect, test } from "vitest";
import {
  resolveTheme,
  uiDensity,
  uiLanguage,
  uiTheme,
} from "./ui-logic";

describe("UI-01 uiTheme の受理集合", () => {
  test("light / system / dark を受理する", () => {
    expect(uiTheme("dark")).toBe("dark");
    expect(uiTheme("light")).toBe("light");
    expect(uiTheme("system")).toBe("system");
    // 大小文字と前後空白は許容する
    expect(uiTheme(" Light ")).toBe("light");
    expect(uiTheme("SYSTEM")).toBe("system");
  });

  test("未設定・その他は既定の dark へ戻す", () => {
    for (const v of ["", "  ", "blue", "off", "dark-mode"]) {
      expect(uiTheme(v), `${v} は dark になるべき`).toBe("dark");
    }
    expect(uiTheme(null)).toBe("dark");
  });
});

describe("UI-02 uiDensity の受理集合", () => {
  test("compact を受理する", () => {
    expect(uiDensity("compact")).toBe("compact");
    expect(uiDensity(" Compact ")).toBe("compact");
  });

  test("未設定・その他は既定の comfortable へ戻す", () => {
    for (const v of ["comfortable", "", "  ", "dense", "wide"]) {
      expect(uiDensity(v), `${v} は comfortable になるべき`).toBe(
        "comfortable",
      );
    }
    expect(uiDensity(null)).toBe("comfortable");
  });
});

describe("UI-03 resolveTheme の system 解決", () => {
  test("明示値は prefers に関わらずそのまま返す", () => {
    expect(resolveTheme("dark", true)).toBe("dark");
    expect(resolveTheme("dark", false)).toBe("dark");
    expect(resolveTheme("light", true)).toBe("light");
    expect(resolveTheme("light", false)).toBe("light");
  });

  test("system は prefers-color-scheme に従う", () => {
    expect(resolveTheme("system", true)).toBe("light");
    expect(resolveTheme("system", false)).toBe("dark");
  });
});

describe("UI-04 uiLanguage の受理集合", () => {
  test("en / ja を受理する", () => {
    expect(uiLanguage("en")).toBe("en");
    expect(uiLanguage("ja")).toBe("ja");
    // 大小文字と前後空白は許容する
    expect(uiLanguage(" EN ")).toBe("en");
    expect(uiLanguage("En")).toBe("en");
  });

  test("未設定・その他は既定の ja へ戻す", () => {
    for (const v of ["", "  ", "english", "en-US", "fr", "off"]) {
      expect(uiLanguage(v), `${v} は ja になるべき`).toBe("ja");
    }
    expect(uiLanguage(null)).toBe("ja");
  });
});
