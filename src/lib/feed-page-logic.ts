/// フィード一覧のページング・放送状態表示に関する純粋ロジック（FR-25・FR-96、仕様決定 AR・EB）。

import { fmtBroadcastDateTime, fmtDateTime } from "$lib/format";

export type FeedSortKey = {
  broadcastState?: string | null;
  isLive?: boolean;
  publishedAt: string | null;
  videoId: string;
};

export type FeedCursorKey = {
  publishedAt: string | null;
  videoId: string;
  isLive: boolean;
};

export function isFeedItemLive(item: {
  broadcastState?: string | null;
  isLive?: boolean;
}): boolean {
  return item.isLive ?? item.broadcastState === "live";
}

/// フィード項目の並び順はライブ中（`broadcastState === "live"`）を最優先とし、
/// 各グループ内は published_at の降順（NULL は末尾）、同時刻は video_id 昇順の全順序
/// （db/feed.rs の 2 段階 ORDER BY と同じ）。
export function feedItemCompare(a: FeedSortKey, b: FeedSortKey): number {
  const aLive = isFeedItemLive(a);
  const bLive = isFeedItemLive(b);
  if (aLive !== bLive) return aLive ? -1 : 1;
  if (a.publishedAt == null && b.publishedAt == null) {
    return a.videoId < b.videoId ? -1 : a.videoId > b.videoId ? 1 : 0;
  }
  if (a.publishedAt == null) return 1;
  if (b.publishedAt == null) return -1;
  if (a.publishedAt !== b.publishedAt) {
    return a.publishedAt < b.publishedAt ? 1 : -1;
  }
  return a.videoId < b.videoId ? -1 : a.videoId > b.videoId ? 1 : 0;
}

/// 項目がカーソル位置（isLive, publishedAt, videoId）より後にあるかを返す。
/// 更新通知での先頭ページ再取得時に、読み込み済みの過去ページを
/// 保持するかどうかの判定に使う。
export function feedItemSortsAfter(
  item: FeedSortKey,
  publishedAtOrCursor:
    | string
    | null
    | { publishedAt: string | null; videoId: string; isLive?: boolean },
  videoId?: string,
): boolean {
  const cursor =
    typeof publishedAtOrCursor === "object" && publishedAtOrCursor !== null
      ? publishedAtOrCursor
      : {
          publishedAt: publishedAtOrCursor,
          videoId: videoId ?? "",
          isLive: false,
        };
  return feedItemCompare(item, cursor) > 0;
}

export type BroadcastDisplay = {
  badge: "live" | "upcoming" | null;
  timeKind: "started" | "scheduled" | "scheduledUnknown" | "published" | "none";
  formattedTime: string;
};

/// フィード行の放送バッジと時刻表示種別を決める純粋関数（FR-96、仕様決定 EB）。
export function broadcastDisplayOf(
  item: {
    broadcastState: string | null;
    broadcastStartMs: number | null;
    publishedAt: string | null;
  },
  now?: Date,
): BroadcastDisplay {
  if (item.broadcastState === "live") {
    const started = fmtBroadcastDateTime(item.broadcastStartMs, now);
    if (started) {
      return { badge: "live", timeKind: "started", formattedTime: started };
    }
    const pub = fmtDateTime(item.publishedAt);
    if (pub) {
      return { badge: "live", timeKind: "published", formattedTime: pub };
    }
    return { badge: "live", timeKind: "none", formattedTime: "" };
  }
  if (item.broadcastState === "upcoming") {
    const scheduled = fmtBroadcastDateTime(item.broadcastStartMs, now);
    if (scheduled) {
      return {
        badge: "upcoming",
        timeKind: "scheduled",
        formattedTime: scheduled,
      };
    }
    return { badge: "upcoming", timeKind: "scheduledUnknown", formattedTime: "" };
  }
  const pub = fmtDateTime(item.publishedAt);
  if (pub) {
    return { badge: null, timeKind: "published", formattedTime: pub };
  }
  return { badge: null, timeKind: "none", formattedTime: "" };
}

