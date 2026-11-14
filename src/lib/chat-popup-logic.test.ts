// チャットポップアップ表示設定の解釈テスト（FR-68、仕様決定 CS）。
// CP-01: popupShowAuthorEnabled の受理集合（on 系のみ、既定は非表示）
// CP-02: popupFontSize の解釈（10〜32、既定 14）
// CP-03: popupOpacity の解釈（0.2〜1.0、既定 1.0）
// CP-04: 非 Rust 形式の数値文字列（16 進など）は既定値へ戻す
import { describe, expect, test } from "vitest";
import {
  POPUP_FONT_SIZE_DEFAULT,
  POPUP_OPACITY_DEFAULT,
  popupFontSize,
  popupOpacity,
  popupShowAuthorEnabled,
} from "./chat-popup-logic";

describe("CP-01 popupShowAuthorEnabled の受理集合", () => {
  test("on 系の値だけ投稿者名を表示する", () => {
    for (const v of ["on", "ON", " true ", "1", "yes"]) {
      expect(popupShowAuthorEnabled(v), `${v} は表示になるべき`).toBe(true);
    }
  });

  test("off 系・その他・未設定は非表示（既定）", () => {
    for (const v of ["off", "false", "0", "no", "", "  ", "invalid"]) {
      expect(popupShowAuthorEnabled(v), `${v} は非表示になるべき`).toBe(false);
    }
    expect(popupShowAuthorEnabled(null)).toBe(false);
  });
});

describe("CP-02 popupFontSize の解釈", () => {
  test("10〜32 の数値を受理する", () => {
    expect(popupFontSize("10")).toBe(10);
    expect(popupFontSize("14")).toBe(14);
    expect(popupFontSize("16.5")).toBe(16.5);
    expect(popupFontSize("32")).toBe(32);
    expect(popupFontSize("  18  ")).toBe(18);
  });

  test("範囲外・非数・空・未設定は既定 14 へ戻す", () => {
    for (const v of ["", "  ", "abc", "-1", "9", "33", "NaN", "inf"]) {
      expect(popupFontSize(v), `${v} は既定値になるべき`).toBe(
        POPUP_FONT_SIZE_DEFAULT,
      );
    }
    expect(popupFontSize(null)).toBe(POPUP_FONT_SIZE_DEFAULT);
  });
});

describe("CP-03 popupOpacity の解釈", () => {
  test("0.2〜1.0 の数値を受理する", () => {
    expect(popupOpacity("0.2")).toBeCloseTo(0.2);
    expect(popupOpacity("0.5")).toBeCloseTo(0.5);
    expect(popupOpacity("  0.75  ")).toBeCloseTo(0.75);
    expect(popupOpacity("1")).toBeCloseTo(1.0);
  });

  test("範囲外・非数・空・未設定は既定 1.0 へ戻す", () => {
    for (const v of ["", "  ", "abc", "0", "0.1", "1.1", "-0.5", "NaN"]) {
      expect(popupOpacity(v), `${v} は既定値になるべき`).toBe(
        POPUP_OPACITY_DEFAULT,
      );
    }
    expect(popupOpacity(null)).toBe(POPUP_OPACITY_DEFAULT);
  });
});

describe("CP-04 Rust 非受理形式の数値文字列は既定値へ戻す", () => {
  test("16 進・2 進・アンダースコア区切りは既定値", () => {
    // JS の Number() では有限数になるが、Rust の f64::from_str は
    // 受理しない形式（解釈の一致を保つため既定値へ戻す）
    for (const v of ["0x10", "0b101", "1_5"]) {
      expect(popupFontSize(v), `${v} は既定値になるべき`).toBe(
        POPUP_FONT_SIZE_DEFAULT,
      );
      expect(popupOpacity(v), `${v} は既定値になるべき`).toBe(
        POPUP_OPACITY_DEFAULT,
      );
    }
  });
});
