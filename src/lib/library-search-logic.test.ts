// library-search-logic.ts のテスト（Phase 64: LS 系）
import { describe, expect, it } from "vitest";
import {
  normalizeSearchQuery,
  sortSources,
  LIBRARY_SOURCE_ORDER,
} from "./library-search-logic";
import type { LibrarySource } from "./players.svelte";

// LS-01: ヒット区分は正準順（履歴→お気に入り→プレイリスト→あとで見る）に並ぶ
// LS-02: 検索語は前後空白を落とす
// LS-03: 既知外の区分は末尾へ寄せて既知区分の順を維持する
describe("library-search-logic", () => {
  it("LS-01 ヒット区分は正準順に並べ替える", () => {
    expect(sortSources(["watchLater", "history", "playlists"])).toEqual([
      "history",
      "playlists",
      "watchLater",
    ]);
    expect(sortSources([...LIBRARY_SOURCE_ORDER])).toEqual(
      LIBRARY_SOURCE_ORDER,
    );
    // 元の配列は書き換えない
    const src: LibrarySource[] = ["watchLater", "history"];
    sortSources(src);
    expect(src).toEqual(["watchLater", "history"]);
  });

  it("LS-02 検索語は前後の空白を落とす", () => {
    expect(normalizeSearchQuery("  abc  ")).toBe("abc");
    expect(normalizeSearchQuery("")).toBe("");
    expect(normalizeSearchQuery("   ")).toBe("");
    // 途中の空白はそのまま（複数語の部分一致はバックエンドの LIKE 任せ）
    expect(normalizeSearchQuery("a b")).toBe("a b");
  });

  it("LS-03 既知外の区分は末尾へ寄せる", () => {
    const unknown = "futureTab" as LibrarySource;
    expect(sortSources([unknown, "favorites", "history"])).toEqual([
      "history",
      "favorites",
      unknown,
    ]);
  });
});
