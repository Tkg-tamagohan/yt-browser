// プレイリスト同期（FR-101、仕様決定 EG）を中心とした library-page 状態管理のテスト。
// PS-01: syncPlaylist 成功（追加あり・!plHasMore）で va.playlists 更新・項目再取得・追加件数トーストを出す
// PS-02: syncPlaylist は addedCount===0 で再取得せず最新トーストを出し、plHasMore===true 時は再取得をスキップする
// PS-03: syncPlaylist は多重実行ガード・選択切替時の上書き防止・エラー時の syncingId 解除・削除および一括操作との競合抑止を行う
// PS-04: syncPlaylist は大量追加（>100 件）でページングを維持し、空一覧からの同期でも全件または先頭ページを反映する
// PS-05: 100 件ちょうどのプレイリストで末尾削除後に syncPlaylist した場合も既読範囲を再取得して同期行を表示し、再取得中・取得中の Undo も整合する
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

    // 同期の再取得中に取り消しを実行しても syncReloadPromise 完了まで UI 反映を待機し、
    // 再取得（syncedPos99）と復元（orig99）の双方が欠落しないこと
    invokeMock.mockReset();
    notifyMock.mockReset();
    const raceInitial = Array.from({ length: 100 }, (_, i) =>
      makeEntry(i, i === 99 ? "orig99" : `race${String(i).padStart(7, "0")}`),
    );
    const orig99 = raceInitial[99];
    const syncedPos99 = makeEntry(99, "syncedPos99");
    const restoredOrig99 = makeEntry(100, "orig99");
    const raceAfterSync = [...raceInitial.slice(0, 99), syncedPos99];
    const racePlBefore = makePlaylist({ id: 41, itemCount: 100 });
    const racePlAfter = makePlaylist({
      id: 41,
      itemCount: 100,
      syncedAt: "2026-11-27T18:00:00+00:00",
    });

    const reloadDef = deferred<PlaylistEntry[]>();
    const restoreDef = deferred<PlaylistEntry>();
    let raceSynced = false;
    let syncCallCount = 0;
    let restoreCallCount = 0;

    invokeMock.mockImplementation(
      (cmd: string, args?: { afterPosition?: number | null }) => {
        if (cmd === "playlist_items") {
          if (args?.afterPosition === null && !raceSynced) {
            return Promise.resolve(raceInitial);
          }
          if (args?.afterPosition === null && raceSynced) {
            return reloadDef.promise;
          }
          return Promise.resolve([]);
        }
        if (cmd === "playlist_remove") {
          return Promise.resolve(orig99);
        }
        if (cmd === "playlist_sync") {
          syncCallCount += 1;
          raceSynced = true;
          return Promise.resolve({ playlist: racePlAfter, addedCount: 1 });
        }
        if (cmd === "playlist_item_restore") {
          restoreCallCount += 1;
          return restoreDef.promise;
        }
        return Promise.resolve(null);
      },
    );

    const raceLib = createLibraryPageState({ goToPlaylistsTab: vi.fn() });
    raceLib.va.playlists = [racePlBefore];
    await raceLib.selectPlaylist(racePlBefore);
    expect(raceLib.playlistItems).toHaveLength(100);
    expect(raceLib.plHasMore).toBe(true);

    await raceLib.removeItem(orig99);
    const undoAction = notifyMock.mock.calls.at(-1)?.[1] as
      | { label?: string; run?: () => void }
      | undefined;
    expect(undoAction?.run).toBeTypeOf("function");

    // syncPlaylist が playlist_sync 完了後に playlist_items 再取得を待機している間に Undo を発火する
    const syncTask = raceLib.syncPlaylist();
    await vi.waitFor(() => expect(raceSynced).toBe(true));

    undoAction!.run!();
    await vi.waitFor(() => expect(restoreCallCount).toBe(1));
    expect(raceLib.restoring).toBe(true);

    // 復元 IPC が返っても syncReloadPromise が終わるまで ++itemsReq せず待機し、
    // 復元中の再同期も抑止される
    restoreDef.resolve(restoredOrig99);
    await Promise.resolve();
    await Promise.resolve();
    expect(raceLib.playlistItems).toHaveLength(99);
    await raceLib.syncPlaylist();
    expect(syncCallCount).toBe(1);

    reloadDef.resolve(raceAfterSync);
    await syncTask;
    await vi.waitFor(() => expect(raceLib.playlistItems).toHaveLength(101));
    expect(raceLib.playlistItems[99]).toEqual(syncedPos99);
    expect(raceLib.playlistItems[100]).toEqual(restoredOrig99);
    expect(raceLib.va.playlists[0].itemCount).toBe(101);
    expect(raceLib.plHasMore).toBe(false);
    expect(raceLib.restoring).toBe(false);
    raceLib.dispose();

    // 同期のリモート取得中に中間削除を取り消したときは同期完了を待たずに復元を開始し、
    // 同期が並走完了して同じ動画が登録済みでも itemCount を二重加算しないこと
    invokeMock.mockReset();
    notifyMock.mockReset();
    const midA = makeEntry(0, "midA0000001");
    const midB = makeEntry(1, "midB0000002");
    const midC = makeEntry(2, "midC0000003");
    const midPlBefore = makePlaylist({ id: 42, itemCount: 3 });
    const midPlAfterSync = makePlaylist({
      id: 42,
      itemCount: 3,
      syncedAt: "2026-11-27T18:10:00+00:00",
    });
    const midSyncDef = deferred<PlaylistSyncOutcome>();
    const midReloadDef = deferred<PlaylistEntry[]>();
    const midRestoreDef = deferred<PlaylistEntry>();
    let midSynced = false;
    let midRestoreCalls = 0;

    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "playlist_items") {
        return midSynced
          ? midReloadDef.promise
          : Promise.resolve([midA, midB, midC]);
      }
      if (cmd === "playlist_remove") {
        return Promise.resolve(midB);
      }
      if (cmd === "playlist_sync") {
        midSynced = true;
        return midSyncDef.promise;
      }
      if (cmd === "playlist_item_restore") {
        midRestoreCalls += 1;
        return midRestoreDef.promise;
      }
      if (cmd === "playlist_list") {
        return Promise.resolve([midPlAfterSync]);
      }
      return Promise.resolve(null);
    });

    const midLib = createLibraryPageState({ goToPlaylistsTab: vi.fn() });
    midLib.va.playlists = [midPlBefore];
    await midLib.selectPlaylist(midPlBefore);
    await midLib.removeItem(midB);
    const midUndo = notifyMock.mock.calls.at(-1)?.[1] as
      | { label?: string; run?: () => void }
      | undefined;

    const midSyncTask = midLib.syncPlaylist();
    midUndo!.run!();
    await vi.waitFor(() => expect(midRestoreCalls).toBe(1));
    expect(midLib.restoring).toBe(true);

    midSyncDef.resolve({ playlist: midPlAfterSync, addedCount: 1 });
    await vi.waitFor(() =>
      expect(
        invokeMock.mock.calls.filter(([c]) => c === "playlist_items").length,
      ).toBe(2),
    );
    midReloadDef.resolve([midA, midC, makeEntry(3, "midB0000002")]);
    await midSyncTask;

    midRestoreDef.resolve(makeEntry(3, "midB0000002"));
    await vi.waitFor(() => expect(midLib.restoring).toBe(false));
    expect(midLib.playlistItems.map((i) => i.videoId)).toEqual([
      "midA0000001",
      "midC0000003",
      "midB0000002",
    ]);
    expect(midLib.playlistItems[2].position).toBe(3);
    expect(midLib.va.playlists[0].itemCount).toBe(3);
    midLib.dispose();

    // 同期のリモート取得中に削除取り消しが先に確定したときは元の position=1 へ戻り、
    // 後続の同期完了後も元の並び（0, 1, 2）と件数 3 が保たれること
    invokeMock.mockReset();
    notifyMock.mockReset();
    const undoFirstSyncDef = deferred<PlaylistSyncOutcome>();
    const undoFirstRestoreDef = deferred<PlaylistEntry>();
    let undoFirstSyncCalls = 0;
    let undoFirstRestoreCalls = 0;

    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "playlist_items") {
        return Promise.resolve([midA, midB, midC]);
      }
      if (cmd === "playlist_remove") {
        return Promise.resolve(midB);
      }
      if (cmd === "playlist_sync") {
        undoFirstSyncCalls += 1;
        return undoFirstSyncDef.promise;
      }
      if (cmd === "playlist_item_restore") {
        undoFirstRestoreCalls += 1;
        return undoFirstRestoreDef.promise;
      }
      return Promise.resolve(null);
    });

    const undoFirstLib = createLibraryPageState({ goToPlaylistsTab: vi.fn() });
    undoFirstLib.va.playlists = [midPlBefore];
    await undoFirstLib.selectPlaylist(midPlBefore);
    await undoFirstLib.removeItem(midB);
    const undoFirstAction = notifyMock.mock.calls.at(-1)?.[1] as
      | { label?: string; run?: () => void }
      | undefined;

    const undoFirstSyncTask = undoFirstLib.syncPlaylist();
    await vi.waitFor(() => expect(undoFirstSyncCalls).toBe(1));
    undoFirstAction!.run!();
    await vi.waitFor(() => expect(undoFirstRestoreCalls).toBe(1));

    undoFirstRestoreDef.resolve(midB);
    await vi.waitFor(() => expect(undoFirstLib.restoring).toBe(false));
    expect(undoFirstLib.playlistItems.map((i) => i.videoId)).toEqual([
      "midA0000001",
      "midB0000002",
      "midC0000003",
    ]);
    expect(undoFirstLib.playlistItems.map((i) => i.position)).toEqual([0, 1, 2]);
    expect(undoFirstLib.va.playlists[0].itemCount).toBe(3);

    undoFirstSyncDef.resolve({ playlist: midPlAfterSync, addedCount: 0 });
    await undoFirstSyncTask;
    expect(undoFirstLib.playlistItems.map((i) => i.videoId)).toEqual([
      "midA0000001",
      "midB0000002",
      "midC0000003",
    ]);
    expect(undoFirstLib.playlistItems.map((i) => i.position)).toEqual([0, 1, 2]);
    expect(undoFirstLib.va.playlists[0].itemCount).toBe(3);
    expect(undoFirstLib.va.playlists[0].syncedAt).toBe(midPlAfterSync.syncedAt);
    undoFirstLib.dispose();

    // 大量同期（addedCount=150 で plHasMore===true 維持）の再取得中に Undo しても
    // plTailPos を未読末尾の先（249）へ進めないため、loadMoreItems() が 199..248 を飛ばさないこと
    invokeMock.mockReset();
    notifyMock.mockReset();
    const bigInitial = Array.from({ length: 100 }, (_, i) =>
      makeEntry(i, i === 99 ? "bigOrig99" : `bigR${String(i).padStart(7, "0")}`),
    );
    const bigOrig99 = bigInitial[99];
    const bigSyncedItems = Array.from({ length: 150 }, (_, i) =>
      makeEntry(99 + i, `bigS${String(i).padStart(7, "0")}`),
    );
    const bigRestoredOrig99 = makeEntry(249, "bigOrig99");
    const bigReloadPage = [...bigInitial.slice(0, 99), ...bigSyncedItems.slice(0, 100)];
    const bigTailPage = [...bigSyncedItems.slice(100), bigRestoredOrig99];
    const bigPlBefore = makePlaylist({ id: 43, itemCount: 100 });
    const bigPlAfterSync = makePlaylist({
      id: 43,
      itemCount: 249,
      syncedAt: "2026-11-27T18:20:00+00:00",
    });
    const bigReloadDef = deferred<PlaylistEntry[]>();
    const bigRestoreDef = deferred<PlaylistEntry>();
    let bigSynced = false;
    let bigRestoreCalls = 0;

    invokeMock.mockImplementation(
      (cmd: string, args?: { afterPosition?: number | null }) => {
        if (cmd === "playlist_items") {
          if (args?.afterPosition === null && !bigSynced) {
            return Promise.resolve(bigInitial);
          }
          if (args?.afterPosition === null && bigSynced) {
            return bigReloadDef.promise;
          }
          if (args?.afterPosition === 198) {
            return Promise.resolve(bigTailPage);
          }
          return Promise.resolve([]);
        }
        if (cmd === "playlist_remove") {
          return Promise.resolve(bigOrig99);
        }
        if (cmd === "playlist_sync") {
          bigSynced = true;
          return Promise.resolve({ playlist: bigPlAfterSync, addedCount: 150 });
        }
        if (cmd === "playlist_item_restore") {
          bigRestoreCalls += 1;
          return bigRestoreDef.promise;
        }
        return Promise.resolve(null);
      },
    );

    const bigLib = createLibraryPageState({ goToPlaylistsTab: vi.fn() });
    bigLib.va.playlists = [bigPlBefore];
    await bigLib.selectPlaylist(bigPlBefore);
    await bigLib.removeItem(bigOrig99);
    const bigUndo = notifyMock.mock.calls.at(-1)?.[1] as
      | { label?: string; run?: () => void }
      | undefined;

    const bigSyncTask = bigLib.syncPlaylist();
    await vi.waitFor(() => expect(bigSynced).toBe(true));

    bigUndo!.run!();
    await vi.waitFor(() => expect(bigRestoreCalls).toBe(1));
    bigRestoreDef.resolve(bigRestoredOrig99);
    await Promise.resolve();

    bigReloadDef.resolve(bigReloadPage);
    await bigSyncTask;
    await vi.waitFor(() => expect(bigLib.restoring).toBe(false));

    expect(bigLib.playlistItems).toHaveLength(199);
    expect(bigLib.playlistItems[198].position).toBe(198);
    expect(bigLib.plHasMore).toBe(true);
    expect(bigLib.va.playlists[0].itemCount).toBe(250);

    await bigLib.loadMoreItems();
    expect(bigLib.playlistItems).toHaveLength(250);
    expect(bigLib.playlistItems[199].position).toBe(199);
    expect(bigLib.playlistItems[249]).toEqual(bigRestoredOrig99);
    expect(bigLib.plHasMore).toBe(false);
    bigLib.dispose();

    // 逆順（Undo 開始後に再取得が復元より先に完了し syncReloadPromise === null に戻ったあと復元が完了）でも
    // initialReload により plTailPos を 249 へ進めず、loadMoreItems() で 199..248 を飛ばさないこと
    const bigRevReloadDef = deferred<PlaylistEntry[]>();
    const bigRevRestoreDef = deferred<PlaylistEntry>();
    let bigRevSynced = false;
    let bigRevRestoreCalls = 0;

    invokeMock.mockReset();
    notifyMock.mockReset();
    invokeMock.mockImplementation(
      (cmd: string, args?: { afterPosition?: number | null }) => {
        if (cmd === "playlist_items") {
          if (args?.afterPosition === null && !bigRevSynced) {
            return Promise.resolve(bigInitial);
          }
          if (args?.afterPosition === null && bigRevSynced) {
            return bigRevReloadDef.promise;
          }
          if (args?.afterPosition === 198) {
            return Promise.resolve(bigTailPage);
          }
          return Promise.resolve([]);
        }
        if (cmd === "playlist_remove") {
          return Promise.resolve(bigOrig99);
        }
        if (cmd === "playlist_sync") {
          bigRevSynced = true;
          return Promise.resolve({ playlist: bigPlAfterSync, addedCount: 150 });
        }
        if (cmd === "playlist_item_restore") {
          bigRevRestoreCalls += 1;
          return bigRevRestoreDef.promise;
        }
        return Promise.resolve(null);
      },
    );

    const bigRevLib = createLibraryPageState({ goToPlaylistsTab: vi.fn() });
    bigRevLib.va.playlists = [bigPlBefore];
    await bigRevLib.selectPlaylist(bigPlBefore);
    await bigRevLib.removeItem(bigOrig99);
    const bigRevUndo = notifyMock.mock.calls.at(-1)?.[1] as
      | { label?: string; run?: () => void }
      | undefined;

    const bigRevSyncTask = bigRevLib.syncPlaylist();
    await vi.waitFor(() => expect(bigRevSynced).toBe(true));

    bigRevUndo!.run!();
    await vi.waitFor(() => expect(bigRevRestoreCalls).toBe(1));

    bigRevReloadDef.resolve(bigReloadPage);
    await bigRevSyncTask;
    expect(bigRevLib.syncingId).toBeNull();

    bigRevRestoreDef.resolve(bigRestoredOrig99);
    await vi.waitFor(() => expect(bigRevLib.restoring).toBe(false));

    expect(bigRevLib.playlistItems).toHaveLength(199);
    expect(bigRevLib.playlistItems[198].position).toBe(198);
    expect(bigRevLib.plHasMore).toBe(true);
    expect(bigRevLib.va.playlists[0].itemCount).toBe(250);

    await bigRevLib.loadMoreItems();
    expect(bigRevLib.playlistItems).toHaveLength(250);
    expect(bigRevLib.playlistItems[199].position).toBe(199);
    expect(bigRevLib.playlistItems[249]).toEqual(bigRestoredOrig99);
    expect(bigRevLib.plHasMore).toBe(false);
    bigRevLib.dispose();

    // syncReloadPromise が失敗しても、playlist_item_restore 成功分のローカル一覧反映と件数更新は維持され、
    // restoreFailed トーストを出さないこと
    invokeMock.mockReset();
    notifyMock.mockReset();
    const failPlBefore = makePlaylist({ id: 44, itemCount: 3 });
    const failPlAfterSync = makePlaylist({
      id: 44,
      itemCount: 3,
      syncedAt: "2026-11-27T18:30:00+00:00",
    });
    const failReloadDef = deferred<PlaylistEntry[]>();
    const failRestoreDef = deferred<PlaylistEntry>();
    let failSynced = false;
    let failRestoreCalls = 0;

    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "playlist_items") {
        return failSynced
          ? failReloadDef.promise
          : Promise.resolve([midA, midB, midC]);
      }
      if (cmd === "playlist_remove") {
        return Promise.resolve(midC);
      }
      if (cmd === "playlist_sync") {
        failSynced = true;
        return Promise.resolve({ playlist: failPlAfterSync, addedCount: 1 });
      }
      if (cmd === "playlist_item_restore") {
        failRestoreCalls += 1;
        return failRestoreDef.promise;
      }
      return Promise.resolve(null);
    });

    const failLib = createLibraryPageState({ goToPlaylistsTab: vi.fn() });
    failLib.va.playlists = [failPlBefore];
    await failLib.selectPlaylist(failPlBefore);
    await failLib.removeItem(midC);
    const failUndo = notifyMock.mock.calls.at(-1)?.[1] as
      | { label?: string; run?: () => void }
      | undefined;

    notifyMock.mockClear();
    const failSyncTask = failLib.syncPlaylist();
    await vi.waitFor(() => expect(failSynced).toBe(true));

    failUndo!.run!();
    await vi.waitFor(() => expect(failRestoreCalls).toBe(1));
    const failRestoredC = makeEntry(3, "midC0000003");
    failRestoreDef.resolve(failRestoredC);
    await Promise.resolve();

    failReloadDef.reject(new Error("reload failed"));
    await failSyncTask;
    await vi.waitFor(() => expect(failLib.restoring).toBe(false));

    expect(failLib.playlistItems.map((i) => i.videoId)).toEqual([
      "midA0000001",
      "midB0000002",
      "midC0000003",
    ]);
    expect(failLib.va.playlists[0].itemCount).toBe(4);
    expect(failLib.syncingId).toBeNull();
    expect(notifyMock).toHaveBeenCalledWith(t("library.restored"));
    expect(notifyMock).toHaveBeenCalledWith(
      t("library.playlist.syncFailed", { message: "reload failed" }),
    );
    expect(notifyMock).not.toHaveBeenCalledWith(
      t("library.restoreFailed", { message: "reload failed" }),
    );
    failLib.dispose();

    // 同期が Undo より先に完了（syncingId === null）してから残存トーストの Undo を押した場合でも、
    // 元の位置（99）より先の末尾（249）へ復元された項目を plHasMore === true の既読ページへ差し込まず、
    // plTailPos = 198 を維持して loadMoreItems() が 199..249 を欠落なく読み込むこと
    invokeMock.mockReset();
    notifyMock.mockReset();
    let postSyncSynced = false;
    const postSyncAfterPositions: Array<number | null | undefined> = [];

    invokeMock.mockImplementation(
      (cmd: string, args?: { afterPosition?: number | null }) => {
        if (cmd === "playlist_items") {
          postSyncAfterPositions.push(args?.afterPosition);
          if (args?.afterPosition === null && !postSyncSynced) {
            return Promise.resolve(bigInitial);
          }
          if (args?.afterPosition === null && postSyncSynced) {
            return Promise.resolve(bigReloadPage);
          }
          if (args?.afterPosition === 198) {
            return Promise.resolve(bigTailPage);
          }
          return Promise.resolve([]);
        }
        if (cmd === "playlist_remove") {
          return Promise.resolve(bigOrig99);
        }
        if (cmd === "playlist_sync") {
          postSyncSynced = true;
          return Promise.resolve({ playlist: bigPlAfterSync, addedCount: 150 });
        }
        if (cmd === "playlist_item_restore") {
          return Promise.resolve(bigRestoredOrig99);
        }
        return Promise.resolve(null);
      },
    );

    const postSyncLib = createLibraryPageState({ goToPlaylistsTab: vi.fn() });
    postSyncLib.va.playlists = [bigPlBefore];
    await postSyncLib.selectPlaylist(bigPlBefore);
    await postSyncLib.removeItem(bigOrig99);
    const postSyncUndo = notifyMock.mock.calls.at(-1)?.[1] as
      | { label?: string; run?: () => void }
      | undefined;

    await postSyncLib.syncPlaylist();
    expect(postSyncLib.syncingId).toBeNull();
    expect(postSyncLib.playlistItems).toHaveLength(199);
    expect(postSyncLib.playlistItems[198].position).toBe(198);
    expect(postSyncLib.plHasMore).toBe(true);

    postSyncUndo!.run!();
    await vi.waitFor(() => expect(postSyncLib.restoring).toBe(false));

    expect(postSyncLib.playlistItems).toHaveLength(199);
    expect(postSyncLib.playlistItems[198].position).toBe(198);
    expect(postSyncLib.plHasMore).toBe(true);
    expect(postSyncLib.va.playlists[0].itemCount).toBe(250);

    await postSyncLib.loadMoreItems();
    expect(postSyncAfterPositions.at(-1)).toBe(198);
    expect(postSyncLib.playlistItems).toHaveLength(250);
    expect(postSyncLib.playlistItems[199].position).toBe(199);
    expect(postSyncLib.playlistItems[249]).toEqual(bigRestoredOrig99);
    expect(postSyncLib.plHasMore).toBe(false);
    postSyncLib.dispose();
  });
});
