// ファイルドロップ抑止の判定の回帰テスト（仕様決定 BG）。
// FD-01: ファイルまたは URL を含むドラッグだけを抑止対象と判定する（アプリ内の並べ替えは対象外）
import { describe, expect, test } from "vitest";
import { isFileDrag } from "./file-drop-logic";

describe("FD-01 isFileDrag", () => {
  test("Files を含むドラッグは抑止対象", () => {
    expect(isFileDrag(["Files"])).toBe(true);
    expect(isFileDrag(["application/x-moz-file", "Files"])).toBe(true);
  });

  test("WebKitGTK のファイルドラッグ（text/uri-list、Files なし）も抑止対象", () => {
    expect(isFileDrag(["text/uri-list", "text/html"])).toBe(true);
    expect(isFileDrag(["text/plain", "text/uri-list"])).toBe(true);
  });

  test("アプリ内の並べ替え（text/plain）は抑止しない", () => {
    expect(isFileDrag(["text/plain"])).toBe(false);
    expect(isFileDrag(["text/plain", "text/html"])).toBe(false);
  });

  test("型が無い・取得できない場合は抑止しない", () => {
    expect(isFileDrag([])).toBe(false);
    expect(isFileDrag(null)).toBe(false);
    expect(isFileDrag(undefined)).toBe(false);
  });

  test("DOMStringList 相当の ArrayLike も扱える", () => {
    const list = { 0: "text/plain", 1: "Files", length: 2 };
    expect(isFileDrag(list)).toBe(true);
  });
});
