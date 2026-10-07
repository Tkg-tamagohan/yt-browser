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

/// `sponsor://skipped` イベント（設計書 §3.2）。action は "skip" | "notify"。
export type SponsorSkipped = {
  instanceId: number;
  videoId: string;
  category: string;
  segment: [number, number];
  action: "skip" | "notify";
};

export type WatchHistory = {
  videoId: string;
  title: string;
  channelId: string | null;
  channelTitle: string | null;
  positionSec: number;
  durationSec: number | null;
  lastWatchedAt: string;
  completed: boolean;
};

/// `favorite_add` / `playlist_add` の入力（検索・関連・フィード行のメタを渡す）。
export type VideoRef = {
  videoId: string;
  title: string;
  channelId: string | null;
  channelTitle: string | null;
  thumbnailUrl: string | null;
};

/// `favorite_list` の 1 行（FR-7）。
export type FavoriteEntry = {
  videoId: string;
  title: string;
  channelId: string | null;
  channelTitle: string | null;
  thumbnailUrl: string | null;
  addedAt: string;
};

/// `playlist_list` の 1 行（FR-7）。
export type Playlist = {
  id: number;
  name: string;
  sortOrder: number;
  itemCount: number;
};

/// `playlist_items` の 1 行（FR-7）。
export type PlaylistEntry = {
  position: number;
  videoId: string;
  title: string;
  channelId: string | null;
  channelTitle: string | null;
  thumbnailUrl: string | null;
};

export type UiError = { code: string; message: string };

export type YtDlpStatus = { path: string | null; version: string | null };

export type DbStatus = { schemaVersion: number };

export type SearchResult = {
  videoId: string;
  title: string;
  channelId: string | null;
  // @handle 形の投稿者 ID。ブロックキーには使えないが購読入力には使える
  uploaderId: string | null;
  channelTitle: string | null;
  durationSec: number | null;
  viewCount: number | null;
  thumbnailUrl: string | null;
};

export type BlockedChannel = {
  channelId: string;
  title: string;
  createdAt: string;
};

/// `chat://message` バッチの 1 要素（設計書 §3.1 の ChatEvent）。
/// `rawJson` は UI へ送られない（直列化省略）。
export type ChatEvent = {
  /// InnerTube のアイテム ID。削除イベントはこの値ではなく
  /// `message` に対象の item ID が入る。
  itemId: string;
  videoId: string;
  postedAtUsec: number;
  authorChannelId: string | null;
  authorName: string | null;
  kind: "text" | "superchat" | "membership" | "deleted" | "other";
  message: string;
  amountDisplay: string | null;
  /// NG フィルタで非表示判定されたもの。保存・送信されるが UI は出さない。
  ng: boolean;
};

/// `chat://status` イベント（設計書 §3.2）。
export type ChatStatus = {
  videoId: string | null;
  level: "info" | "warn" | "error" | string;
  message: string;
};

/// `filters` テーブルの 1 行（NG フィルタ）。
export type Filter = {
  id: number;
  target: string;
  kind: string;
  pattern: string;
  enabled: boolean;
  createdAt: string;
};

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
