// format.ts の表示用フォーマッタテスト（FR-96、仕様決定 EB）。
// FMT-01: fmtBroadcastDateTime の ja/en・当年/異年・不正値フォールバック
import { describe, expect, test } from "vitest";
import { fmtBroadcastDateTime } from "./format";
import { setLocale } from "./i18n";

describe("FMT-01: fmtBroadcastDateTime", () => {
  test("null / undefined / 0 以下 / 非有限値は空文字を返す", () => {
    const now = new Date(2026, 10, 27, 12, 0);
    expect(fmtBroadcastDateTime(null, now)).toBe("");
    expect(fmtBroadcastDateTime(undefined, now)).toBe("");
    expect(fmtBroadcastDateTime(0, now)).toBe("");
    expect(fmtBroadcastDateTime(-1000, now)).toBe("");
    expect(fmtBroadcastDateTime(Number.NaN, now)).toBe("");
    expect(fmtBroadcastDateTime(Number.POSITIVE_INFINITY, now)).toBe("");
  });

  test("ja ロケールで当年は年なし、異年は年付きで整形する", () => {
    try {
      setLocale("ja");
      const now = new Date(2026, 10, 27, 12, 0);
      const sameYear = new Date(2026, 10, 27, 19, 5).getTime();
      const diffYear = new Date(2027, 0, 2, 9, 30).getTime();

      const sameStr = fmtBroadcastDateTime(sameYear, now);
      expect(sameStr).toContain("11/27");
      expect(sameStr).toContain("19:05");
      expect(sameStr).not.toContain("2026");

      const diffStr = fmtBroadcastDateTime(diffYear, now);
      expect(diffStr).toContain("2027");
      expect(diffStr).toContain("1/2");
      expect(diffStr).toContain("09:30");
    } finally {
      setLocale("ja");
    }
  });

  test("en ロケールでも当年は年なし、異年は年付きで整形する", () => {
    try {
      setLocale("en");
      const now = new Date(2026, 10, 27, 12, 0);
      const sameYear = new Date(2026, 10, 27, 19, 5).getTime();
      const diffYear = new Date(2027, 0, 2, 9, 30).getTime();

      const sameStr = fmtBroadcastDateTime(sameYear, now);
      expect(sameStr).toContain("11/27");
      expect(sameStr).not.toContain("2026");

      const diffStr = fmtBroadcastDateTime(diffYear, now);
      expect(diffStr).toContain("2027");
    } finally {
      setLocale("ja");
    }
  });
});
