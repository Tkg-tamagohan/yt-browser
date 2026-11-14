import { describe, expect, it } from "vitest";
import { fmtWatchSec } from "./stats-logic";

// WS-01: 視聴統計の秒数は和文単位で表示する（FR-65、仕様決定 CP）。
// 1 時間以上は時間と分、1 時間未満は分と秒、0 以下は 0秒。
describe("WS-01: fmtWatchSec", () => {
  it("0 以下・非有限・null/undefined は 0秒", () => {
    expect(fmtWatchSec(0)).toBe("0秒");
    expect(fmtWatchSec(-5)).toBe("0秒");
    expect(fmtWatchSec(Number.NaN)).toBe("0秒");
    expect(fmtWatchSec(Number.POSITIVE_INFINITY)).toBe("0秒");
    expect(fmtWatchSec(null)).toBe("0秒");
    expect(fmtWatchSec(undefined)).toBe("0秒");
  });

  it("1 分未満は秒だけを表示する", () => {
    expect(fmtWatchSec(1)).toBe("1秒");
    expect(fmtWatchSec(59)).toBe("59秒");
  });

  it("1 時間未満は分と秒を表示し、秒が 0 なら分だけ", () => {
    expect(fmtWatchSec(60)).toBe("1分");
    expect(fmtWatchSec(75)).toBe("1分15秒");
    expect(fmtWatchSec(3599)).toBe("59分59秒");
  });

  it("1 時間以上は時間と分を表示し、秒は切り捨てる", () => {
    expect(fmtWatchSec(3600)).toBe("1時間");
    expect(fmtWatchSec(3661)).toBe("1時間1分");
    expect(fmtWatchSec(45296)).toBe("12時間34分");
  });
});
