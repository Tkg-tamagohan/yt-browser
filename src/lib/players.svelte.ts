// プレイヤー状態の共有ストア（設計書 §3.2 のイベント経路）。
// 複数ページ（再生・設定）で同じマップを参照するため、イベント購読は一度だけ初期化する。

import { invoke } from "@tauri-apps/api/core";
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
  /// PiP（最前面・枠なしの小窓）表示中かどうか（設計書 §4.5）。
  pip: boolean;
  /// 現在適用中の画質式（`ytdl-format`）。
  format: string;
};

export type PlayerEnded = {
  instanceId: number;
  videoId: string;
  reason: string;
  /// 連続再生で同一インスタンスが次項目へ進んだとき true（FR-10、仕様決定 S）
  continued: boolean;
  /// 継続先として読み込みを開始した項目（FR-16、仕様決定 AA）。
  /// videoId と同一なら現在項目の繰り返し（ループ）。非継続時は null
  continuedVideoId: string | null;
};

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
  /// 投稿日時（フィード投入済み項目のみ。未取得は null、ソート時は末尾）
  publishedAt: string | null;
};

export type UiError = { code: string; message: string };

/// invoke の失敗を通知表示用の文字列にする。UiError は message を持つので
/// それを取り出し、それ以外は文字列化する。
export function asErrorMessage(e: unknown): string {
  if (typeof e === "object" && e !== null && "message" in e) {
    return String((e as UiError).message);
  }
  return String(e);
}

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
  | { type: "frame_back_step" }
  | { type: "pip"; enabled: boolean };

export const playerStates = $state<{
  list: Map<number, PlayerState>;
}>({ list: new Map() });

/// インスタンス別画質を指定済みの instanceId 集合（仕様決定 X。セッション内のみ有効）。
/// プレイヤーカードの画質選択が成功した時点で登録し、インスタンス終了で解放する。
/// 設定画面の全体画質の即時適用はこの集合のインスタンスを除外する
/// （個別指定が全体既定で消えないようにする）
export const formatOverrides = new Set<number>();

/// 再生インスタンスの終了・手動クローズ直後に呼び出すページ側フック
/// （resumeHint の再取得など）。PlayerCards はレイアウトで常時マウントのため
/// ページのローカル状態を直接参照できず、+page はマウント中だけここに登録する。
export const playbackHooks = new Set<() => void>();

let initPromise: Promise<void> | null = null;

/// `player://state` / `player://ended` を購読して共有マップを更新する。
/// ページをまたいで利用するため、最初の onMount で一度だけ登録する。
export function initPlayerEvents(): Promise<void> {
  if (!initPromise) {
    initPromise = (async () => {
      // player_list の応答を待つ間に届いたイベントの instanceId を記録する。
      // スナップショットは取得時点の値なので、飛行中に更新・終了が起きた
      // インスタンスはイベント側を優先し、古い値で上書きしない。
      const inFlight = new Set<number>();
      await listen<PlayerState>("player://state", (ev) => {
        inFlight.add(ev.payload.instanceId);
        const next = new Map(playerStates.list);
        next.set(ev.payload.instanceId, ev.payload);
        playerStates.list = next;
      });
      await listen<PlayerEnded>("player://ended", (ev) => {
        inFlight.add(ev.payload.instanceId);
        // 連続再生で次項目へ進んだインスタンスは稼働中のまま残す
        // （FR-10、仕様決定 S。カードが消えずに新しい状態へ移行する）
        // continuedVideoId は実際に読み込みが始まった項目なので、
        // 定期の状態イベントを待たずに再生中項目だけはここで同期する
        // （遷移直後の操作が古い項目を拾わないように）
        if (ev.payload.continued) {
          const cur = playerStates.list.get(ev.payload.instanceId);
          if (cur && ev.payload.continuedVideoId) {
            const next = new Map(playerStates.list);
            next.set(ev.payload.instanceId, {
              ...cur,
              videoId: ev.payload.continuedVideoId,
            });
            playerStates.list = next;
          }
          return;
        }
        const next = new Map(playerStates.list);
        next.delete(ev.payload.instanceId);
        playerStates.list = next;
        formatOverrides.delete(ev.payload.instanceId);
      });
      // リロード後はイベントが来ない一時停止中インスタンスがあるため、
      // 登録直後に一覧を取得してカードを復元する。
      const snapshot = await invoke<PlayerState[]>("player_list");
      const next = new Map(playerStates.list);
      for (const s of snapshot) {
        if (!inFlight.has(s.instanceId)) next.set(s.instanceId, s);
      }
      playerStates.list = next;
    })();
  }
  return initPromise;
}
