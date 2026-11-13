// シークバー位置換算と SponsorBlock 区間帯の回帰テスト（FR-36、BG-8、仕様決定 BI）。
// SB-01: 再生位置とポインタ位置の割合換算（境界・duration 不明のクランプ）
// SB-02: キーボード操作のシーク先（左右 ±5 秒・Home/End・対象外キー）
// SB-03: 区間帯の割合計算（はみ出しクランプ・無効区間・duration 不明）
// SB-04: 動作 off 区間の表示除外
import { describe, expect, test } from "vitest";
import {
  ARROW_STEP_SEC,
  positionForRatio,
  ratioForPosition,
  ratioFromPointerX,
  seekTargetForKey,
  segmentBand,
  visibleSegments,
} from "$lib/seek-logic";

describe("SB-01 位置↔割合の換算", () => {
  test("再生位置の割合化", () => {
    expect(ratioForPosition(0, 100)).toBe(0);
    expect(ratioForPosition(50, 100)).toBe(0.5);
    expect(ratioForPosition(100, 100)).toBe(1);
  });

  test("duration 不明と範囲外のクランプ", () => {
    expect(ratioForPosition(10, 0)).toBe(0);
    expect(ratioForPosition(10, -5)).toBe(0);
    expect(ratioForPosition(-3, 100)).toBe(0);
    expect(ratioForPosition(150, 100)).toBe(1);
  });

  test("ポインタ X の割合化（トラック外は端へクランプ）", () => {
    expect(ratioFromPointerX(0, 0, 200)).toBe(0);
    expect(ratioFromPointerX(100, 0, 200)).toBe(0.5);
    expect(ratioFromPointerX(200, 0, 200)).toBe(1);
    expect(ratioFromPointerX(-10, 0, 200)).toBe(0);
    expect(ratioFromPointerX(300, 0, 200)).toBe(1);
    expect(ratioFromPointerX(150, 50, 200)).toBe(0.5);
    expect(ratioFromPointerX(0, 0, 0)).toBe(0);
  });

  test("割合から秒へ（往復で一致）", () => {
    expect(positionForRatio(0.25, 600)).toBe(150);
    expect(positionForRatio(0, 600)).toBe(0);
    expect(positionForRatio(1, 600)).toBe(600);
    expect(positionForRatio(1.5, 600)).toBe(600);
    expect(positionForRatio(0.5, 0)).toBe(0);
    // ポインタ→秒の往復が元の位置を維持する（ドラッグ終点ずれの回帰）
    const duration = 1234.5;
    const px = 377;
    const rect = { left: 12, width: 500 };
    const seconds = positionForRatio(
      ratioFromPointerX(px, rect.left, rect.width),
      duration,
    );
    expect(seconds).toBeCloseTo(((px - rect.left) / rect.width) * duration);
  });
});

describe("SB-02 キーボード操作", () => {
  test("左右は ±5 秒", () => {
    expect(ARROW_STEP_SEC).toBe(5);
    expect(seekTargetForKey("ArrowLeft", 30, 100)).toBe(25);
    expect(seekTargetForKey("ArrowRight", 30, 100)).toBe(35);
  });

  test("端でクランプする", () => {
    expect(seekTargetForKey("ArrowLeft", 3, 100)).toBe(0);
    expect(seekTargetForKey("ArrowRight", 98, 100)).toBe(100);
    expect(seekTargetForKey("ArrowRight", 100, 100)).toBe(100);
  });

  test("Home/End は先頭・末尾", () => {
    expect(seekTargetForKey("Home", 30, 100)).toBe(0);
    expect(seekTargetForKey("End", 30, 100)).toBe(100);
  });

  test("対象外キーと duration 不明は null", () => {
    expect(seekTargetForKey("Space", 30, 100)).toBeNull();
    expect(seekTargetForKey("a", 30, 100)).toBeNull();
    expect(seekTargetForKey("ArrowRight", 30, 0)).toBeNull();
  });
});

describe("SB-03 区間帯の割合計算", () => {
  test("通常の区間", () => {
    expect(segmentBand(25, 75, 100)).toEqual({ leftPct: 25, widthPct: 50 });
    expect(segmentBand(0, 100, 100)).toEqual({ leftPct: 0, widthPct: 100 });
  });

  test("はみ出す区間はクランプされる", () => {
    expect(segmentBand(-10, 20, 100)).toEqual({ leftPct: 0, widthPct: 20 });
    expect(segmentBand(90, 150, 100)).toEqual({ leftPct: 90, widthPct: 10 });
  });

  test("無効区間は null", () => {
    expect(segmentBand(50, 50, 100)).toBeNull();
    expect(segmentBand(60, 50, 100)).toBeNull();
    expect(segmentBand(10, 20, 0)).toBeNull();
    expect(segmentBand(-30, -10, 100)).toBeNull(); // 全体が範囲外
    expect(segmentBand(110, 120, 100)).toBeNull();
  });
});

describe("SB-04 表示対象の区間", () => {
  test("off の区間を除外する", () => {
    const segs = [
      { category: "sponsor", action: "skip" },
      { category: "intro", action: "off" },
      { category: "outro", action: "notify" },
    ];
    expect(visibleSegments(segs).map((s) => s.category)).toEqual([
      "sponsor",
      "outro",
    ]);
    expect(visibleSegments([])).toEqual([]);
  });
});
