// プレイリスト同期（FR-101、仕様決定 EG）を中心とした library-page 状態管理のテスト。
// PS-01: syncPlaylist 成功（追加あり・!plHasMore）で va.playlists 更新・項目再取得・追加件数トーストを出す
// PS-02: syncPlaylist は addedCount===0 で再取得せず最新トーストを出し、plHasMore===true 時は再取得をスキップする
// PS-03: syncPlaylist は多重実行ガード・選択切替時の上書き防止・エラー時の syncingId 解除・削除および一括操作との競合抑止を行う
// PS-04: syncPlaylist は大量追加（>100 件）でページングを維持し、空一覧からの同期でも全件または先頭ページを反映する
// PS-05: 100 件ちょうどのプレイリストで末尾削除後に syncPlaylist した場合も既読範囲を再取得して同期行を表示する
import { beforeEach, describe, expect, it, vi } from "vitest";

const { invokeMock, notifyMock, loadLibraryMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  notifyMock: vi.fn(),
  loadLibraryMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: invokeMock,
}));

vi.mock("$lib/notices.svelte", () => ({
  notify: notifyMock,
}));

vi.mock("$lib/library", () => ({
  loadLibrary: loadLibraryMock,
}));

vi.mock("$lib/ui-prefs", () => ({
  navigateOnPlay: vi.fn(),
}));

vi.mock("$lib/queue.svelte", () => ({
  discardQueue: vi.fn(),
  newQueueEntry: vi.fn((videoId: string) => ({ uid: 1, videoId })),
  queueAt: vi.fn(() => undefined),
  rememberMeta: vi.fn(),
  startQueue: vi.fn(),
}));

import { createLibraryPageState } from "./library-page.svelte";
import type {
  Playlist,
  PlaylistEntry,
  PlaylistSyncOutcome,
} from "./players.svelte";
import { setLocale, t } from "./i18n";

function makePlaylist(overrides: Partial<Playlist> = {}): Playlist {
  return {
    id: 1,
    name: "Imported PL",
    sortOrder: 0,
    itemCount: 1,
    sourceListId: "PLsource001",
    syncedAt: "2026-11-27T00:00:00+00:00",
    ...overrides,
  };
}