/// 再取得（`feed://kind_updated` 等）時に先頭ページと保持済み過去ページを結合し、
/// 重複排除と全順序再ソートを行う（FR-25・FR-96）。
export function mergeFeedReload<T extends FeedSortKey>(params: {
  res: readonly T[];
  prevItems: readonly T[];
  sameFilter: boolean;
  prevFeedCursor: FeedCursorKey | null;
  prevFeedHasMore: boolean;
  prevGapBoundary: FeedCursorKey | null;
  prevDeepCursor: FeedCursorKey | null;
  prevDeepHasMore: boolean;
  pageSize?: number;
}): {
  items: T[];
  feedCursor: FeedCursorKey | null;
  feedHasMore: boolean;
  gapBoundary: FeedCursorKey | null;
  deepCursor: FeedCursorKey | null;
  deepHasMore: boolean;
} {
  const pageSize = params.pageSize ?? 500;
  const last = params.res[params.res.length - 1];
  const lastCursor: FeedCursorKey | null = last
    ? {
        publishedAt: last.publishedAt,
        videoId: last.videoId,
        isLive: isFeedItemLive(last),
      }
    : null;
  const tail =
    lastCursor && params.res.length >= pageSize && params.sameFilter
      ? params.prevItems.filter((i) => feedItemSortsAfter(i, lastCursor))
      : [];

  let gapBoundary = params.prevGapBoundary;
  let deepCursor = params.prevDeepCursor;
  let deepHasMore = params.prevDeepHasMore;
  if (tail.length > 0) {
    if (!gapBoundary) {
      deepCursor = params.prevFeedCursor;
      deepHasMore = params.prevFeedHasMore;
    }
    const head = tail[0];
    gapBoundary = {
      publishedAt: head.publishedAt,
      videoId: head.videoId,
      isLive: isFeedItemLive(head),
    };
  } else {
    gapBoundary = null;
    deepCursor = null;
    deepHasMore = false;
  }

  const seen = new Set(params.res.map((i) => i.videoId));
  const items = [
    ...params.res,
    ...tail.filter((i) => !seen.has(i.videoId)),
  ].sort(feedItemCompare);

  return {
    items,
    feedCursor: lastCursor,
    feedHasMore: params.res.length >= pageSize,
    gapBoundary,
    deepCursor,
    deepHasMore,
  };
}

/// 追記（「さらに読み込む」）時に状態遷移由来の重複を除外して再ソートし、
/// gapBoundary / deepCursor を更新する（FR-25・FR-96）。
export function mergeFeedLoadMore<T extends FeedSortKey>(params: {
  res: readonly T[];
  prevItems: readonly T[];
  prevFeedCursor: FeedCursorKey | null;
  gapBoundary: FeedCursorKey | null;
  deepCursor: FeedCursorKey | null;
  deepHasMore: boolean;
  pageSize?: number;
}): {
  items: T[];
  feedCursor: FeedCursorKey | null;
  feedHasMore: boolean;
  gapBoundary: FeedCursorKey | null;
  deepCursor: FeedCursorKey | null;
  deepHasMore: boolean;
} {
  const pageSize = params.pageSize ?? 500;
  const incomingIds = new Set(params.res.map((i) => i.videoId));
  const items = [
    ...params.prevItems.filter((i) => !incomingIds.has(i.videoId)),
    ...params.res,
  ].sort(feedItemCompare);
  const last = params.res[params.res.length - 1];
  const lastCursor: FeedCursorKey | null = last
    ? {
        publishedAt: last.publishedAt,
        videoId: last.videoId,
        isLive: isFeedItemLive(last),
      }
    : null;

  if (
    lastCursor &&
    params.gapBoundary &&
    feedItemCompare(lastCursor, params.gapBoundary) >= 0
  ) {
    return {
      items,
      feedCursor: params.deepCursor ?? lastCursor,
      feedHasMore: params.deepHasMore,
      gapBoundary: null,
      deepCursor: null,
      deepHasMore: false,
    };
  }

  const feedHasMore = params.res.length >= pageSize;
  return {
    items,
    feedCursor: lastCursor ?? params.prevFeedCursor,
    feedHasMore,
    gapBoundary: feedHasMore ? params.gapBoundary : null,
    deepCursor: feedHasMore ? params.deepCursor : null,
    deepHasMore: feedHasMore ? params.deepHasMore : false,
  };
}

/// 指定動画を一覧上で既読化し、未読のみ表示中なら一覧から取り除く
/// （FR-20・FR-92、仕様決定 DU）。
export function markFeedItemRead<T extends { videoId: string; isRead: boolean }>(
  items: readonly T[],
  videoId: string,
  unreadOnly: boolean,
): T[] {
  if (unreadOnly) {
    return items.filter((i) => i.videoId !== videoId);
  }
  return items.map((i) =>
    i.videoId === videoId && !i.isRead ? { ...i, isRead: true } : i,
  );
}
