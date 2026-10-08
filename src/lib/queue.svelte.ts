//! 連続再生キュー（FR-10、仕様決定 S）。
//! キューはフロント側のセッション状態として持つ。次項目は `player_set_next`
//! でバックエンドへ事前登録（武装）し、mpv の終端イベントで同一インスタンスが
//! 読み替える。登録済みプレイリストと実際に流れた項目がずれる
//! （queue drift）のは仕様上の制約として許容する。

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { PlayerEnded } from "$lib/players.svelte";

/// 稼働中キューの状態。`items` は動画 ID 列、`index` は現在の再生位置。
/// `instanceId` はキューを背負うプレイヤーインスタンス。
export const queue = $state<{
  playlistId: number | null;
  playlistName: string;
  items: string[];
  index: number;
  instanceId: number | null;
}>({ playlistId: null, playlistName: "", items: [], index: 0, instanceId: null });

let initPromise: Promise<void> | null = null;

/// 次項目をバックエンドへ武装する。末尾なら解除する。
async function armNext(): Promise<void> {
  if (queue.instanceId === null) return;
  const next = queue.items[queue.index + 1];
  try {
    await invoke("player_set_next", {
      instanceId: queue.instanceId,
      videoId: next ?? null,
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

/// `player://ended` を購読してキューを進める。最初の onMount で一度だけ登録する。
/// バックエンドが次項目へ進めたとき (`continued`) は index を進めて次々項目を
/// 武装し、普通に終端したときはキューを畳む。
export function initQueueEvents(): Promise<void> {
  if (!initPromise) {
    initPromise = (async () => {
      await listen<PlayerEnded>("player://ended", (ev) => {
        const q = queue;
        if (q.instanceId !== ev.payload.instanceId) return;
        if (ev.payload.continued) {
          // 武装していた次項目が再生開始済み。次の次を武装する
          q.index += 1;
          void armNext();
        } else {
          // 終端（末尾・途中失敗・手動停止）: キューを畳む
          stopQueue();
        }
      });
    })();
  }
  return initPromise;
}
