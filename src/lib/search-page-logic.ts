import type { SearchPage, SearchResult } from "./players.svelte";

export const SEARCH_PAGE = 20;
// バックエンド search の件数上限（SEARCH_COUNT_MAX）に合わせる。
export const SEARCH_MAX = 500;

export type SearchPaginationState = {
  results: SearchResult[];
  pending: SearchResult[];
  shownLimit: number;
  backendHasMore: boolean;
  hasMore: boolean;
};

export function emptySearchPagination(): SearchPaginationState {
  return {
    results: [],
    pending: [],
    shownLimit: 0,
    backendHasMore: false,
    hasMore: false,
  };
}

export function computeSearchHasMore(
  resultsLen: number,
  pendingLen: number,
  shownLimit: number,
  backendHasMore: boolean,
  max = SEARCH_MAX,
): boolean {
  return (
    resultsLen < max &&
    (pendingLen > 0 || (backendHasMore && shownLimit < max))
  );
}

function dedupeSearchItems(items: SearchResult[]): SearchResult[] {
  const seen = new Set<string>();
  const out: SearchResult[] = [];
  for (const item of items) {
    if (seen.has(item.videoId)) continue;
    seen.add(item.videoId);
    out.push(item);
  }
  return out;
}

export function applyInitialSearch(
  page: SearchPage,
  pageSize = SEARCH_PAGE,
  max = SEARCH_MAX,
): SearchPaginationState {
  const unique = dedupeSearchItems(page.items);
  const cap = Math.min(pageSize, max);
  const results = unique.slice(0, cap);
  const pending = unique.slice(cap);
  const shownLimit = Math.min(pageSize, max);
  return {
    results,
    pending,
    shownLimit,
    backendHasMore: page.hasMore,
    hasMore: computeSearchHasMore(
      results.length,
      pending.length,
      shownLimit,
      page.hasMore,
      max,
    ),
  };
}

export function shouldFetchMoreFromBackend(
  state: SearchPaginationState,
  pageSize = SEARCH_PAGE,
  max = SEARCH_MAX,
): boolean {
  if (state.results.length >= max) return false;
  if (state.pending.length >= pageSize) return false;
  return state.backendHasMore && state.shownLimit < max;
}

export function nextSearchCount(
  state: SearchPaginationState,
  pageSize = SEARCH_PAGE,
  max = SEARCH_MAX,
): number {
  return Math.min(state.shownLimit + pageSize, max);
}

export function consumePendingOnly(
  state: SearchPaginationState,
  pageSize = SEARCH_PAGE,
  max = SEARCH_MAX,
): SearchPaginationState {
  const remainingCap = Math.max(0, max - state.results.length);
  const take = Math.min(pageSize, remainingCap);
  const appended = state.pending.slice(0, take);
  const results = [...state.results, ...appended];
  const pending = state.pending.slice(take);
  return {
    results,
    pending,
    shownLimit: state.shownLimit,
    backendHasMore: state.backendHasMore,
    hasMore: computeSearchHasMore(
      results.length,
      pending.length,
      state.shownLimit,
      state.backendHasMore,
      max,
    ),
  };
}

export function applyLoadMoreSearch(
  state: SearchPaginationState,
  page: SearchPage,
  requestedCount: number,
  pageSize = SEARCH_PAGE,
  max = SEARCH_MAX,
): SearchPaginationState {
  const seen = new Set<string>();
  for (const r of state.results) seen.add(r.videoId);
  for (const p of state.pending) seen.add(p.videoId);

  const freshUnseen: SearchResult[] = [];
  for (const item of page.items) {
    if (seen.has(item.videoId)) continue;
    seen.add(item.videoId);
    freshUnseen.push(item);
  }

  const combined = [...state.pending, ...freshUnseen];
  const remainingCap = Math.max(0, max - state.results.length);
  const take = Math.min(pageSize, remainingCap);
  const results = [...state.results, ...combined.slice(0, take)];
  const pending = combined.slice(take);
  const shownLimit = Math.min(requestedCount, max);

  return {
    results,
    pending,
    shownLimit,
    backendHasMore: page.hasMore,
    hasMore: computeSearchHasMore(
      results.length,
      pending.length,
      shownLimit,
      page.hasMore,
      max,
    ),
  };
}

export function removeBlockedChannelFromSearch(
  state: SearchPaginationState,
  channelId: string,
  max = SEARCH_MAX,
): SearchPaginationState {
  const results = state.results.filter((x) => x.channelId !== channelId);
  const pending = state.pending.filter((x) => x.channelId !== channelId);
  return {
    ...state,
    results,
    pending,
    hasMore: computeSearchHasMore(
      results.length,
      pending.length,
      state.shownLimit,
      state.backendHasMore,
      max,
    ),
  };
}
