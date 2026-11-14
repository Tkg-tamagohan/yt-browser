// 再生完了時の自動クローズ判定の回帰テスト（FR-43、仕様決定 BS）。
// EC-01: closeOnEofEnabled の受理集合（on 系のみ、既定 off）
// EC-02: closeInstanceOnEnd の終端判定（eof 限定・継続除外・非 eof は畳む）
import { describe, expect, test } from "vitest";
import { closeInstanceOnEnd, closeOnEofEnabled } from "./eof-logic";

describe("EC-01 closeOnEofEnabled の解釈", () => {
  test("on 系値だけを有効として受理する", () => {
    for (const on of ["on", "true", "1", "yes", "ON", " on "]) {
      expect(closeOnEofEnabled(on)).toBe(true);
    }
  });

  test("未設定・その他の値は既定の off として扱う", () => {
    for (const off of ["off", "false", "0", "no", "", "x"]) {
      expect(closeOnEofEnabled(off)).toBe(false);
    }
    expect(closeOnEofEnabled(null)).toBe(false);
  });
});

describe("EC-02 closeInstanceOnEnd の終端判定", () => {
  test("off + eof + 非継続 → 残す", () => {
    expect(closeInstanceOnEnd("eof", false, false)).toBe(false);
  });

  test("on + eof + 非継続 → 畳む（キュー消化完了の eof も同じ経路）", () => {
    expect(closeInstanceOnEnd("eof", false, true)).toBe(true);
  });

  test("継続再生へ進んだ終端は設定に関わらず残す", () => {
    expect(closeInstanceOnEnd("eof", true, true)).toBe(false);
    expect(closeInstanceOnEnd("eof", true, false)).toBe(false);
  });

  test("eof 以外の終端は設定に関わらず畳む", () => {
    for (const reason of ["error", "stop", "process_exit", "unknown"]) {
      expect(closeInstanceOnEnd(reason, false, false)).toBe(true);
      expect(closeInstanceOnEnd(reason, false, true)).toBe(true);
    }
  });
});
