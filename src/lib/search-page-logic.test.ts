import { describe, expect, it } from "vitest";
import type { SearchResult } from "./players.svelte";
import {
  applyInitialSearch,
  applyLoadMoreSearch,
  consumePendingOnly,
  nextSearchCount,
  removeBlockedChannelFromSearch,
  SEARCH_MAX,
  SEARCH_PAGE,
  shouldFetchMoreFromBackend,
} from "./search-page-logic";

// SR-01: 2 ページ上限や NG フィルタで要求件数未満でも backendHasMore=true なら hasMore を維持し追加取得できる
// SR-02: 公式 API から yt-dlp フォールバックへ切り替わり重複が部分的でも 1 回の追記は増分と残り枠に制限される
// SR-03: 未表示バッファが 1 ページ分以上残る場合はバックエンドを呼ばずに消費し終端で閉じる
// SR-04: チャンネルブロック時は表示済み結果と未表示バッファの双方から除外する

const item = (id: string, channelId = "UC0000000000000000000001"): SearchResult => ({
  videoId: id,
  title: `Title ${id}`,
  channelId,
  uploaderId: null,
  channelTitle: "Channel",
  durationSec: 60,
  viewCount: 100,
  thumbnailUrl: null,
});

describe("SR-01: applyInitialSearch / hasMore continuation", () => {
  it("要求数 20 件未満の応答でも hasMore=true なら継続でき、終端応答で false になる", () => {
    const s1 = applyInitialSearch({
      items: [item("v1"), item("v2")],
      hasMore: true,
    });
    expect(s1.results.map((r) => r.videoId)).toEqual(["v1", "v2"]);
    expect(s1.hasMore).toBe(true);
    expect(shouldFetchMoreFromBackend(s1)).toBe(true);
    expect(nextSearchCount(s1)).toBe(40);

    const s2 = applyLoadMoreSearch(
      s1,
      { items: [item("v1"), item("v2"), item("v3")], hasMore: false },
      40,
    );
    expect(s2.results.map((r) => r.videoId)).toEqual(["v1", "v2", "v3"]);
    expect(s2.pending).toEqual([]);
    expect(s2.hasMore).toBe(false);
    expect(shouldFetchMoreFromBackend(s2)).toBe(false);
  });
});

describe("SR-02: backend switch partial overlap capping and SEARCH_MAX bound", () => {
  it("公式 API 40 件から部分重複する yt-dlp 60 件へ切り替わっても 1 回 20 件・上限 500 件を超えない", () => {
    const api40 = Array.from({ length: 40 }, (_, i) => item(`api_${i}`));
    const initial = applyInitialSearch({
      items: api40.slice(0, SEARCH_PAGE),
      hasMore: true,
    });
    const after40 = applyLoadMoreSearch(
      initial,
      { items: api40, hasMore: true },
      40,
    );
    expect(after40.results).toHaveLength(40);

    // yt-dlp が 60 件を返し、先頭 25 件のみ重複し 35 件が新規候補になるケース
    const yt60 = [
      ...api40.slice(0, 25),
      ...Array.from({ length: 35 }, (_, i) => item(`yt_${i}`)),
    ];
    const req60 = nextSearchCount(after40);
    expect(req60).toBe(60);
    const after60 = applyLoadMoreSearch(
      after40,
      { items: yt60, hasMore: true },
      req60,
    );
    expect(after60.results).toHaveLength(60);
    expect(after60.pending).toHaveLength(15);
    expect(after60.hasMore).toBe(true);

    // 残り枠が 10 件（490 件表示済み）の状態で 25 件新規候補が来ても 500 件で打ち切る
    const base490 = Array.from({ length: 490 }, (_, i) => item(`b_${i}`));
    const state490 = {
      results: base490,
      pending: [],
      shownLimit: 480,
      backendHasMore: true,
      hasMore: true,
    };
    const next500 = applyLoadMoreSearch(
      state490,
      {
        items: [
          ...base490.slice(0, 475),
          ...Array.from({ length: 25 }, (_, i) => item(`extra_${i}`)),
        ],
        hasMore: true,
      },
      SEARCH_MAX,
    );
    expect(next500.results).toHaveLength(SEARCH_MAX);
    expect(next500.hasMore).toBe(false);
    expect(shouldFetchMoreFromBackend(next500)).toBe(false);
  });
});

describe("SR-03: pending buffer consumption", () => {
  it("pending が 20 件以上あるときは通信せず 20 件ずつ消費し、枯渇後に終了する", () => {
    const state = {
      results: Array.from({ length: 20 }, (_, i) => item(`r_${i}`)),
      pending: Array.from({ length: 25 }, (_, i) => item(`p_${i}`)),
      shownLimit: 40,
      backendHasMore: false,
      hasMore: true,
    };
    expect(shouldFetchMoreFromBackend(state)).toBe(false);

    const step1 = consumePendingOnly(state);
    expect(step1.results).toHaveLength(40);
    expect(step1.pending).toHaveLength(5);
    expect(step1.hasMore).toBe(true);
    expect(shouldFetchMoreFromBackend(step1)).toBe(false);

    const step2 = consumePendingOnly(step1);
    expect(step2.results).toHaveLength(45);
    expect(step2.pending).toHaveLength(0);
    expect(step2.hasMore).toBe(false);
  });
});

describe("SR-04: removeBlockedChannelFromSearch", () => {
  it("表示済み一覧と pending バッファの双方からブロック対象チャンネルを除外する", () => {
    const state = {
      results: [item("r1", "UCbad"), item("r2", "UCgood")],
      pending: [item("p1", "UCbad")],
      shownLimit: 20,
      backendHasMore: false,
      hasMore: true,
    };
    const updated = removeBlockedChannelFromSearch(state, "UCbad");
    expect(updated.results.map((r) => r.videoId)).toEqual(["r2"]);
    expect(updated.pending).toEqual([]);
    expect(updated.hasMore).toBe(false);
  });
});
