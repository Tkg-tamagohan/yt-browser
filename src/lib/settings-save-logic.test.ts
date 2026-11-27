// 設定画面（src/routes/settings/+page.svelte）の保存検証・競合検出テスト。
// SV-01: validateOptionalRange / validateAutoReadDays / validateWheelDelta の境界・不正入力
// SV-02: saveToggleSetting / saveStringSetting / saveSettingBatch のスナップショット競合検出
// SV-03: saveOptionalRangeSetting の空欄・数値入力・範囲外スキップ・待機中変更検出
// SV-04: saveSettingBatch の途中失敗時に成功済みキーだけ onCommitted が発火する
// SV-05: youtube.api_key の saveStringSetting による onCommitted スナップショット確定
import { describe, expect, test } from "vitest";
import { TONE_MAPPING_MPV } from "./i18n";
import {
  normalizeOptionalRaw,
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
  test("saveToggleSetting は変化無ければ true、保存中に値が変われば未保存通知と false、onCommitted は DB 成功時のみ発火", async () => {
    const writes: [string, string][] = [];
    let current = true;
    const unsaved: string[] = [];
    const committed: boolean[] = [];

    const okSame = await saveToggleSetting(
      async (k, v) => {
        writes.push([k, v]);
      },
      "pip.default",
      current,
      () => current,
      () => unsaved.push("unsaved"),
      () => committed.push(true),
    );
    expect(okSame).toBe(true);
    expect(writes).toEqual([["pip.default", "on"]]);
    expect(unsaved).toEqual([]);
    expect(committed).toEqual([true]);

    const okChanged = await saveToggleSetting(
      async (k, v) => {
        writes.push([k, v]);
        current = false;
      },
      "pip.default",
      current,
      () => current,
      () => unsaved.push("unsaved"),
      () => committed.push(true),
    );
    expect(okChanged).toBe(false);
    expect(writes).toEqual([
      ["pip.default", "on"],
      ["pip.default", "on"],
    ]);
    expect(unsaved).toEqual(["unsaved"]);
    // DB 書込は成功しているため保存中競合でも onCommitted が発火する
    expect(committed).toEqual([true, true]);

    await expect(
      saveToggleSetting(
        async () => {
          throw new Error("db error");
        },
        "pip.default",
        current,
        () => current,
        () => unsaved.push("unsaved"),
        () => committed.push(false),
      ),
    ).rejects.toThrow("db error");
    expect(committed).toEqual([true, true]);
  });

  test("saveStringSetting / saveSettingBatch は writeValue 分離と競合検出・onCommitted 発火を行う", async () => {
    const writes: [string, string][] = [];
    let screen = "DISPLAY1";
    const unsaved: string[] = [];
    const stringCommitted: string[] = [];

    expect(
      await saveStringSetting(
        async (k, v) => {
          writes.push([k, v]);
        },
        "window.screen",
        screen,
        () => screen,
        () => unsaved.push("screen"),
        () => stringCommitted.push(screen),
      ),
    ).toBe(true);
    expect(writes).toEqual([["window.screen", "DISPLAY1"]]);
    expect(stringCommitted).toEqual(["DISPLAY1"]);

    await expect(
      saveStringSetting(
        async () => {
          throw new Error("db error");
        },
        "window.screen",
        screen,
        () => screen,
        () => unsaved.push("screen"),
        () => stringCommitted.push("should-not-run"),
      ),
    ).rejects.toThrow("db error");
    expect(stringCommitted).toEqual(["DISPLAY1"]);

    let tm = "bt2390";
    let cp = "auto";
    const batchCommitted: string[] = [];
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
            onCommitted: () => batchCommitted.push("hdr.tone_mapping"),
          },
          {
            key: "hdr.compute_peak",
            snapshot: cp,
            getCurrent: () => cp,
            onCommitted: () => batchCommitted.push("hdr.compute_peak"),
          },
        ],
        () => unsaved.push("hdr"),
      ),
    ).toBe(false);
    expect(writes.slice(1)).toEqual([
      ["hdr.tone_mapping", TONE_MAPPING_MPV.bt2390],
      ["hdr.compute_peak", "auto"],
    ]);
    expect(unsaved).toEqual(["hdr"]);
    expect(batchCommitted).toEqual(["hdr.tone_mapping", "hdr.compute_peak"]);
  });
});

