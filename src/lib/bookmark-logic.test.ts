// 動画内ブックマーク（FR-49、仕様決定 BZ）の純粋ロジックの回帰テスト。
// BM-01: 位置表記（m:ss / h:mm:ss・小数切り捨て・0 以下と非有限は 0:00）
// BM-02: ラベル整形（trim・空・空白のみは位置表記へ復帰）
// BM-03: 一覧の並び（createdAt 降順・同時刻は id 降順）
// BM-04: タイトル表示（解決タイトル優先・未解決・空は動画 ID）
import { describe, expect, test } from "vitest";
import {
  bookmarkCompare,
  bookmarkTitle,
  fmtBookmarkSec,
  normalizeBookmarkLabel,
  type VideoBookmark,
} from "./bookmark-logic";

describe("BM-01 fmtBookmarkSec（位置表記）", () => {
  test("m:ss / h:mm:ss の表記と小数切り捨て", () => {
    expect(fmtBookmarkSec(0)).toBe("0:00");
    expect(fmtBookmarkSec(5)).toBe("0:05");
    expect(fmtBookmarkSec(59)).toBe("0:59");
    expect(fmtBookmarkSec(60)).toBe("1:00");
    expect(fmtBookmarkSec(754)).toBe("12:34");
    expect(fmtBookmarkSec(754.9)).toBe("12:34");
    expect(fmtBookmarkSec(3599)).toBe("59:59");
    expect(fmtBookmarkSec(3600)).toBe("1:00:00");
    expect(fmtBookmarkSec(3661)).toBe("1:01:01");
  });

  test("0 以下・非有限は 0:00", () => {
    expect(fmtBookmarkSec(-3)).toBe("0:00");
    expect(fmtBookmarkSec(NaN)).toBe("0:00");
    expect(fmtBookmarkSec(Infinity)).toBe("0:00");
  });
});

describe("BM-02 normalizeBookmarkLabel", () => {
  test("前後の空白は除く・中身は保持", () => {
    expect(normalizeBookmarkLabel(" 副反応の説明 ", 754)).toBe("副反応の説明");
    expect(normalizeBookmarkLabel("メモ1", 754)).toBe("メモ1");
  });

  test("空・空白のみは位置表記へ戻す", () => {
    expect(normalizeBookmarkLabel("", 754)).toBe("12:34");
    expect(normalizeBookmarkLabel("   ", 3661)).toBe("1:01:01");
  });
});

describe("BM-03 bookmarkCompare（保存の新しい順）", () => {
  function bm(id: number, createdAt: string): VideoBookmark {
    return {
      id,
      videoId: "x",
      positionSec: 0,
      label: "",
      createdAt,
      title: null,
    };
  }

  test("createdAt 降順、同時刻は id 降順", () => {
    const sorted = [
      bm(1, "2026-02-10 10:00:00"),
      bm(3, "2026-02-12 10:00:00"),
      bm(2, "2026-02-10 10:00:00"),
      bm(4, "2026-02-10 10:00:00"),
    ].sort(bookmarkCompare);
    expect(sorted.map((b) => b.id)).toEqual([3, 4, 2, 1]);
  });
});

describe("BM-04 bookmarkTitle（表示タイトル）", () => {
  function bm(title: string | null): VideoBookmark {
    return {
      id: 1,
      videoId: "dQw4w9WgXcQ",
      positionSec: 0,
      label: "",
      createdAt: "2026-02-10 10:00:00",
      title,
    };
  }

  test("解決タイトル優先・null と空は動画 ID", () => {
    expect(bookmarkTitle(bm("動画A"))).toBe("動画A");
    expect(bookmarkTitle(bm(null))).toBe("dQw4w9WgXcQ");
    expect(bookmarkTitle(bm(""))).toBe("dQw4w9WgXcQ");
  });
});
