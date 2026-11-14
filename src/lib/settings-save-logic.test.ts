// 設定画面（src/routes/settings/+page.svelte）の保存検証・競合検出テスト。
// SV-01: validateOptionalRange / validateAutoReadDays / validateWheelDelta の境界・不正入力
// SV-02: saveToggleSetting / saveStringSetting / saveSettingBatch のスナップショット競合検出
// SV-03: saveOptionalRangeSetting の空欄保存・範囲外スキップ・待機中変更検出
import { describe, expect, test } from "vitest";
import { TONE_MAPPING_MPV } from "./i18n";
import {
  saveOptionalRangeSetting,
  saveSettingBatch,
  saveStringSetting,
  saveToggleSetting,
  validateAutoReadDays,
  validateOptionalRange,
  validateWheelDelta,
} from "./settings-save-logic";

describe("SV-01: 設定入力値の検証（範囲・自動既読日数・ホイール変化量）", () => {
  test("validateOptionalRange は空欄を受理し、空白トリム後に [min, max] を検証する", () => {
    expect(validateOptionalRange("", 10, 32)).toEqual({ ok: true, value: "" });
    expect(validateOptionalRange("  ", 10, 32)).toEqual({ ok: true, value: "" });
    expect(validateOptionalRange(" 10 ", 10, 32)).toEqual({ ok: true, value: "10" });
    expect(validateOptionalRange("32", 10, 32)).toEqual({ ok: true, value: "32" });
    expect(validateOptionalRange("0.25", 0.25, 4.0)).toEqual({
      ok: true,
      value: "0.25",
    });

    expect(validateOptionalRange("9", 10, 32)).toEqual({ ok: false, raw: "9" });
    expect(validateOptionalRange("33", 10, 32)).toEqual({ ok: false, raw: "33" });
    expect(validateOptionalRange("NaN", 10, 32)).toEqual({ ok: false, raw: "NaN" });
    expect(validateOptionalRange("Infinity", 10, 32)).toEqual({
      ok: false,
      raw: "Infinity",
    });
    expect(validateOptionalRange("abc", 0, 130)).toEqual({ ok: false, raw: "abc" });
  });

  test("validateAutoReadDays は undefined と 0 以上の整数のみ受理する", () => {
    expect(validateAutoReadDays(undefined)).toEqual({ ok: true, value: "0" });
    expect(validateAutoReadDays(0)).toEqual({ ok: true, value: "0" });
    expect(validateAutoReadDays(14)).toEqual({ ok: true, value: "14" });
    expect(validateAutoReadDays(-1)).toEqual({ ok: false, raw: "-1" });
    expect(validateAutoReadDays(1.5)).toEqual({ ok: false, raw: "1.5" });
    expect(validateAutoReadDays(Number.NaN)).toEqual({ ok: false, raw: "NaN" });
  });

  test("validateWheelDelta は undefined をスキップし ±100 以内の有限値のみ受理する", () => {
    expect(validateWheelDelta(undefined)).toEqual({ skip: true });
    expect(validateWheelDelta(-100)).toEqual({ skip: false, ok: true, value: "-100" });
    expect(validateWheelDelta(100)).toEqual({ skip: false, ok: true, value: "100" });
    expect(validateWheelDelta(101)).toEqual({ skip: false, ok: false, raw: "101" });
    expect(validateWheelDelta(Number.POSITIVE_INFINITY)).toEqual({
      skip: false,
      ok: false,
      raw: "Infinity",
    });
  });
});

describe("SV-02: 保存待ち中のスナップショット競合検出", () => {
  test("saveToggleSetting は変化無ければ true、保存中に値が変われば未保存通知と false", async () => {
    const writes: [string, string][] = [];
    let current = true;
    const unsaved: string[] = [];

    const okSame = await saveToggleSetting(
      async (k, v) => {
        writes.push([k, v]);
      },
      "pip.default",
      current,
      () => current,
      () => unsaved.push("unsaved"),
    );
    expect(okSame).toBe(true);
    expect(writes).toEqual([["pip.default", "on"]]);
    expect(unsaved).toEqual([]);

    const okChanged = await saveToggleSetting(
      async (k, v) => {
        writes.push([k, v]);
        current = false;
      },
      "pip.default",
      current,
      () => current,
      () => unsaved.push("unsaved"),
    );
    expect(okChanged).toBe(false);
    expect(writes).toEqual([
      ["pip.default", "on"],
      ["pip.default", "on"],
    ]);
    expect(unsaved).toEqual(["unsaved"]);
  });

  test("saveStringSetting / saveSettingBatch は writeValue 分離と競合検出を行う", async () => {
    const writes: [string, string][] = [];
    let screen = "DISPLAY1";
    const unsaved: string[] = [];

    expect(
      await saveStringSetting(
        async (k, v) => {
          writes.push([k, v]);
        },
        "window.screen",
        screen,
        () => screen,
        () => unsaved.push("screen"),
      ),
    ).toBe(true);
    expect(writes).toEqual([["window.screen", "DISPLAY1"]]);

    let tm = "bt2390";
    let cp = "auto";
    expect(
      await saveSettingBatch(
        async (k, v) => {
          writes.push([k, v]);
          if (k === "hdr.compute_peak") {
            tm = "clip";
          }
        },
        [
          {
            key: "hdr.tone_mapping",
            snapshot: tm,
            writeValue: TONE_MAPPING_MPV[tm],
            getCurrent: () => tm,
          },
          { key: "hdr.compute_peak", snapshot: cp, getCurrent: () => cp },
        ],
        () => unsaved.push("hdr"),
      ),
    ).toBe(false);
    expect(writes.slice(1)).toEqual([
      ["hdr.tone_mapping", TONE_MAPPING_MPV.bt2390],
      ["hdr.compute_peak", "auto"],
    ]);
    expect(unsaved).toEqual(["hdr"]);
  });
});

describe("SV-03: saveOptionalRangeSetting の空欄・範囲外・競合動作", () => {
  test("空欄と範囲内は書き込み、範囲外は書き込まず失敗コールバックを呼ぶ", async () => {
    const writes: [string, string][] = [];
    const failed: string[] = [];
    const unsaved: string[] = [];

    let font = "  ";
    expect(
      await saveOptionalRangeSetting(
        async (k, v) => {
          writes.push([k, v]);
        },
        "chat.popup_font_size",
        font,
        () => font,
        10,
        32,
        (raw) => failed.push(raw),
        () => unsaved.push("font"),
      ),
    ).toBe(true);
    expect(writes).toEqual([["chat.popup_font_size", ""]]);

    font = "99";
    expect(
      await saveOptionalRangeSetting(
        async (k, v) => {
          writes.push([k, v]);
        },
        "chat.popup_font_size",
        font,
        () => font,
        10,
        32,
        (raw) => failed.push(raw),
        () => unsaved.push("font"),
      ),
    ).toBe(false);
    expect(writes).toHaveLength(1);
    expect(failed).toEqual(["99"]);
    expect(unsaved).toEqual([]);

    font = "18";
    expect(
      await saveOptionalRangeSetting(
        async (k, v) => {
          writes.push([k, v]);
          font = "20";
        },
        "chat.popup_font_size",
        font,
        () => font,
        10,
        32,
        (raw) => failed.push(raw),
        () => unsaved.push("font"),
      ),
    ).toBe(false);
    expect(writes).toEqual([
      ["chat.popup_font_size", ""],
      ["chat.popup_font_size", "18"],
    ]);
    expect(unsaved).toEqual(["font"]);
  });
});