describe("SV-03: saveOptionalRangeSetting の空欄・範囲外・競合動作", () => {
  test("空欄と範囲内は書き込み、範囲外や DB 例外時は書き込まず onCommitted を呼ばない", async () => {
    const writes: [string, string][] = [];
    const failed: string[] = [];
    const unsaved: string[] = [];
    const committed: string[] = [];

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
        () => committed.push("empty"),
      ),
    ).toBe(true);
    expect(writes).toEqual([["chat.popup_font_size", ""]]);
    expect(committed).toEqual(["empty"]);

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
        () => committed.push("out-of-range"),
      ),
    ).toBe(false);
    expect(writes).toHaveLength(1);
    expect(failed).toEqual(["99"]);
    expect(unsaved).toEqual([]);
    expect(committed).toEqual(["empty"]);

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
        () => committed.push("race"),
      ),
    ).toBe(false);
    expect(writes).toEqual([
      ["chat.popup_font_size", ""],
      ["chat.popup_font_size", "18"],
    ]);
    expect(unsaved).toEqual(["font"]);
    expect(committed).toEqual(["empty", "race"]);

    await expect(
      saveOptionalRangeSetting(
        async () => {
          throw new Error("db error");
        },
        "chat.popup_font_size",
        "16",
        () => "16",
        10,
        32,
        (raw) => failed.push(raw),
        () => unsaved.push("font"),
        () => committed.push("db-error"),
      ),
    ).rejects.toThrow("db error");
    expect(committed).toEqual(["empty", "race"]);

    // number / undefined（<input type="number"> バインド値）をそのまま保存・検証できる
    let speed: number | undefined = 1.5;
    const speedCommitted: Array<number | undefined> = [];
    expect(normalizeOptionalRaw(speed)).toBe("1.5");
    expect(normalizeOptionalRaw(undefined)).toBe("");
    expect(
      await saveOptionalRangeSetting(
        async (k, v) => {
          writes.push([k, v]);
        },
        "player.initial_speed",
        speed,
        () => speed,
        0.25,
        4.0,
        (raw) => failed.push(raw),
        () => unsaved.push("speed"),
        () => speedCommitted.push(speed),
      ),
    ).toBe(true);
    expect(writes.at(-1)).toEqual(["player.initial_speed", "1.5"]);
    expect(speedCommitted).toEqual([1.5]);

    speed = undefined;
    expect(
      await saveOptionalRangeSetting(
        async (k, v) => {
          writes.push([k, v]);
        },
        "player.initial_speed",
        speed,
        () => speed,
        0.25,
        4.0,
        (raw) => failed.push(raw),
        () => unsaved.push("speed"),
        () => speedCommitted.push(speed),
      ),
    ).toBe(true);
    expect(writes.at(-1)).toEqual(["player.initial_speed", ""]);
    expect(speedCommitted).toEqual([1.5, undefined]);
  });
});

describe("SV-04: saveSettingBatch の途中失敗時の部分コミット確定", () => {
  test("後続キーが例外終了しても先行して保存成功したキーのみ onCommitted が呼ばれる", async () => {
    const committed: string[] = [];
    let theme = "light";
    let lang = "en";

    await expect(
      saveSettingBatch(
        async (key) => {
          if (key === "ui.language") {
            throw new Error("db busy");
          }
        },
        [
          {
            key: "ui.theme",
            snapshot: theme,
            getCurrent: () => theme,
            onCommitted: () => committed.push("ui.theme"),
          },
          {
            key: "ui.language",
            snapshot: lang,
            getCurrent: () => lang,
            onCommitted: () => committed.push("ui.language"),
          },
        ],
        () => {},
      ),
    ).rejects.toThrow("db busy");

    expect(committed).toEqual(["ui.theme"]);
  });
});

describe("SV-05: youtube.api_key の onCommitted スナップショット更新", () => {
  test("保存中に別値へ書き換わっても DB 成功直後の onCommitted で確定値へ更新される", async () => {
    const writes: [string, string][] = [];
    const unsaved: string[] = [];
    const savedSnapshot = { youtubeApiKey: "" };
    const form = { youtubeApiKey: "  AIzaSyTEST_KEY_ABC  " };

    const snap = form.youtubeApiKey.trim();
    const okConflict = await saveStringSetting(
      async (k, v) => {
        writes.push([k, v]);
        // 保存待ち中にユーザーが別のキーへ書き換えた場合を再現
        form.youtubeApiKey = "AIzaSyTEST_KEY_XYZ";
      },
      "youtube.api_key",
      snap,
      () => form.youtubeApiKey.trim(),
      () => unsaved.push("deps"),
      () => {
        savedSnapshot.youtubeApiKey = snap;
      },
    );

    expect(okConflict).toBe(false);
    expect(writes).toEqual([["youtube.api_key", "AIzaSyTEST_KEY_ABC"]]);
    expect(unsaved).toEqual(["deps"]);
    expect(savedSnapshot.youtubeApiKey).toBe("AIzaSyTEST_KEY_ABC");

    // DB 書込が失敗した場合は onCommitted が呼ばれずスナップショットを据え置く
    await expect(
      saveStringSetting(
        async () => {
          throw new Error("db error");
        },
        "youtube.api_key",
        "AIzaSyTEST_KEY_XYZ",
        () => form.youtubeApiKey.trim(),
        () => unsaved.push("deps"),
        () => {
          savedSnapshot.youtubeApiKey = "AIzaSyTEST_KEY_XYZ";
        },
      ),
    ).rejects.toThrow("db error");
    expect(savedSnapshot.youtubeApiKey).toBe("AIzaSyTEST_KEY_ABC");
  });
});
