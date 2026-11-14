// 診断情報エクスポートの UI 側ロジックの回帰テスト（FR-82、仕様決定 DG）。
// DX-01: diagFileName の命名規則（日時入り・zip 拡張子・0 埋め）
import { describe, expect, test } from "vitest";
import { diagFileName } from "./diag-logic";

describe("DX-01 diagFileName の命名", () => {
  test("yt-browser-diag-<yyyymmdd>-<hhmmss>.zip 形式になる", () => {
    const d = new Date(2026, 1, 3, 4, 5, 6); // 2026-02-03 04:05:06
    expect(diagFileName(d)).toBe("yt-browser-diag-20260203-040506.zip");
  });

  test("一桁の月日・時刻は 0 埋めされる", () => {
    const d = new Date(2026, 0, 9, 1, 2, 3); // 2026-01-09 01:02:03
    expect(diagFileName(d)).toBe("yt-browser-diag-20260109-010203.zip");
  });
});
