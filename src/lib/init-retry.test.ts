// ストア初期化の再試行（initPromise リセット・部分購読解除）と
// 破棄競合ガードの回帰テスト。
// IR-01: initBookmarks の失敗後に initPromise がリセットされ再試行できる
// IR-02: initLiveWaits の失敗時に既登録リスナーが解除され再試行できる
// IR-03: initChatEvents / initPlayerEvents の失敗時に部分購読が解除される
// IR-04: initQueue / initQueueEvents の失敗時に部分購読が解除され再試行できる
// IR-05: initDeepLinks / initShortcuts の遅延 resolve が破棄後にリークしない
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";

const { invokeMock, listenMock, gotoMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  listenMock: vi.fn(),
  gotoMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: invokeMock,
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: listenMock,
}));

vi.mock("$app/navigation", () => ({
  goto: gotoMock,
}));

import { bookmarks, initBookmarks } from "./bookmark.svelte";
import { initChatEvents } from "./chat.svelte";
import { initDeepLinks } from "./deeplink.svelte";
import { initLiveWaits, liveWaits } from "./livewait.svelte";
import { initPlayerEvents } from "./players.svelte";
import { initQueue } from "./queue.svelte";
import { initShortcuts } from "./shortcuts.svelte";

async function flushMicrotasks(turns = 6): Promise<void> {
  for (let i = 0; i < turns; i++) {
    await Promise.resolve();
  }
}

beforeEach(() => {
  invokeMock.mockReset();
  listenMock.mockReset();
  gotoMock.mockReset();
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("IR-01 initBookmarks の再試行", () => {
  test("bookmark_list 失敗後に次回呼び出しで再取得できる", async () => {
    invokeMock.mockRejectedValueOnce(new Error("db error"));
    await initBookmarks();
    expect(invokeMock).toHaveBeenCalledTimes(1);

    const row = {
      id: 1,
      videoId: "dQw4w9WgXcQ",
      positionSec: 42,
      label: "0:42",
      createdAt: "2026-11-15T00:00:00Z",
      title: "Sample",
      channelTitle: null,
      thumbnailUrl: null,
    };
    invokeMock.mockResolvedValueOnce([row]);
    await initBookmarks();
    expect(invokeMock).toHaveBeenCalledTimes(2);
    expect(bookmarks.list).toHaveLength(1);
    expect(bookmarks.list[0].videoId).toBe("dQw4w9WgXcQ");
  });
});

describe("IR-02 initLiveWaits のクリーンアップと再試行", () => {
  test("購読途中の失敗と refresh 失敗の両方で既登録リスナーを解除し再試行できる", async () => {
    const u1 = vi.fn();
    listenMock
      .mockResolvedValueOnce(u1)
      .mockRejectedValueOnce(new Error("listen failed"));
    await expect(initLiveWaits()).rejects.toThrow("listen failed");
    expect(u1).toHaveBeenCalledTimes(1);

    const u2a = vi.fn();
    const u2b = vi.fn();
    listenMock.mockResolvedValueOnce(u2a).mockResolvedValueOnce(u2b);
    invokeMock.mockRejectedValueOnce(new Error("list failed"));
    await initLiveWaits();
    expect(u2a).toHaveBeenCalledTimes(1);
    expect(u2b).toHaveBeenCalledTimes(1);

    const u3a = vi.fn();
    const u3b = vi.fn();
    listenMock.mockResolvedValueOnce(u3a).mockResolvedValueOnce(u3b);
    invokeMock.mockResolvedValueOnce([]);
    await initLiveWaits();
    expect(u3a).not.toHaveBeenCalled();
    expect(u3b).not.toHaveBeenCalled();
    expect(liveWaits.list).toEqual([]);
  });
});

describe("IR-03 チャット/プレイヤーイベントの部分購読解除", () => {
  test("initChatEvents の途中失敗で既登録リスナーを解除して再試行できる", async () => {
    const u1 = vi.fn();
    const u2 = vi.fn();
    listenMock
      .mockResolvedValueOnce(u1)
      .mockResolvedValueOnce(u2)
      .mockRejectedValueOnce(new Error("chat listen failed"));
    await expect(initChatEvents()).rejects.toThrow("chat listen failed");
    expect(u1).toHaveBeenCalledTimes(1);
    expect(u2).toHaveBeenCalledTimes(1);

    const active = [vi.fn(), vi.fn(), vi.fn(), vi.fn()];
    for (const u of active) listenMock.mockResolvedValueOnce(u);
    await initChatEvents();
    for (const u of active) expect(u).not.toHaveBeenCalled();
  });

  test("initPlayerEvents の player_list 失敗で全 5 リスナーを解除して再試行できる", async () => {
    const partial = [vi.fn(), vi.fn(), vi.fn(), vi.fn(), vi.fn()];
    for (const u of partial) listenMock.mockResolvedValueOnce(u);
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "settings_get") return Promise.resolve("on");
      if (cmd === "player_list") {
        return Promise.reject(new Error("player_list failed"));
      }
      return Promise.resolve(null);
    });
    await expect(initPlayerEvents()).rejects.toThrow("player_list failed");
    for (const u of partial) expect(u).toHaveBeenCalledTimes(1);

    const active = [vi.fn(), vi.fn(), vi.fn(), vi.fn(), vi.fn()];
    for (const u of active) listenMock.mockResolvedValueOnce(u);
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "settings_get") return Promise.resolve("on");
      if (cmd === "player_list") return Promise.resolve([]);
      return Promise.resolve(null);
    });
    await initPlayerEvents();
    for (const u of active) expect(u).not.toHaveBeenCalled();
  });
});

