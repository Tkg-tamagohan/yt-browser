// 連続再生キュー・ループの純粋ロジックの回帰テスト（FR-16、仕様決定 AA・AD）。
// LP-01: 武装キューの選定（1 項目は現在項目のみ、全体は回転順、なしは次項目以降）
// LP-02: 終了イベントの継続項目によるキュー位置の照合
import { describe, expect, test } from "vitest";
import { armPlanFor, reconcileIndex } from "./queue-logic";

describe("LP-01 armPlanFor", () => {
  const items = ["A", "B", "C"];

  test("none は次項目以降を順に登録し、末尾では空列（武装解除）", () => {
    expect(armPlanFor("none", items, 0)).toEqual({ items: ["B", "C"], loop: false });
    expect(armPlanFor("none", items, 1)).toEqual({ items: ["C"], loop: false });
    expect(armPlanFor("none", items, 2)).toEqual({ items: [], loop: false });
  });

  test("all は次項目以降＋先頭〜現在項目の回転順を巡回登録する", () => {
    // [A]再生中: 今後は B,C,A,B,C,... と巡回する（B→C→A→B の回転）
    expect(armPlanFor("all", items, 0)).toEqual({ items: ["B", "C", "A"], loop: true });
    // [C]再生中（末尾）: 今後は A,B,C,A,... と先頭へ巻き戻る
    expect(armPlanFor("all", items, 2)).toEqual({ items: ["A", "B", "C"], loop: true });
    // 1 項目だけのキューでも回転順＝現在項目のみ（繰り返し）
    expect(armPlanFor("all", ["A"], 0)).toEqual({ items: ["A"], loop: true });
  });

  test("one は常に現在項目のみを巡回登録する（キュー中も次へ進まない）", () => {
    expect(armPlanFor("one", items, 0)).toEqual({ items: ["A"], loop: true });
    expect(armPlanFor("one", items, 2)).toEqual({ items: ["C"], loop: true });
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
