// ライブ配信の待機（FR-44、仕様決定 BT）の純粋ロジック。
// 放送状態の分類・ポーリング間隔・24 時間失効の判定はバックエンド
// （src-tauri/src/livewait/mod.rs）と同一ルールの TS 実装で、
// 待機の状態遷移を Vitest で検証するためここに置く。
// 一覧の並び・待機判定・登録結果 → 通知キーのマッピングは UI 側が使う。

import type { MessageKey } from "./i18n";

/// `live_waits` の 1 行（`live_wait_list` の戻り値要素）。
export type LiveWait = {
  videoId: string;
  title: string;
  channelId: string | null;
  channelTitle: string | null;
  thumbnailUrl: string | null;
  /// 予定開始（epoch ms）。不明は null。
  scheduledStartMs: number | null;
  /// 登録時刻（epoch ms）。
  registeredMs: number;
};

/// `live_wait_add` の結果区分。added 以外は登録されない。
export type WaitAddOutcome =
  | { result: "added"; scheduledStartMs: number | null }
  | { result: "duplicate" }
  | { result: "live" }
  | { result: "ended" }
  | { result: "notBroadcast" };

/// `livewait://changed` のペイロード。
export type WaitChanged = { videoId: string | null };

/// `livewait://status` のペイロード。
export type WaitStatusEvent = {
  videoId: string | null;
  level: "info" | "warn" | "error" | string;
  message: string;
};

/// InnerTube `player` 応答の放送状態（バックエンドの BroadcastStatus と同形）。
export type BroadcastStatus = {
  isLiveNow: boolean | null;
  startMs: number | null;
  endMs: number | null;
  scheduledStartMs: number | null;
  hasDetails: boolean;
  isUpcoming: boolean;
};

/// `live_wait.action` の動作。未設定・その他の値は play。
export type WaitAction = "play" | "notify";

// ポーリングポリシーの定数（Rust 側と同一値に保つ）。
/// 予定時刻の無い項目と直前窓の外にある項目の定間隔（仕様決定 BT「60 秒程度」）。
export const WAIT_POLL_BASE_MS = 60_000;
/// 直前窓内の短間隔（暫定 15 秒）。
export const WAIT_POLL_SHORT_MS = 15_000;
/// 直前窓の手前側。予定開始のこの時間前から短間隔へ（暫定 2 分）。
export const WAIT_NEAR_BEFORE_MS = 120_000;
/// 直前窓の後側。予定超過後も遅延開始を短間隔で追う時間（暫定 30 分）。
export const WAIT_NEAR_AFTER_MS = 30 * 60_000;
/// 開始予定からの自動解除期限（仕様決定 BT「24 時間超過」）。
export const WAIT_EXPIRY_MS = 24 * 60 * 60_000;

/// 待機対象の状態分類（バックエンドの WaitBroadcast と同形）。
/// 優先順位は 放送中 > 終了済み > 開始前 > 非放送。
export type WaitBroadcast =
  | { kind: "upcoming"; scheduledMs: number | null }
  | { kind: "live" }
  | { kind: "ended" }
  | { kind: "notBroadcast" };

/// `player` 応答から待機対象の状態を分類する。
/// `isLiveNow` 真は `endTimestamp` が混在する矛盾形でも放送中とする。
/// 予定時刻は `scheduledStartTimestamp` を優先し、無ければ
/// `startTimestamp` を使う（実応答ではこちらに予定が入る。暫定解釈）。
export function classifyWaitBroadcast(s: BroadcastStatus): WaitBroadcast {
  if (s.isLiveNow === true) return { kind: "live" };
  if (s.endMs !== null) return { kind: "ended" };
  if (s.hasDetails || s.isUpcoming) {
    return {
      kind: "upcoming",
      scheduledMs: s.scheduledStartMs ?? s.startMs,
    };
  }
  return { kind: "notBroadcast" };
}

/// 次回ポーリングまでの間隔（ms）。予定時刻が直前窓内なら短間隔、
/// それ以外（予定無しを含む）は定間隔（仕様決定 BT）。
export function nextWaitPollMs(
  nowMs: number,
  scheduledMs: number | null,
): number {
  if (scheduledMs === null) return WAIT_POLL_BASE_MS;
  if (
    nowMs >= scheduledMs - WAIT_NEAR_BEFORE_MS &&
    nowMs <= scheduledMs + WAIT_NEAR_AFTER_MS
  ) {
    return WAIT_POLL_SHORT_MS;
  }
  return WAIT_POLL_BASE_MS;
}

/// 開始予定から 24 時間を超えて未開始の項目の自動解除判定（仕様決定 BT）。
/// 予定の無い項目は登録時刻を起点とする（暫定解釈）。
export function isWaitExpired(
  nowMs: number,
  scheduledMs: number | null,
  registeredMs: number,
): boolean {
  return nowMs > (scheduledMs ?? registeredMs) + WAIT_EXPIRY_MS;
}

/// `live_wait.action` の値の解釈。未設定・その他の値は `play`（既定）。
export function waitActionOf(value: string | null): WaitAction {
  return value === "notify" ? "notify" : "play";
}

/// 同一動画が既に待機中か（重複抑止の表示側判定）。
export function isWaiting(
  waits: readonly { videoId: string }[],
  videoId: string,
): boolean {
  return waits.some((w) => w.videoId === videoId);
}

/// 待機一覧の表示順。予定時刻の昇順（未定は末尾）→ 登録時刻昇順。
/// `live_wait_list` の DB 順と同じ規則（描画側でも順序を明示するため）。
export function liveWaitCompare(a: LiveWait, b: LiveWait): number {
  if (a.scheduledStartMs === null && b.scheduledStartMs === null) {
    return a.registeredMs - b.registeredMs || a.videoId.localeCompare(b.videoId);
  }
  if (a.scheduledStartMs === null) return 1;
  if (b.scheduledStartMs === null) return -1;
  return (
    a.scheduledStartMs - b.scheduledStartMs ||
    a.registeredMs - b.registeredMs ||
    a.videoId.localeCompare(b.videoId)
  );
}

/// 予定時刻の表示。未定は空文字（呼び出し側で代替表示）。
export function fmtWaitStart(ms: number | null): string {
  if (ms === null) return "";
  return new Date(ms).toLocaleString("ja-JP");
}

/// 登録結果 → 通知メッセージキー。
export function waitAddOutcomeKey(
  result: WaitAddOutcome["result"],
): MessageKey {
  switch (result) {
    case "added":
      return "livewait.added";
    case "duplicate":
      return "livewait.duplicate";
    case "live":
      return "livewait.live";
    case "ended":
      return "livewait.ended";
    case "notBroadcast":
      return "livewait.notBroadcast";
  }
}
