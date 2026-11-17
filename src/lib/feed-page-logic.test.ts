import { describe, expect, it } from "vitest";
import {
  feedItemCompare,
  feedItemSortsAfter,
  markFeedItemRead,
} from "./feed-page-logic";

// FP-01: feedItemCompare の並び順（published_at 降順・NULL 末尾・video_id 昇順）
// FP-02: feedItemSortsAfter のカーソル境界判定
// FP-03: markFeedItemRead の一覧更新（未読のみ抽出時は除去、全表示時は isRead 更新）
const item = (publishedAt: string | null, videoId: string) => ({
  publishedAt,
  videoId,
});

describe("FP-01: feedItemCompare", () => {
  it("published_at 降順・NULL 末尾・video_id 昇順の全順序を再現する", () => {
    const sorted = [
      item("2024-03-01T00:00:00Z", "b"),
      item("2024-02-01T00:00:00Z", "z"),
      item("2024-02-01T00:00:00Z", "a"),
      item(null, "b"),
      item(null, "a"),
      item("2024-01-01T00:00:00Z", "x"),
    ].sort(feedItemCompare);
    expect(sorted.map((i) => i.videoId)).toEqual([
      "b", "a", "z", "x", "a", "b",
    ]);
  });
});

describe("FP-02: feedItemSortsAfter", () => {
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

describe("FP-03: markFeedItemRead", () => {
  const sample = [
    { videoId: "v1", isRead: false },
    { videoId: "v2", isRead: false },
  ];

  it("unreadOnly=true では該当項目を一覧から除去する", () => {
    expect(markFeedItemRead(sample, "v1", true)).toEqual([
      { videoId: "v2", isRead: false },
    ]);
  });

  it("unreadOnly=false では該当項目のみ isRead=true へ更新する", () => {
    const next = markFeedItemRead(sample, "v1", false);
    expect(next).toEqual([
      { videoId: "v1", isRead: true },
      { videoId: "v2", isRead: false },
    ]);
  });
});
