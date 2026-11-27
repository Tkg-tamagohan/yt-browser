import { describe, expect, it } from "vitest";
import {
  broadcastDisplayOf,
  feedItemCompare,
  feedItemSortsAfter,
  markFeedItemRead,
  mergeFeedLoadMore,
  mergeFeedReload,
} from "./feed-page-logic";

// FP-01: feedItemCompare の並び順（published_at 降順・NULL 末尾・video_id 昇順）
// FP-02: feedItemSortsAfter のカーソル境界判定
// FP-03: markFeedItemRead の一覧更新（未読のみ抽出時は除去、全表示時は isRead 更新）
// FP-04: ライブ中優先ソートとカーソル判定
// FP-05: broadcastDisplayOf のバッジ・時刻・フォールバック判定
// FP-06: mergeFeedReload / mergeFeedLoadMore の重複排除と再整列
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

describe("FP-04: ライブ中優先ソートとカーソル判定", () => {
  const bItem = (
    videoId: string,
    publishedAt: string | null,
    broadcastState: string | null,
  ) => ({ videoId, publishedAt, broadcastState });

  it("broadcastState=live の項目は投稿日時が古くても常に先頭に並ぶ", () => {
    const sorted = [
      bItem("vod_new", "2026-11-27T10:00:00Z", null),
      bItem("live_old", "2026-11-25T10:00:00Z", "live"),
      bItem("upcoming_1", "2026-11-27T11:00:00Z", "upcoming"),
      bItem("live_new", "2026-11-27T09:00:00Z", "live"),
      bItem("live_null", null, "live"),
    ].sort(feedItemCompare);
    expect(sorted.map((i) => i.videoId)).toEqual([
      "live_new",
      "live_old",
      "live_null",
      "upcoming_1",
      "vod_new",
    ]);
  });

  it("feedItemSortsAfter は isLive 境界を跨いで正しく判定する", () => {
    const liveCursor = {
      publishedAt: "2026-11-27T09:00:00Z",
      videoId: "live1",
      isLive: true,
    };
    expect(
      feedItemSortsAfter(
        bItem("vod_newer", "2026-11-27T12:00:00Z", null),
        liveCursor,
      ),
    ).toBe(true);
    expect(
      feedItemSortsAfter(
        bItem("live_newer", "2026-11-27T12:00:00Z", "live"),
        liveCursor,
      ),
    ).toBe(false);
  });
});

describe("FP-05: broadcastDisplayOf", () => {
  const now = new Date(2026, 10, 27, 12, 0);
  const startMs = new Date(2026, 10, 27, 19, 0).getTime();

  it("live は開始時刻があれば started、無ければ published、それも無ければ none", () => {
    const d1 = broadcastDisplayOf(
      {
        broadcastState: "live",
        broadcastStartMs: startMs,
        publishedAt: "2026-11-27T09:00:00Z",
      },
      now,
    );
    expect(d1.badge).toBe("live");
    expect(d1.timeKind).toBe("started");
    expect(d1.formattedTime).toContain("19:00");

    const d2 = broadcastDisplayOf(
      {
        broadcastState: "live",
        broadcastStartMs: null,
        publishedAt: "2026-11-27T09:00:00Z",
      },
      now,
    );
    expect(d2.badge).toBe("live");
    expect(d2.timeKind).toBe("published");
    expect(d2.formattedTime).not.toBe("");

    const d3 = broadcastDisplayOf(
      {
        broadcastState: "live",
        broadcastStartMs: 0,
        publishedAt: null,
      },
      now,
    );
    expect(d3).toEqual({ badge: "live", timeKind: "none", formattedTime: "" });
  });

  it("upcoming は予定時刻があれば scheduled、無ければ scheduledUnknown", () => {
    const d1 = broadcastDisplayOf(
      {
        broadcastState: "upcoming",
        broadcastStartMs: startMs,
        publishedAt: "2026-11-27T09:00:00Z",
      },
      now,
    );
    expect(d1.badge).toBe("upcoming");
    expect(d1.timeKind).toBe("scheduled");
    expect(d1.formattedTime).toContain("19:00");

    const d2 = broadcastDisplayOf(
      {
        broadcastState: "upcoming",
        broadcastStartMs: null,
        publishedAt: "2026-11-27T09:00:00Z",
      },
      now,
    );
    expect(d2).toEqual({
      badge: "upcoming",
      timeKind: "scheduledUnknown",
      formattedTime: "",
    });
  });

  it("ended / null はバッジなしで投稿日時のみ表示する", () => {
    const d1 = broadcastDisplayOf(
      {
        broadcastState: "ended",
        broadcastStartMs: startMs,
        publishedAt: "2026-11-27T09:00:00Z",
      },
      now,
    );
    expect(d1.badge).toBeNull();
    expect(d1.timeKind).toBe("published");

    const d2 = broadcastDisplayOf(
      {
        broadcastState: null,
        broadcastStartMs: null,
        publishedAt: null,
      },
      now,
    );
    expect(d2).toEqual({ badge: null, timeKind: "none", formattedTime: "" });
  });
});

