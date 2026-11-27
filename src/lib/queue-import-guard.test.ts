// データインポート中のプレイリストキュー永続化ガードと IPC 連携テスト（FR-94、仕様決定 DX）。
// QS-04: ガード中の連続再生遷移がインポート失敗時に IPC へリプレイされ、成功時は破棄される
// QS-05: 単独ループの全体 → 1 項目切替で同一動画に同一 uid を維持し、解除で破棄する（FR-95）
import { beforeEach, describe, expect, test, vi } from "vitest";

const { invokeMock, listenMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  listenMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: invokeMock,
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: listenMock,
}));

import {
  beginImportQueueGuard,
  clearLoop,
  cycleLoop,
  endImportQueueGuard,
  initQueue,
  queues,
  refreshQueues,
} from "./queue.svelte";
import { playerStates } from "./players.svelte";

async function flushMicrotasks(turns = 8): Promise<void> {
  for (let i = 0; i < turns; i++) {
    await Promise.resolve();
  }
}

describe("QS-04 プレイリストキュー永続化ガードの IPC 連携（FR-94、仕様決定 DX）", () => {
  const listeners = new Map<string, (ev: { payload: unknown }) => void>();
  let playlistLoaded = true;
  let mockedOnce = false;

  beforeEach(async () => {
    invokeMock.mockReset();
    playlistLoaded = true;

    if (!mockedOnce) {
      mockedOnce = true;
      listenMock.mockImplementation(
        (event: string, cb: (ev: { payload: unknown }) => void) => {
          listeners.set(event, cb);
          return Promise.resolve(vi.fn());
        },
      );
    }
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "queue_load") {
        return Promise.resolve({ items: [], currentIndex: 0 });
      }
      if (cmd === "queue_playlist_load") {
        return Promise.resolve(
          playlistLoaded
            ? [{ playlistId: 7, playlistName: "PL7", currentIndex: 0 }]
            : [],
        );
      }
      if (cmd === "playlist_items") {
        return Promise.resolve([
          {
            videoId: "vidA0000001",
            title: "A",
            channelId: null,
            channelTitle: "CH",
            thumbnailUrl: null,
            position: 0,
          },
          {
            videoId: "vidB0000002",
            title: "B",
            channelId: null,
            channelTitle: "CH",
            thumbnailUrl: null,
            position: 1,
          },
          {
            videoId: "vidC0000003",
            title: "C",
            channelId: null,
            channelTitle: "CH",
            thumbnailUrl: null,
            position: 2,
          },
        ]);
      }
      if (cmd === "player_set_queue") {
        return Promise.resolve(true);
      }
      return Promise.resolve(null);
    });

    await initQueue();
    // 2 回目以降のテストでも playlistId=7 のキューを取り直して初期状態に揃える
    if (!queues.list.has(7)) {
      await refreshQueues();
    }
    const q = queues.list.get(7)!;
    q.index = 0;
    q.instanceId = 101;
    invokeMock.mockClear();
  });

  test("インポート中に連続再生が次項目へ進み、その後インポートが失敗した場合は最後の位置が queue_playlist_save IPC へ届く", async () => {
    await beginImportQueueGuard();

    const q = queues.list.get(7)!;
    const ended = listeners.get("player://ended");
    expect(ended).toBeDefined();

    ended!({
      payload: {
        instanceId: 101,
        videoId: "vidA0000001",
        reason: "eof",
        continued: true,
        continuedVideoId: "vidB0000002",
        continuedUid: q.items[1].uid,
        armedSeq: 1,
        closed: false,
      },
    });
    await flushMicrotasks();

    // ガード中はメモリ上の位置だけ進み、queue_playlist_save IPC はまだ呼ばれない
    expect(q.index).toBe(1);
    expect(
      invokeMock.mock.calls.filter(([cmd]) => cmd === "queue_playlist_save"),
    ).toHaveLength(0);

    // インポート失敗時のガード解除で、保留されていた最後の位置が世代ガードを通って IPC へ保存される
    endImportQueueGuard(false);
    await flushMicrotasks();

    expect(
      invokeMock.mock.calls.filter(([cmd]) => cmd === "queue_playlist_save"),
    ).toEqual([["queue_playlist_save", { playlistId: 7, currentIndex: 1 }]]);
  });

  test("インポート中に連続再生が次項目へ進み、その後インポートが成功した場合は保留分を破棄して refreshQueues が旧キューを掃除する", async () => {
    await beginImportQueueGuard();

    const q = queues.list.get(7)!;
    const ended = listeners.get("player://ended");
    expect(ended).toBeDefined();

    ended!({
      payload: {
        instanceId: 101,
        videoId: "vidB0000002",
        reason: "eof",
        continued: true,
        continuedVideoId: "vidC0000003",
        continuedUid: q.items[2].uid,
        armedSeq: 2,
        closed: false,
      },
    });
    await flushMicrotasks();
    expect(q.index).toBe(2);

    // インポート側のプレイリスト置換後を想定し、queue_playlist_load は空を返す
    playlistLoaded = false;
    await refreshQueues();
    endImportQueueGuard(true);
    await flushMicrotasks();

    expect(
      invokeMock.mock.calls.filter(([cmd]) => cmd === "queue_playlist_save"),
    ).toHaveLength(0);
    expect(
      invokeMock.mock.calls.filter(([cmd]) => cmd === "player_set_queue"),
    ).toEqual([
      [
        "player_set_queue",
        { instanceId: 101, items: [], loopAll: false, baseSeq: 2 },
      ],
    ]);
    expect(queues.list.has(7)).toBe(false);
  });
});

describe("QS-05 単独ループのモード切替で同一動画の uid を維持する（FR-95、仕様決定 EA）", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "player_set_queue") return Promise.resolve(true);
      return Promise.resolve(undefined);
    });
    clearLoop(202);
    const next = new Map(playerStates.list);
    next.set(202, {
      instanceId: 202,
      videoId: "vidSolo0001",
      pause: false,
      position: 0,
      duration: 60,
      fps: 30,
      state: "playing",
      volume: 100,
      speed: 1,
      mediaTitle: "Solo",
      pip: false,
      format: "best",
      audioDevice: "auto",
      subTracks: [],
      sid: "auto",
    });
    playerStates.list = next;
  });

  test("none → all → one の切替で同じ uid を引き継ぎ、none 解除後は新しい uid を払い出す", async () => {
    // none → all
    await cycleLoop(202);
    // all → one
    await cycleLoop(202);
    // one → none
    await cycleLoop(202);
    // none → all（再有効化）
    await cycleLoop(202);

    const setCalls = invokeMock.mock.calls
      .filter(([cmd]) => cmd === "player_set_queue")
      .map(([, args]) => args as { items: Array<{ uid: number; videoId: string }>; loopAll: boolean });

    expect(setCalls).toHaveLength(4);
    expect(setCalls[0].loopAll).toBe(true);
    expect(setCalls[1].loopAll).toBe(true);
    expect(setCalls[0].items[0].videoId).toBe("vidSolo0001");
    expect(setCalls[1].items[0].videoId).toBe("vidSolo0001");
    // all → one で同じ uid を維持する
    expect(setCalls[1].items[0].uid).toBe(setCalls[0].items[0].uid);
    // none で武装解除
    expect(setCalls[2]).toMatchObject({ items: [], loopAll: false });
    // 再有効化では新しい uid が払い出される
    expect(setCalls[3].items[0].uid).not.toBe(setCalls[0].items[0].uid);

    clearLoop(202);
    const clean = new Map(playerStates.list);
    clean.delete(202);
    playerStates.list = clean;
  });
});
