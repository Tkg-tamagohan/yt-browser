// A-B リピートの状態機械（FR-37、仕様決定 BM）。
// AB ボタンの押下で「なし → A 点設定 → ループ中 → 解除」の 3 状態を
// 巡回し、各遷移に mpv 側へ送る player_control アクションを対応付ける。
// ループは再生中の動画に束ねるため、状態は動画 ID と一緒に保持する。

import type { PlayerAction } from "./players.svelte";

/// A-B リピートの表示状態。off=未設定、a=A 点のみ設定、loop=A-B ループ中。
export type AbState = "off" | "a" | "loop";

/// 押下 1 回分の遷移結果。next は新しい表示状態、action は mpv へ送る操作。
export type AbStep = { next: AbState; action: PlayerAction };

/// AB ボタン押下の遷移。A 点設定 → B 点設定（ループ開始）→ 解除。
export function abAdvance(cur: AbState): AbStep {
  switch (cur) {
    case "off":
      return { next: "a", action: { type: "ab_loop_set_a" } };
    case "a":
      return { next: "loop", action: { type: "ab_loop_set_b" } };
    case "loop":
      return { next: "off", action: { type: "ab_loop_clear" } };
  }
}

/// 動画 ID に束ねた状態エントリ。
export type AbEntry = { videoId: string; state: AbState };

/// インスタンスの表示状態を解決する。エントリの動画 ID が現在の再生動画と
/// 一致しないとき（連続再生・読み替えで mpv 側のループが消えた分）は
/// off とみなす（FR-37、仕様決定 BM: ループは動画の切替で引き継がない）。
export function abStateFor(
  entry: AbEntry | undefined,
  videoId: string,
): AbState {
  return entry !== undefined && entry.videoId === videoId ? entry.state : "off";
}
