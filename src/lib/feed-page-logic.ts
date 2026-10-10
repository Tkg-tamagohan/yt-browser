/// フィード一覧のページングに関する純粋ロジック（FR-25、仕様決定 AR）。

/// フィード項目の並び順は published_at の降順（NULL は末尾）、
/// 同時刻は video_id 昇順の全順序（db/feed.rs のカーソル比較と同じ）。
/// 項目がカーソル位置（publishedAt, videoId）より後にあるかを返す。
/// 更新通知での先頭ページ再取得時に、読み込み済みの過去ページを
/// 保持するかどうかの判定に使う
export function feedItemSortsAfter(
  item: { publishedAt: string | null; videoId: string },
  publishedAt: string | null,
  videoId: string,
): boolean {
  if (publishedAt == null) {
    return item.publishedAt == null && item.videoId > videoId;
  }
  return (
    item.publishedAt == null ||
    item.publishedAt < publishedAt ||
    (item.publishedAt === publishedAt && item.videoId > videoId)
  );
}
