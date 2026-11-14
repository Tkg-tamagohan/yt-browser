import { describe, expect, it } from "vitest";
import { focusDisabledKey, focusWindowSupported } from "./focus-logic";

// FG-01: mpv 窓の前面化可否判定（FR-42、仕様決定 BR）
describe("FG-01: mpv 窓の前面化可否判定（FR-42、仕様決定 BR）", () => {
  it("windows と x11 は前面化可能、wayland は恒久制限で不可", () => {
    expect(focusWindowSupported("windows")).toBe(true);
    expect(focusWindowSupported("x11")).toBe(true);
    expect(focusWindowSupported("wayland")).toBe(false);
    expect(focusWindowSupported("other")).toBe(false);
  });

  it("無効理由のツールチップキー: wayland は専用文、その他の不明環境は汎用文、対応環境と未判定は null", () => {
    expect(focusDisabledKey("wayland")).toBe("player.focus.wayland");
    expect(focusDisabledKey("other")).toBe("player.focus.other");
    expect(focusDisabledKey("windows")).toBeNull();
    expect(focusDisabledKey("x11")).toBeNull();
    // 判定前（windowing_system 未取得・失敗）は無効側に倒さない
    expect(focusDisabledKey("")).toBeNull();
  });
});
