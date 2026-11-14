// 設定画面の節フィルタの回帰テスト（FR-73、仕様決定 CX）。
// SF-01: sectionMatches の一致判定（大小文字無視・空クエリは全一致・ラベル OR）
// SF-02: filterSectionIds の絞り込み結果と節定義の網羅
import { describe, expect, test } from "vitest";
import {
  filterSectionIds,
  resolveSectionDefs,
  sectionMatches,
  SETTINGS_SECTION_DEFS,
} from "./settings-filter";
import { t } from "./i18n";

describe("SF-01 sectionMatches の一致判定", () => {
  const labels = ["画質（ytdl-format）", "フォーマット式", "カスタム"];

  test("空・空白のみのクエリは常に一致する", () => {
    for (const q of ["", " ", "　"]) {
      expect(sectionMatches(q, labels)).toBe(true);
      expect(sectionMatches(q, [])).toBe(true);
    }
  });

  test("ラベルへの部分一致で一致する（大小文字は区別しない）", () => {
    expect(sectionMatches("画質", labels)).toBe(true);
    expect(sectionMatches("フォーマット", labels)).toBe(true);
    // 英字は大小文字を区別しない
    expect(sectionMatches("YTDLP", ["ytdlp.path"])).toBe(true);
    expect(sectionMatches("mpv", ["MPV 引数"])).toBe(true);
  });

  test("いずれかのラベルに一致すればよい（OR 判定）", () => {
    expect(sectionMatches("カスタム", ["画質", "カスタム"])).toBe(true);
    expect(sectionMatches("存在しない語", labels)).toBe(false);
    expect(sectionMatches("画質", [])).toBe(false);
  });
});

describe("SF-02 filterSectionIds の絞り込み結果", () => {
  const defs = resolveSectionDefs(t);

  test("空クエリでは全節が返る", () => {
    const ids = filterSectionIds("", defs);
    expect(ids.size).toBe(SETTINGS_SECTION_DEFS.length);
    for (const d of SETTINGS_SECTION_DEFS) {
      expect(ids.has(d.id)).toBe(true);
    }
  });

  test("クエリ一致の節だけが残り、不一致は除外される", () => {
    const ids = filterSectionIds("フィード", defs);
    expect(ids.has("feed")).toBe(true);
    expect(ids.has("quality")).toBe(false);
    // ラベルに依存語を含む節の確認（コマンド名の extraLabels）
    const deps = filterSectionIds("yt-dlp", defs);
    expect(deps.has("deps")).toBe(true);
    expect(deps.has("diag")).toBe(true);
    expect(deps.has("ops")).toBe(false);
  });

  test("節定義の id は重複しない", () => {
    const ids = SETTINGS_SECTION_DEFS.map((d) => d.id);
    expect(new Set(ids).size).toBe(ids.length);
  });
});
