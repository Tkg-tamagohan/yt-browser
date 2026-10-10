import { describe, expect, it } from "vitest";
import { feedItemSortsAfter } from "./feed-page-logic";

const item = (publishedAt: string | null, videoId: string) => ({
  publishedAt,
  videoId,
});

describe("feedItemSortsAfter", () => {
  it("published_at がカーソルより古い項目は後側", () => {
    expect(
      feedItemSortsAfter(item("2024-01-01T00:00:00Z", "a"), "2024-02-01T00:00:00Z", "z"),
    ).toBe(true);
    expect(
      feedItemSortsAfter(item("2024-03-01T00:00:00Z", "a"), "2024-02-01T00:00:00Z", "z"),
    ).toBe(false);
  });

  it("同時刻は video_id の昇順でタイブレークする", () => {
    const cur = "2024-02-01T00:00:00Z";
    expect(feedItemSortsAfter(item(cur, "b"), cur, "a")).toBe(true);
    expect(feedItemSortsAfter(item(cur, "a"), cur, "b")).toBe(false);
    expect(feedItemSortsAfter(item(cur, "a"), cur, "a")).toBe(false);
  });

  it("NULL 投稿日はカーソルが非 NULL なら常に後側（NULLS LAST）", () => {
    expect(
      feedItemSortsAfter(item(null, "a"), "2024-02-01T00:00:00Z", "a"),
    ).toBe(true);
  });

  it("カーソルが NULL 投稿日なら NULL 同士で video_id 比較", () => {
    expect(feedItemSortsAfter(item(null, "b"), null, "a")).toBe(true);
    expect(feedItemSortsAfter(item(null, "a"), null, "b")).toBe(false);
    // 非 NULL は NULL カーソルより後に来ない（NULL が末尾側のため）
    expect(feedItemSortsAfter(item("2024-01-01T00:00:00Z", "z"), null, "a")).toBe(
      false,
    );
  });
});
