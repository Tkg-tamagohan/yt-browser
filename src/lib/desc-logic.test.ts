// 概要欄の分割ロジックの回帰テスト（FR-33、仕様決定 BB）。
// DP-01: URL と時刻表記の分割（末尾記号の除去・h:mm:ss の解釈・改行の保持）
// DP-02: 時刻表記の誤検出抑止（比率・秒の範囲外・コロン連続・URL 内の時刻）
import { describe, expect, test } from "vitest";
import { parseDescription } from "./desc-logic";

describe("DP-01 parseDescription の分割", () => {
  test("URL はリンク区切り、時刻表記は seek 区切りになる", () => {
    const segs = parseDescription(
      "詳細は https://example.com/a を参照\n[03:25] 曲名",
    );
    expect(segs).toEqual([
      { kind: "text", text: "詳細は " },
      { kind: "url", text: "https://example.com/a", url: "https://example.com/a" },
      { kind: "text", text: " を参照\n[" },
      { kind: "seek", text: "03:25", seconds: 205 },
      { kind: "text", text: "] 曲名" },
    ]);
  });

  test("改行は text 区内に保持される", () => {
    const segs = parseDescription("1 行目\n\n3 行目");
    expect(segs).toEqual([{ kind: "text", text: "1 行目\n\n3 行目" }]);
  });

  test("h:mm:ss は時間込みの秒数へ、mm:ss は分秒へ換算する", () => {
    expect(parseDescription("1:02:03")).toEqual([
      { kind: "seek", text: "1:02:03", seconds: 3723 },
    ]);
    expect(parseDescription("12:34")).toEqual([
      { kind: "seek", text: "12:34", seconds: 754 },
    ]);
    expect(parseDescription("0:00")).toEqual([
      { kind: "seek", text: "0:00", seconds: 0 },
    ]);
    expect(parseDescription("100:30")).toEqual([
      { kind: "seek", text: "100:30", seconds: 6030 },
    ]);
  });

  test("URL 末尾の句読点・外側括弧は本文側へ戻す", () => {
    expect(parseDescription("見て https://example.com/x。")).toEqual([
      { kind: "text", text: "見て " },
      { kind: "url", text: "https://example.com/x", url: "https://example.com/x" },
      { kind: "text", text: "。" },
    ]);
    expect(parseDescription("公式（https://example.com/x）")).toEqual([
      { kind: "text", text: "公式（" },
      { kind: "url", text: "https://example.com/x", url: "https://example.com/x" },
      { kind: "text", text: "）" },
    ]);
    // URL 内の対応済み括弧は残す
    expect(parseDescription("https://example.com/Foo_(bar) へ")).toEqual([
      { kind: "url", text: "https://example.com/Foo_(bar)", url: "https://example.com/Foo_(bar)" },
      { kind: "text", text: " へ" },
    ]);
  });

  test("空文字列は空列を返す", () => {
    expect(parseDescription("")).toEqual([]);
  });
});

describe("DP-02 時刻表記の誤検出抑止", () => {
  test("比率・秒 60 超・4 桁以上の分は対象外", () => {
    for (const s of ["16:9", "1:2:3", "3:80", "1234:56", "12:34:56:78"]) {
      expect(parseDescription(s)).toEqual([{ kind: "text", text: s }]);
    }
  });

  test("数字やコロンに挟まれた途中では一致しない", () => {
    // 直前が数字・コロンなら部分一致しない（文字列先頭や区切り後のみ）
    expect(parseDescription(":12:34")).toEqual([
      { kind: "text", text: ":12:34" },
    ]);
    // 英文字の直後なら時刻として切り出す
    expect(parseDescription("v12:34")).toEqual([
      { kind: "text", text: "v" },
      { kind: "seek", text: "12:34", seconds: 754 },
    ]);
  });

  test("URL 内の時刻風トークンは URL の一部として扱う", () => {
    const segs = parseDescription("https://example.com/?t=10:30 へ");
    expect(segs).toEqual([
      {
        kind: "url",
        text: "https://example.com/?t=10:30",
        url: "https://example.com/?t=10:30",
      },
      { kind: "text", text: " へ" },
    ]);
  });
});
