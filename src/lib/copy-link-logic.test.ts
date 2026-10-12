// 「リンクをコピー」の文面組み立ての回帰テスト（FR-34、仕様決定 BC）。
// CL-01: 文面の組み立て（タイトルありは 2 行、タイトル無しは URL のみ、t= を含めない）
// CL-02: URL 形式の media-title の潰し（タイトル未取得フォールバック。レビュー指摘の回帰）
import { describe, expect, test } from "vitest";
import { linkCopyText } from "./copy-link-logic";

const URL = "https://www.youtube.com/watch?v=dQw4w9WgXcQ";

describe("CL-01 linkCopyText の文面組み立て", () => {
  test("タイトルがある動画はタイトル＋改行＋URL の 2 行を返す", () => {
    expect(linkCopyText("dQw4w9WgXcQ", "動画タイトル")).toBe(
      `動画タイトル\n${URL}`,
    );
  });

  test("タイトルが空・空白のみの動画は URL のみを返す", () => {
    expect(linkCopyText("dQw4w9WgXcQ", "")).toBe(URL);
    expect(linkCopyText("dQw4w9WgXcQ", "  ")).toBe(URL);
  });

  test("URL は videoId から組み立て再生位置 t= を含めない", () => {
    const text = linkCopyText("dQw4w9WgXcQ", "動画タイトル");
    expect(text).toContain(URL);
    expect(text).not.toContain("t=");
  });
});

describe("CL-02 URL 形式タイトルの潰し", () => {
  test("watch?v=... や http(s)://... の media-title は URL のみを返す", () => {
    // mpv がタイトル未取得時に media-title へ置くフォールバック値
    expect(linkCopyText("dQw4w9WgXcQ", "watch?v=dQw4w9WgXcQ")).toBe(URL);
    expect(linkCopyText("dQw4w9WgXcQ", "https://youtu.be/dQw4w9WgXcQ")).toBe(
      URL,
    );
    expect(linkCopyText("dQw4w9WgXcQ", "http://example.com/x")).toBe(URL);
  });

  test("前後空白つきの通常タイトルはトリムしたうえで 2 行を返す", () => {
    expect(linkCopyText("id", "  タイトル  ")).toBe(
      "タイトル\nhttps://www.youtube.com/watch?v=id",
    );
  });
});