function makeEntry(position: number, videoId: string): PlaylistEntry {
  return {
    position,
    videoId,
    title: `Video ${videoId}`,
    channelId: "UC0000000000000000000001",
    channelTitle: "Channel",
    thumbnailUrl: null,
    publishedAt: "2026-11-27T00:00:00+00:00",
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

describe("library-page playlist sync", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    notifyMock.mockReset();
    loadLibraryMock.mockReset();
    setLocale("ja");
    loadLibraryMock.mockResolvedValue({
      favIds: new Set<string>(),
      playlists: [],
      watchLaterIds: new Set<string>(),
    });
  });

  it("PS-01 syncPlaylist 成功（追加あり・!plHasMore）で一覧・項目・トーストを更新する", async () => {
    const initialPl = makePlaylist({ id: 10, itemCount: 1 });
    const updatedPl = makePlaylist({
      id: 10,
      itemCount: 3,
      syncedAt: "2026-11-27T12:00:00+00:00",
    });
    const initialItems = [makeEntry(0, "vid00000001")];
    const syncedItems = [
      makeEntry(0, "vid00000001"),
      makeEntry(1, "vid00000002"),
      makeEntry(2, "vid00000003"),
    ];

    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "playlist_items") {
        const calls = invokeMock.mock.calls.filter(
          ([c]) => c === "playlist_items",
        ).length;
        return Promise.resolve(calls === 1 ? initialItems : syncedItems);
      }
      if (cmd === "playlist_sync") {
        const outcome: PlaylistSyncOutcome = {
          playlist: updatedPl,
          addedCount: 2,
        };
        return Promise.resolve(outcome);
      }
      return Promise.resolve(null);
    });

    const lib = createLibraryPageState({ goToPlaylistsTab: vi.fn() });
    lib.va.playlists = [initialPl];
    await lib.selectPlaylist(initialPl);
    expect(lib.playlistItems).toHaveLength(1);
    expect(lib.plHasMore).toBe(false);

    await lib.syncPlaylist();

    expect(lib.syncingId).toBeNull();
    expect(lib.va.playlists[0]).toEqual(updatedPl);
    expect(lib.playlistItems).toEqual(syncedItems);
    expect(lib.plHasMore).toBe(false);
    expect(notifyMock).toHaveBeenCalledWith(
      t("library.playlist.synced", { name: updatedPl.name, count: 2 }),
    );
    expect(invokeMock).toHaveBeenCalledWith("playlist_sync", {
      playlistId: 10,
    });
    expect(invokeMock).toHaveBeenLastCalledWith("playlist_items", {
      playlistId: 10,
      afterPosition: null,
      limit: null,
    });
    lib.dispose();
  });

  it("PS-02 addedCount===0 は項目再取得せず最新通知し、plHasMore===true 時は再取得をスキップする", async () => {
    const pl1 = makePlaylist({ id: 1, itemCount: 1 });
    const pl1After = makePlaylist({
      id: 1,
      itemCount: 1,
      syncedAt: "2026-11-27T13:00:00+00:00",
    });
    const page100 = Array.from({ length: 100 }, (_, i) =>
      makeEntry(i, `vid${String(i).padStart(8, "0")}`),
    );
    const pl2 = makePlaylist({ id: 2, itemCount: 100 });
    const pl2After = makePlaylist({
      id: 2,
      itemCount: 105,
      syncedAt: "2026-11-27T14:00:00+00:00",
    });

    invokeMock.mockImplementation((cmd: string, args?: { playlistId?: number }) => {
      if (cmd === "playlist_items") {
        return Promise.resolve(
          args?.playlistId === 1 ? [makeEntry(0, "vid00000001")] : page100,
        );
      }
      if (cmd === "playlist_sync") {
        if (args?.playlistId === 1) {
          return Promise.resolve({ playlist: pl1After, addedCount: 0 });
        }
        return Promise.resolve({ playlist: pl2After, addedCount: 5 });
      }
      return Promise.resolve(null);
    });

    const lib = createLibraryPageState({ goToPlaylistsTab: vi.fn() });
    lib.va.playlists = [pl1, pl2];

    await lib.selectPlaylist(pl1);
    invokeMock.mockClear();
    await lib.syncPlaylist();

    expect(
      invokeMock.mock.calls.filter(([cmd]) => cmd === "playlist_items"),
    ).toHaveLength(0);
    expect(lib.va.playlists[0].syncedAt).toBe("2026-11-27T13:00:00+00:00");
    expect(notifyMock).toHaveBeenCalledWith(
      t("library.playlist.syncedNoChanges", { name: pl1After.name }),
    );

    notifyMock.mockClear();
    await lib.selectPlaylist(pl2);
    expect(lib.plHasMore).toBe(true);
    invokeMock.mockClear();
    await lib.syncPlaylist();

    expect(
      invokeMock.mock.calls.filter(([cmd]) => cmd === "playlist_items"),
    ).toHaveLength(0);
    expect(lib.va.playlists[1]).toEqual(pl2After);
    expect(notifyMock).toHaveBeenCalledWith(
      t("library.playlist.synced", { name: pl2After.name, count: 5 }),
    );
    lib.dispose();
  });

  it("PS-03 多重実行ガード・選択切替時の項目保護・競合操作抑止・エラー時の syncingId 解除", async () => {
    const plA = makePlaylist({ id: 1, name: "PL A", itemCount: 1 });
    const plB = makePlaylist({ id: 2, name: "PL B", itemCount: 2 });
    const plAAfter = makePlaylist({
      id: 1,
      name: "PL A",
      itemCount: 2,
      syncedAt: "2026-11-27T15:00:00+00:00",
    });
    const entryA0 = makeEntry(0, "vidA0000001");
    const entryB0 = makeEntry(0, "vidB0000001");
    const entryB1 = makeEntry(1, "vidB0000002");
    const syncGate = deferred<PlaylistSyncOutcome>();
    const removeGate = deferred<PlaylistEntry | null>();
    let blockRemove = false;

    invokeMock.mockImplementation((cmd: string, args?: { playlistId?: number }) => {
      if (cmd === "playlist_items") {
        return Promise.resolve(
          args?.playlistId === 1 ? [entryA0] : [entryB0, entryB1],
        );
      }
      if (cmd === "playlist_sync") {
        return syncGate.promise;
      }
      if (cmd === "playlist_remove") {
        return blockRemove ? removeGate.promise : Promise.resolve(entryB0);
      }
      return Promise.resolve(null);
    });

    const lib = createLibraryPageState({ goToPlaylistsTab: vi.fn() });
    lib.va.playlists = [plA, plB];
    await lib.selectPlaylist(plA);

    const first = lib.syncPlaylist();
    expect(lib.syncingId).toBe(1);
    // 多重押下は無視される
    await lib.syncPlaylist();
    expect(
      invokeMock.mock.calls.filter(([cmd]) => cmd === "playlist_sync"),
    ).toHaveLength(1);

    // 同一プレイリスト同期中の削除・ソート・反転は抑止される
    await lib.removeItem(entryA0);
    await lib.sortByPublished();
    await lib.reverseItems();
    expect(
      invokeMock.mock.calls.filter(
        ([cmd]) =>
          cmd === "playlist_remove" ||
          cmd === "playlist_sort" ||
          cmd === "playlist_reverse",
      ),
    ).toHaveLength(0);
    expect(lib.sortBusy).toBe(false);
    expect(lib.playlistItems).toEqual([entryA0]);
    expect(lib.va.playlists[0].itemCount).toBe(1);

    // 同期中に別プレイリスト B へ切替えた場合、B に対する削除・ソートは許可される
    await lib.selectPlaylist(plB);
    expect(lib.selectedId).toBe(2);
    expect(lib.playlistItems[0].videoId).toBe("vidB0000001");

    await lib.removeItem(entryB0);
    expect(
      invokeMock.mock.calls.filter(([cmd]) => cmd === "playlist_remove"),
    ).toHaveLength(1);
    expect(lib.playlistItems).toEqual([entryB1]);
    expect(lib.va.playlists[1].itemCount).toBe(1);

    await lib.sortByPublished();
    expect(
      invokeMock.mock.calls.filter(([cmd]) => cmd === "playlist_sort"),
    ).toHaveLength(1);

    syncGate.resolve({ playlist: plAAfter, addedCount: 1 });
    await first;

    expect(lib.syncingId).toBeNull();
    // 一覧メタは更新されるが、表示中の PL B の項目は上書きされない
    expect(lib.va.playlists[0]).toEqual(plAAfter);
    expect(lib.selectedId).toBe(2);
    expect(lib.playlistItems.map((i) => i.videoId)).toEqual(["vidB0000001", "vidB0000002"]);

    // 削除処理の飛行中は同一プレイリストへの syncPlaylist を抑止し、
    // 削除確定時の ++itemsReq で飛行中の項目再取得を無効化する
    await lib.selectPlaylist(plA);
    const staleLoad = deferred<PlaylistEntry[]>();
    invokeMock.mockImplementationOnce(() => staleLoad.promise);
    const loading = lib.loadMoreItems();

    blockRemove = true;
    const removing = lib.removeItem(entryA0);
    await lib.syncPlaylist();
    expect(
      invokeMock.mock.calls.filter(([cmd]) => cmd === "playlist_sync"),
    ).toHaveLength(1);

    removeGate.resolve(entryA0);
    await removing;
    staleLoad.resolve([makeEntry(9, "vidStale001")]);
    await loading;

    expect(lib.playlistItems).toEqual([]);
    expect(lib.va.playlists[0].itemCount).toBe(1);

    // 失敗時は syncFailed トーストを出し、syncingId を解除する
    invokeMock.mockImplementationOnce(() => Promise.reject(new Error("boom")));
    await lib.syncPlaylist();
    expect(lib.syncingId).toBeNull();
    expect(notifyMock).toHaveBeenLastCalledWith(
      t("library.playlist.syncFailed", { message: "boom" }),
    );
    lib.dispose();
  });

  it("PS-04 大量追加（>100 件）のページング維持と空一覧からの同期反映", async () => {
    const emptyPl = makePlaylist({ id: 30, itemCount: 0 });
    const updatedPl = makePlaylist({
      id: 30,
      itemCount: 120,
      syncedAt: "2026-11-27T16:00:00+00:00",
    });
    const firstPage = Array.from({ length: 100 }, (_, i) =>
      makeEntry(i, `big${String(i).padStart(8, "0")}`),
    );
    const secondPage = Array.from({ length: 20 }, (_, i) =>
      makeEntry(100 + i, `big${String(100 + i).padStart(8, "0")}`),
    );

    invokeMock.mockImplementation(
      (cmd: string, args?: { afterPosition?: number | null; limit?: number | null }) => {
        if (cmd === "playlist_items") {
          if (args?.afterPosition === null && args?.limit === 100 && libInitialized) {
            return Promise.resolve(firstPage);
          }
          if (args?.afterPosition === 99 && args?.limit === 100) {
            return Promise.resolve(secondPage);
          }
          return Promise.resolve([]);
        }
        if (cmd === "playlist_sync") {
          return Promise.resolve({ playlist: updatedPl, addedCount: 120 });
        }
        return Promise.resolve(null);
      },
    );

    let libInitialized = false;
    const lib = createLibraryPageState({ goToPlaylistsTab: vi.fn() });
    lib.va.playlists = [emptyPl];
    await lib.selectPlaylist(emptyPl);
    expect(lib.playlistItems).toEqual([]);
    expect(lib.plHasMore).toBe(false);

    libInitialized = true;
    await lib.syncPlaylist();

    expect(lib.playlistItems).toHaveLength(100);
    expect(lib.plHasMore).toBe(true);

    await lib.loadMoreItems();
    expect(lib.playlistItems).toHaveLength(120);
    expect(lib.plHasMore).toBe(false);
    lib.dispose();
  });

  it("PS-05 100 件ちょうどのプレイリストで末尾削除後に同期した行が一覧に表示される", async () => {
    const initial100 = Array.from({ length: 100 }, (_, i) =>
      makeEntry(i, `exact${String(i).padStart(7, "0")}`),
    );
    const syncedNew = makeEntry(99, "syncedAtPos99");
    const afterDelete99 = initial100.slice(0, 99);
    const afterSync100 = [...afterDelete99, syncedNew];

    const plBefore = makePlaylist({ id: 40, itemCount: 100 });
    const plAfter = makePlaylist({
      id: 40,
      itemCount: 100,
      syncedAt: "2026-11-27T17:00:00+00:00",
    });

    let synced = false;
    invokeMock.mockImplementation(
      (cmd: string, args?: { afterPosition?: number | null; limit?: number | null }) => {
        if (cmd === "playlist_items") {
          if (args?.afterPosition === null && !synced) {
            return Promise.resolve(initial100);
          }
          if (args?.afterPosition === null && synced) {
            return Promise.resolve(afterSync100);
          }
          return Promise.resolve([]);
        }
        if (cmd === "playlist_remove") {
          return Promise.resolve(initial100[99]);
        }
        if (cmd === "playlist_sync") {
          synced = true;
          return Promise.resolve({ playlist: plAfter, addedCount: 1 });
        }
        return Promise.resolve(null);
      },
    );

    const lib = createLibraryPageState({ goToPlaylistsTab: vi.fn() });
    lib.va.playlists = [plBefore];
    await lib.selectPlaylist(plBefore);
    expect(lib.playlistItems).toHaveLength(100);
    expect(lib.plHasMore).toBe(true);

    // 末尾（position=99）を削除したあと同期を実行し、同じ position=99 に追記された行が表示されること
    await lib.removeItem(initial100[99]);
    expect(lib.playlistItems).toHaveLength(99);
    expect(lib.plHasMore).toBe(true);

    await lib.syncPlaylist();

    expect(lib.playlistItems).toHaveLength(100);
    expect(lib.playlistItems[99]).toEqual(syncedNew);
    expect(lib.plHasMore).toBe(false);
    lib.dispose();
  });
});