describe("FP-06: mergeFeedReload と mergeFeedLoadMore", () => {
  const row = (
    videoId: string,
    publishedAt: string | null,
    broadcastState: string | null = null,
  ) => ({ videoId, publishedAt, broadcastState });

  it("状態遷移した動画が次ページへ現れても重複排除し全順序へ再整列する", () => {
    const prevItems = [
      row("v_live_was", "2026-11-27T08:00:00Z", "live"),
      row("v_old", "2026-11-27T09:00:00Z", null),
    ];
    const res = [
      row("v_live_was", "2026-11-27T08:00:00Z", "ended"),
      row("v_newer", "2026-11-27T10:00:00Z", null),
    ];
    const merged = mergeFeedLoadMore({
      res,
      prevItems,
      prevFeedCursor: {
        publishedAt: "2026-11-27T09:00:00Z",
        videoId: "v_old",
        isLive: false,
      },
      gapBoundary: null,
      deepCursor: null,
      deepHasMore: false,
      pageSize: 2,
    });
    expect(merged.items.map((i) => i.videoId)).toEqual([
      "v_live_was",
      "v_newer",
      "v_old",
    ]);
    expect(merged.feedHasMore).toBe(true);
  });

  it("再取得時に過去ページ保持とギャップ復帰が isLive 付きで動作する", () => {
    const p1 = [row("l1", "2026-11-27T05:00:00Z", "live")];
    const p2 = [row("v1", "2026-11-27T10:00:00Z", null)];
    const reloaded = mergeFeedReload({
      res: [row("l2", "2026-11-27T06:00:00Z", "live")],
      prevItems: [...p1, ...p2],
      sameFilter: true,
      prevFeedCursor: {
        publishedAt: "2026-11-27T10:00:00Z",
        videoId: "v1",
        isLive: false,
      },
      prevFeedHasMore: false,
      prevGapBoundary: null,
      prevDeepCursor: null,
      prevDeepHasMore: false,
      pageSize: 1,
    });
    expect(reloaded.items.map((i) => i.videoId)).toEqual(["l2", "l1", "v1"]);
    expect(reloaded.gapBoundary).toEqual({
      publishedAt: "2026-11-27T05:00:00Z",
      videoId: "l1",
      isLive: true,
    });

    const filled = mergeFeedLoadMore({
      res: [row("l1", "2026-11-27T05:00:00Z", "live")],
      prevItems: reloaded.items,
      prevFeedCursor: reloaded.feedCursor,
      gapBoundary: reloaded.gapBoundary,
      deepCursor: reloaded.deepCursor,
      deepHasMore: reloaded.deepHasMore,
      pageSize: 1,
    });
    expect(filled.gapBoundary).toBeNull();
    expect(filled.feedCursor).toEqual({
      publishedAt: "2026-11-27T10:00:00Z",
      videoId: "v1",
      isLive: false,
    });
    expect(filled.feedHasMore).toBe(false);
  });
});
