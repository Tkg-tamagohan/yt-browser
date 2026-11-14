// 字幕トラック選択ロジックの回帰テスト（FR-40、仕様決定 BP）。
// ST-01: subTrackLabel の表示名組み立て — title/lang/ID の優先順位
import { describe, expect, test } from "vitest";
import { subTrackLabel } from "./subtitle-logic";

describe("ST-01 subTrackLabel の表示名", () => {
  test("title と lang があれば `title (lang)` で併記する", () => {
    expect(
      subTrackLabel({ id: 3, title: "English", lang: "en" }),
    ).toBe("English (en)");
  });

  test("title だけなら title をそのまま出す", () => {
    expect(subTrackLabel({ id: 3, title: "English", lang: null })).toBe(
      "English",
    );
  });

  test("lang だけなら lang をそのまま出す", () => {
    expect(subTrackLabel({ id: 4, title: null, lang: "ja" })).toBe("ja");
  });

  test("title と lang が同じ文字列なら重複併記しない", () => {
    expect(subTrackLabel({ id: 5, title: "en", lang: "en" })).toBe("en");
  });

  test("どちらも無ければトラック ID を `#id` で出す", () => {
    expect(subTrackLabel({ id: 7, title: null, lang: null })).toBe("#7");
    expect(subTrackLabel({ id: 7, title: "", lang: "" })).toBe("#7");
  });
});
