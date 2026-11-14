// アプリ側キーボードショートカット（FR-79、仕様決定 DD）の
// 純粋ロジックの回帰テスト。キーマップ・有効化設定・対象ガード・
// 選択行の遷移は暫定解釈（shortcuts-logic.ts の先頭コメント参照）。
// KS-01: 既定キーマップ（j/↓・k/↑・Enter・a・q）の写像
// KS-02: ui.shortcuts の受理集合（on 系のみ有効、既定 off）
// KS-03: 未割当キーと修飾キー付きは対象外
// KS-04: 入力要素（input/textarea/select/contenteditable）のガード
// KS-05: 一覧選択の遷移（未選択からの出入り・端でのクランプ・空一覧）
// KS-06: ボタン・リンク上の Enter は要素の既定起動を優先する
import { describe, expect, test } from "vitest";
import {
  DEFAULT_KEYMAP,
  isActivateTarget,
  isEditableTarget,
  moveIndex,
  resolveShortcut,
  shortcutsEnabled,
} from "./shortcuts-logic";

describe("KS-01 既定キーマップの写像（暫定解釈）", () => {
  test("j / ArrowDown は次の行、k / ArrowUp は前の行", () => {
    expect(DEFAULT_KEYMAP.j).toBe("listNext");
    expect(DEFAULT_KEYMAP.ArrowDown).toBe("listNext");
    expect(DEFAULT_KEYMAP.k).toBe("listPrev");
    expect(DEFAULT_KEYMAP.ArrowUp).toBe("listPrev");
  });

  test("Enter は選択行の再生、a はキューに追加、q はパネル開閉", () => {
    expect(DEFAULT_KEYMAP.Enter).toBe("listPlay");
    expect(DEFAULT_KEYMAP.a).toBe("listQueueAdd");
    expect(DEFAULT_KEYMAP.q).toBe("queueToggle");
  });

  test("resolveShortcut はキーマップ通りに解決する", () => {
    expect(resolveShortcut({ key: "j" })).toBe("listNext");
    expect(resolveShortcut({ key: "ArrowUp" })).toBe("listPrev");
    expect(resolveShortcut({ key: "Enter" })).toBe("listPlay");
    expect(resolveShortcut({ key: "a" })).toBe("listQueueAdd");
    expect(resolveShortcut({ key: "q" })).toBe("queueToggle");
  });
});

describe("KS-02 ui.shortcuts の解釈（on 系のみ、既定 off）", () => {
  test("on 系（on/true/1/yes）は有効、大小文字・前後空白を許容する", () => {
    for (const v of ["on", "ON", " on ", "true", "1", "yes"]) {
      expect(shortcutsEnabled(v), v).toBe(true);
    }
  });

  test("未設定・off・その他の値は無効（既定）", () => {
    for (const v of [null, undefined, "", "off", "0", "no", "enabled"]) {
      expect(shortcutsEnabled(v), String(v)).toBe(false);
    }
  });
});

describe("KS-03 対象外キーと修飾キー", () => {
  test("マップに無いキーは null", () => {
    expect(resolveShortcut({ key: "x" })).toBeNull();
    expect(resolveShortcut({ key: "Escape" })).toBeNull();
    expect(resolveShortcut({ key: "J" })).toBeNull();
  });

  test("Ctrl/Alt/Meta 付きはブラウザ・OS 側を優先して対象外", () => {
    expect(resolveShortcut({ key: "j", ctrlKey: true })).toBeNull();
    expect(resolveShortcut({ key: "j", altKey: true })).toBeNull();
    expect(resolveShortcut({ key: "q", metaKey: true })).toBeNull();
    expect(resolveShortcut({ key: "Enter", ctrlKey: true })).toBeNull();
  });
});

describe("KS-04 入力要素のガード", () => {
  test("input / textarea / select / contenteditable は対象外", () => {
    expect(isEditableTarget({ tagName: "INPUT" })).toBe(true);
    expect(isEditableTarget({ tagName: "input" })).toBe(true);
    expect(isEditableTarget({ tagName: "TEXTAREA" })).toBe(true);
    expect(isEditableTarget({ tagName: "SELECT" })).toBe(true);
    expect(isEditableTarget({ isContentEditable: true })).toBe(true);
    expect(
      isEditableTarget({ tagName: "DIV", closest: () => ({} as Element) }),
    ).toBe(true);
  });

  test("通常要素・非要素はガードしない", () => {
    expect(isEditableTarget({ tagName: "DIV", closest: () => null })).toBe(
      false,
    );
    expect(isEditableTarget({ tagName: "BUTTON" })).toBe(false);
    expect(isEditableTarget(null)).toBe(false);
    expect(isEditableTarget("input")).toBe(false);
  });
});

describe("KS-05 一覧選択の遷移", () => {
  test("未選択（-1）から下へは先頭、上へは末尾", () => {
    expect(moveIndex(-1, 1, 5)).toBe(0);
    expect(moveIndex(-1, -1, 5)).toBe(4);
  });

  test("選択中は前後へ移動し、端でクランプする（折り返さない）", () => {
    expect(moveIndex(2, 1, 5)).toBe(3);
    expect(moveIndex(2, -1, 5)).toBe(1);
    expect(moveIndex(4, 1, 5)).toBe(4);
    expect(moveIndex(0, -1, 5)).toBe(0);
  });

  test("空一覧は常に選択なし（-1）", () => {
    expect(moveIndex(-1, 1, 0)).toBe(-1);
    expect(moveIndex(3, 1, 0)).toBe(-1);
  });
});

describe("KS-06 Enter 既定起動の保護", () => {
  test("button / a / summary 上の Enter は既定起動を優先する", () => {
    expect(isActivateTarget({ tagName: "BUTTON" })).toBe(true);
    expect(isActivateTarget({ tagName: "A" })).toBe(true);
    expect(isActivateTarget({ tagName: "SUMMARY" })).toBe(true);
  });

  test("その他の要素や非要素は起動対象でない", () => {
    expect(isActivateTarget({ tagName: "DIV" })).toBe(false);
    expect(isActivateTarget({ tagName: "SPAN" })).toBe(false);
    expect(isActivateTarget(null)).toBe(false);
  });
});
