// プレイヤー状態の共有ストア（設計書 §3.2 のイベント経路）。
// 複数ページ（再生・設定）で同じマップを参照するため、イベント購読は一度だけ初期化する。

import { listen } from "@tauri-apps/api/event";

export type PlayStatus = "idle" | "playing" | "paused" | "buffering" | "ended";

export type PlayerState = {
  instanceId: number;
  videoId: string;
  pause: boolean;
  position: number;
  duration: number;
  fps: number;
  state: PlayStatus;
  volume: number;
  speed: number;
  mediaTitle: string;
};

export type PlayerEnded = { instanceId: number; videoId: string; reason: string };

export type WatchHistory = {
  videoId: string;
  title: string;
  positionSec: number;
  durationSec: number | null;
  completed: boolean;
};

export type UiError = { code: string; message: string };

export type YtDlpStatus = { path: string | null; version: string | null };

export type DbStatus = { schemaVersion: number };

export type PlayerAction =
  | { type: "pause"; value: boolean }
  | { type: "seek"; seconds: number }
  | { type: "volume"; value: number }
  | { type: "speed"; value: number }
  | { type: "quality"; format: string }
  | { type: "frame_step" }
  | { type: "frame_back_step" };

export const playerStates = $state<{
  list: Map<number, PlayerState>;
}>({ list: new Map() });

let initPromise: Promise<void> | null = null;

/// `player://state` / `player://ended` を購読して共有マップを更新する。
/// ページをまたいで利用するため、最初の onMount で一度だけ登録する。
export function initPlayerEvents(): Promise<void> {
  if (!initPromise) {
    initPromise = (async () => {
      await listen<PlayerState>("player://state", (ev) => {
        const next = new Map(playerStates.list);
        next.set(ev.payload.instanceId, ev.payload);
        playerStates.list = next;
      });
      await listen<PlayerEnded>("player://ended", (ev) => {
        const next = new Map(playerStates.list);
        next.delete(ev.payload.instanceId);
        playerStates.list = next;
      });
    })();
  }
  return initPromise;
}
