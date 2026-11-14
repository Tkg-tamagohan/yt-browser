/// 履歴一覧のページングと検索絞り込みに関する純粋ロジック
/// （FR-52・FR-61、仕様決定 CC・CL）。

/// 「さらに読み込む」で取得した次ページを既表示一覧へ追記する。
/// カーソルは一覧の全順序を厳密に進むため重複は起きないはずだが、
/// フィード一覧の追記と同じ方針で video_id の重複は除いておく
export function mergeHistoryPage<T extends { videoId: string }>(
  current: T[],
  page: T[],
): T[] {
  const seen = new Set(current.map((h) => h.videoId));
  return [...current, ...page.filter((h) => !seen.has(h.videoId))];
}
