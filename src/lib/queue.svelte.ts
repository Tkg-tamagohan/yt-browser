//! 連続再生キューとループ（FR-10、仕様決定 S・AA）。
//! キューはフロント側のセッション状態として持つ。次項目は `player_set_next`
//! でバックエンドへ事前登録（武装）し、mpv の終端イベントで同一インスタンスが
//! 読み替える。登録済みプレイリストと実際に流れた項目がずれる
//! （queue drift）のは仕様上の制約として許容する。
//! ループ状態もインスタンス別にフロントが持ち、武装対象の選択で実現する
//! （1 項目は現在項目を、全体は末尾到達で先頭を、キュー無しは現在項目を武装）。

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { playerStates, type PlayerEnded } from "$lib/players.svelte";

/// 稼働中キューの状態。`items` は動画 ID 列、`index` は現在の再生位置。
/// `instanceId` はキューを背負うプレイヤーインスタンス。
export const queue = $state<{
  playlistId: number | null;
  playlistName: string;
  items: string[];
  index: number;
  instanceId: number | null;
}>({ playlistId: null, playlistName: "", items: [], index: 0, instanceId: null });

/// ループ状態（仕様決定 AA）。「なし」「プレイリスト全体」「1 項目」の 3 状態。
export type LoopMode = "none" | "all" | "one";

/// インスタンス別のループ状態。未登録は "none" とみなす。
/// セッション内のみ有効（キューと同じく永続化しない）
export const loopModes = $state<{ list: Map<number, LoopMode> }>({
  list: new Map(),
});

/// インスタンスの現在のループ状態。未登録は "none"。
export function loopMode(instanceId: number): LoopMode {
  return loopModes.list.get(instanceId) ?? "none";
}

function setLoopMode(instanceId: number, mode: LoopMode): void {
  const next = new Map(loopModes.list);
  if (mode === "none") next.delete(instanceId);
  else next.set(instanceId, mode);
  loopModes.list = next;
}

/// インスタンス終了・クローズ時にループ状態を外す。
export function clearLoop(instanceId: number): void {
  if (!loopModes.list.has(instanceId)) return;
  setLoopMode(instanceId, "none");
}

/// ループ状態を なし → 全体 → 1 項目 の順に切り替える（仕様決定 AA）。
/// 切り替え時点でそのインスタンスの武装を新しいモードの対象へ張り替える。
export async function cycleLoop(instanceId: number): Promise<void> {
  const cur = loopMode(instanceId);
  const next: LoopMode =
    cur === "none" ? "all" : cur === "all" ? "one" : "none";
  setLoopMode(instanceId, next);
  if (instanceId === queue.instanceId) {
    await armNext();
    return;
  }
  // キュー外インスタンス: none 以外は現在項目を武装（繰り返し）。none は解除
  const videoId =
    next === "none"
      ? null
      : (playerStates.list.get(instanceId)?.videoId ?? null);
  try {
    await invoke("player_set_next", { instanceId, videoId });
  } catch {
    // インスタンスが既に無い場合はループ状態も畳む
    clearLoop(instanceId);
  }
}

/// 次項目をバックエンドへ武装する。ループモードに応じて対象を選ぶ:
/// 1 項目は現在項目、全体は末尾到達で先頭、なしは末尾到達で解除する。
async function armNext(): Promise<void> {
  if (queue.instanceId === null) return;
  const mode = loopMode(queue.instanceId);
  let next: string | null;
  if (mode === "one") {
    next = queue.items[queue.index] ?? null;
  } else if (mode === "all") {
    next = queue.items[queue.index + 1] ?? queue.items[0] ?? null;
  } else {
    next = queue.items[queue.index + 1] ?? null;
  }
  try {
    await invoke("player_set_next", {
      instanceId: queue.instanceId,
      videoId: next,
    });
  } catch {
    // インスタンスが既に無い場合はキューを畳む
    stopQueue();
  }
}

/// キューを開始する。呼び出し側は `items[index]` の再生を別途起動済みで、
/// その instanceId を渡す。終了後は自動で次項目へ進む。
export async function startQueue(
  playlistId: number,
  playlistName: string,
  items: string[],
  index: number,
  instanceId: number,
): Promise<void> {
  queue.playlistId = playlistId;
  queue.playlistName = playlistName;
  queue.items = items;
  queue.index = index;
  queue.instanceId = instanceId;
  await armNext();
}

/// キューを畳む（インスタンスが残る場合は武装も解除する）。
export function stopQueue(): void {
  if (queue.instanceId !== null) {
    const id = queue.instanceId;
    queue.instanceId = null;
    // 解除は応答を待たない（失敗時はインスタンス終了で自然に畳まれる）
    void invoke("player_set_next", { instanceId: id, videoId: null }).catch(
      () => {},
    );
  }
  queue.playlistId = null;
  queue.playlistName = "";
  queue.items = [];
  queue.index = 0;
}

/// キューがこのプレイリストを背負っているか。
export function queueActive(playlistId: number): boolean {
  return queue.playlistId === playlistId;
}

/// 現在の再生位置（その項目が再生中なら true）。
export function queuePlayingAt(videoId: string): boolean {
  return (
    queue.instanceId !== null && queue.items[queue.index] === videoId
  );
}

let initPromise: Promise<void> | null = null;

/// `player://ended` を購読してキューを進め、キュー外インスタンスの
/// ループ繰り返しを再武装する。最初の onMount で一度だけ登録する。
export function initQueueEvents(): Promise<void> {
  if (!initPromise) {
    initPromise = (async () => {
      await listen<PlayerEnded>("player://ended", (ev) => {
        const p = ev.payload;
        if (!p.continued) {
          // 終端（末尾・途中失敗・手動停止）: キューとループ状態を畳む
          if (p.instanceId === queue.instanceId) stopQueue();
          clearLoop(p.instanceId);
          return;
        }
        if (p.instanceId === queue.instanceId) {
          // 武装していた項目が再生開始済み。モードに応じて位置を進めて再武装
          const mode = loopMode(p.instanceId);
          if (mode === "all") {
            queue.index =
              queue.index + 1 >= queue.items.length ? 0 : queue.index + 1;
          } else if (mode !== "one") {
            queue.index += 1;
          }
          // "one" は現在項目の繰り返しなので位置は据え置き
          void armNext();
          return;
        }
        // キュー外インスタンスの繰り返し（ループで武装した項目）。
        // モードが継続していれば次周回のために再武装する
        if (loopMode(p.instanceId) !== "none") {
          void invoke("player_set_next", {
            instanceId: p.instanceId,
            videoId: p.continuedVideoId ?? p.videoId,
          }).catch(() => clearLoop(p.instanceId));
        }
      });
    })();
  }
  return initPromise;
}
