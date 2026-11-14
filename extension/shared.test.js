// 拡張側の URL・位置ロジックの回帰テスト（FR-51、仕様決定 CB）
// CB-01: withTimeParam — t= の付与・既存 t= の上書き・他クエリの温存
// CB-02: hasUsablePosition — t= を付ける意味がある再生位置の判定
import { describe, expect, test } from "vitest";
import { hasUsablePosition, withTimeParam } from "./shared.js";

const WATCH = "https://www.youtube.com/watch?v=dQw4w9WgXcQ";

describe("CB-01 withTimeParam", () => {
  test("watch URL に t=<秒> を付ける", () => {
    expect(withTimeParam(WATCH, 2630)).toBe(`${WATCH}&t=2630`);
  });

  test("小数秒は整数へ切り捨てる", () => {
    expect(withTimeParam(WATCH, 12.9)).toBe(`${WATCH}&t=12`);
  });

  test("既存の t= は上書きし、list= など他のクエリは温存する", () => {
    const withList = `${WATCH}&list=PLtest12345&t=10`;
    expect(withTimeParam(withList, 42)).toBe(`${WATCH}&list=PLtest12345&t=42`);
  });

  test("youtu.be の t= も書き換える", () => {
    expect(withTimeParam("https://youtu.be/dQw4w9WgXcQ?t=1h2m3s", 90)).toBe(
      "https://youtu.be/dQw4w9WgXcQ?t=90",
    );
  });
});

describe("CB-02 hasUsablePosition", () => {
  test("1 秒以上の有限数だけを受理する", () => {
    expect(hasUsablePosition(90)).toBe(true);
    expect(hasUsablePosition(1)).toBe(true);
    expect(hasUsablePosition(0.5)).toBe(false);
    expect(hasUsablePosition(0)).toBe(false);
    expect(hasUsablePosition(-3)).toBe(false);
  });

  test("非数・非数値は受理しない", () => {
    expect(hasUsablePosition(Number.NaN)).toBe(false);
    expect(hasUsablePosition(Number.POSITIVE_INFINITY)).toBe(false);
    expect(hasUsablePosition(null)).toBe(false);
    expect(hasUsablePosition(undefined)).toBe(false);
    expect(hasUsablePosition("90")).toBe(false);
  });
});
