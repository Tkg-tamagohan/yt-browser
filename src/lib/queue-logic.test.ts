// 連続再生キュー・ループの純粋ロジックの回帰テスト（FR-16、仕様決定 AA）。
// LP-01: 武装対象の選定（1 項目は現在項目、全体は末尾で先頭、なしは次項目）
// LP-02: 終了イベントの継続項目によるキュー位置の照合
import { describe, expect, test } from "vitest";
import { armTargetFor, reconcileIndex } from "./queue-logic";

describe("LP-01 armTargetFor", () => {
  const items = ["A", "B", "C"];

  test("none は次項目を返し、末尾では null（武装解除）", () => {
    expect(armTargetFor("none", items, 0)).toBe("B");
    expect(armTargetFor("none", items, 1)).toBe("C");
    expect(armTargetFor("none", items, 2)).toBeNull();
  });

  test("all は次項目を返し、末尾では先頭へ戻る", () => {
    expect(armTargetFor("all", items, 0)).toBe("B");
    expect(armTargetFor("all", items, 2)).toBe("A");
    // 1 項目だけのキューでも先頭＝現在項目へ戻る（繰り返し）
    expect(armTargetFor("all", ["A"], 0)).toBe("A");
  });

  test("one は常に現在項目を返す（キュー中も次へ進まない）", () => {
    expect(armTargetFor("one", items, 0)).toBe("A");
    expect(armTargetFor("one", items, 2)).toBe("C");
  });
});

describe("LP-02 reconcileIndex", () => {
  const items = ["A", "B", "C"];

  test("継続項目が現在項目なら繰り返し（位置据え置き）", () => {
    expect(reconcileIndex(items, 0, "A")).toBe(0);
    expect(reconcileIndex(items, 2, "C")).toBe(2);
  });

  test("継続項目が次項目なら 1 つ進める", () => {
    expect(reconcileIndex(items, 0, "B")).toBe(1);
    expect(reconcileIndex(items, 1, "C")).toBe(2);
  });

  test("末尾で先頭項目が来たら全体ループの巻き戻り（0 へ）", () => {
    expect(reconcileIndex(items, 2, "A")).toBe(0);
  });

  test("遷移中のモード変更で続きが繰り返しでも位置を飛ばさない", () => {
    // one → all に遷移と同時に切り替わった場合: 実際に読まれたのは A
    // （モードではなく continuedVideoId で照合する回帰）
    expect(reconcileIndex(items, 0, "A")).toBe(0);
    // 逆に all → one 中に次項目が読まれたら前進する
    expect(reconcileIndex(items, 0, "B")).toBe(1);
  });

  test("キュー外の項目（drift）では位置を変えない", () => {
    expect(reconcileIndex(items, 1, "X")).toBe(1);
    expect(reconcileIndex(items, 1, null)).toBe(1);
  });
});
