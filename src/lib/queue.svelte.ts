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
import { armTargetFor, reconcileIndex } from "./queue-logic";

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

// `player_set_next` の直列化（インスタンスごと）。
// invoke の到着順は保証されないため、武装・解除・再武装を一本の
// Promise チェーンに載せ、各操作は実行時点で対象を評価する。
// これにより最後に予約した意図だけが必ず backend へ届く。
const armRuns = new Map<number, Promise<void>>();

/// インスタンスへの `player_set_next` を直列化して送る。
/// `target` は実行時点で評価し、送信順序をチェーンで固定する。
function enqueueArm(
  instanceId: number,
  target: () => string | null,
): Promise<void> {
  const prev = armRuns.get(instanceId) ?? Promise.resolve();
  const run = prev.then(() =>
    invoke("player_set_next", {
      instanceId,
      videoId: target(),
    }).then(() => undefined),
  );
  // 後続のチェーンは失敗に関わらず進める（失敗処理は呼び出し側の catch）
  armRuns.set(instanceId, run.catch(() => {}));
  return run;
}

/// ループ状態を なし → 全体 → 1 項目 の順に切り替える（仕様決定 AA）。
/// 切り替え時点でそのインスタンスの武装を新しいモードの対象へ張り替える。
export async function cycleLoop(instanceId: number): Promise<void> {
  const cur = loopMode(instanceId);
  const next: LoopMode =
    cur === "none" ? "all" : cur === "all" ? "one" : "none";
  setLoopMode(instanceId, next);
  try {
    if (instanceId === queue.instanceId) {
      await enqueueArm(instanceId, () =>
        armTargetFor(loopMode(instanceId), queue.items, queue.index),
      );
      return;
    }
    // キュー外インスタンス: none 以外は現在項目を武装（繰り返し）。none は解除
    await enqueueArm(instanceId, () =>
      loopMode(instanceId) === "none"
        ? null
        : (playerStates.list.get(instanceId)?.videoId ?? null),
    );
  } catch {
    // インスタンスが既に無い場合はキュー・ループ状態を畳む
    if (instanceId === queue.instanceId) stopQueue();
    clearLoop(instanceId);
  }
}

/// 次項目をバックエンドへ武装する。ループモードに応じて対象を選ぶ
/// （対象の決定は armTargetFor、LP-NN 系回帰テストで検証）。
async function armNext(): Promise<void> {
  if (queue.instanceId === null) return;
  const id = queue.instanceId;
  try {
    await enqueueArm(id, () =>
      armTargetFor(loopMode(id), queue.items, queue.index),
    );
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

/// キューを畳む。ループモードが残るインスタンスは単独ループへ移行する
/// （キュー無しの全体/1 項目は現在項目の繰り返し。仕様決定 AA）。
/// なしなら武装を解除して終端で自然に終了する。
export function stopQueue(): void {
  if (queue.instanceId !== null) {
    const id = queue.instanceId;
    const vid = queue.items[queue.index] ?? null;
    queue.instanceId = null;
    void enqueueArm(id, () =>
      loopMode(id) === "none" ? null : vid,
    ).catch(() => {});
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
          // 実際に読み込みが始まった項目で位置を照合する。
          // 遷移とモード変更が重なってもイベント到着時のモードは
          // 直前の遷移を表さないため、位置はロード済み項目で決める
          queue.index = reconcileIndex(
            queue.items,
            queue.index,
            p.continuedVideoId,
          );
          void armNext();
          return;
        }
        // キュー外インスタンスの繰り返し（ループで武装した項目）。
        // モードが継続していれば次周回のために再武装する
        if (loopMode(p.instanceId) !== "none") {
          const vid = p.continuedVideoId ?? p.videoId;
          void enqueueArm(p.instanceId, () => vid).catch(() =>
            clearLoop(p.instanceId),
          );
        }
      });
    })();
  }
  return initPromise;
}