describe("IR-04 initQueue / initQueueEvents の再試行", () => {
  test("player://ended 購読失敗時に player://state を解除し次回 initQueue で再試行できる", async () => {
    const uState = vi.fn();
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "queue_load") {
        return Promise.resolve({ items: [], currentIndex: 0 });
      }
      if (cmd === "queue_playlist_load") return Promise.resolve([]);
      return Promise.resolve(null);
    });
    listenMock
      .mockResolvedValueOnce(uState)
      .mockRejectedValueOnce(new Error("ended listen failed"));
    await expect(initQueue()).rejects.toThrow("ended listen failed");
    expect(uState).toHaveBeenCalledTimes(1);

    const ok1 = vi.fn();
    const ok2 = vi.fn();
    listenMock.mockResolvedValueOnce(ok1).mockResolvedValueOnce(ok2);
    await initQueue();
    expect(ok1).not.toHaveBeenCalled();
    expect(ok2).not.toHaveBeenCalled();
  });
});

describe("IR-05 initDeepLinks / initShortcuts の破棄後リーク防止", () => {
  test("initDeepLinks は破棄後に解決したイベント購読を即解除し take_open_urls を呼ばない", async () => {
    let resolveListen!: (fn: () => void) => void;
    listenMock.mockImplementationOnce(
      () =>
        new Promise<() => void>((resolve) => {
          resolveListen = resolve;
        }),
    );
    const unlisten = vi.fn();
    const stop = initDeepLinks();
    stop();
    resolveListen(unlisten);
    await flushMicrotasks();

    expect(unlisten).toHaveBeenCalledTimes(1);
    expect(invokeMock).not.toHaveBeenCalled();
  });

  test("initShortcuts は破棄後に解決したイベント購読を即解除する", async () => {
    const addListener = vi.fn();
    const removeListener = vi.fn();
    vi.stubGlobal("window", {
      addEventListener: addListener,
      removeEventListener: removeListener,
    });
    vi.stubGlobal("document", {
      querySelectorAll: () => [],
    });

    let resolveListen!: (fn: () => void) => void;
    listenMock.mockImplementationOnce(
      () =>
        new Promise<() => void>((resolve) => {
          resolveListen = resolve;
        }),
    );
    invokeMock.mockResolvedValueOnce("on");

    const stop = initShortcuts();
    stop();
    const unlisten = vi.fn();
    resolveListen(unlisten);
    await flushMicrotasks();

    expect(addListener).toHaveBeenCalledTimes(1);
    expect(removeListener).toHaveBeenCalledTimes(1);
    expect(unlisten).toHaveBeenCalledTimes(1);
  });
});
