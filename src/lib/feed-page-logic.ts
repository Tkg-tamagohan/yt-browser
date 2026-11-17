/// フィード一覧のページングに関する純粋ロジック（FR-25、仕様決定 AR）。

/// フィード項目の並び順は published_at の降順（NULL は末尾）、
/// 同時刻は video_id 昇順の全順序（db/feed.rs の ORDER BY と同じ）。
/// 追記分の重複除去後に全体を整列するのに使う（保持した過去ページの
/// 手前に属する項目を正しい位置へ挿すため）
export function feedItemCompare(
  a: { publishedAt: string | null; videoId: string },
  b: { publishedAt: string | null; videoId: string },
): number {
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

/// 項目がカーソル位置（publishedAt, videoId）より後にあるかを返す。
/// 更新通知での先頭ページ再取得時に、読み込み済みの過去ページを
/// 保持するかどうかの判定に使う
export function feedItemSortsAfter(
  item: { publishedAt: string | null; videoId: string },
  publishedAt: string | null,
  videoId: string,
): boolean {
  return feedItemCompare(item, { publishedAt, videoId }) > 0;
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
