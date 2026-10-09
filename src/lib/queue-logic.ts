//! 連続再生キュー・ループの純粋ロジック（FR-16、仕様決定 AA）。
//! queue.svelte.ts の状態・invoke から分離した決定部分で、
//! vitest の回帰テスト（LP-NN）から直接検証する。

import type { LoopMode } from "./queue.svelte";

/// 次に武装すべき項目。1 項目ループは現在項目、全体ループは末尾到達で
/// 先頭、なしは次項目（末尾なら null で武装解除）。
export function armTargetFor(
  mode: LoopMode,
  items: string[],
  index: number,
): string | null {
  if (mode === "one") return items[index] ?? null;
  if (mode === "all") return items[index + 1] ?? items[0] ?? null;
  return items[index + 1] ?? null;
}

/// `player://ended` の `continuedVideoId`（実際に読み込みが始まった項目）から
/// キュー位置を照合する。モード変更と遷移の競合でイベント到着時のモードは
/// 直前の遷移を表さないため、位置はロード済み項目で決める（続きの武装の
/// 対象選定はその時点のモードで行う）。
/// - 現在項目と同一: 繰り返し（位置据え置き）
/// - 次項目と同一: 1 つ進める
/// - 先頭項目と同一: 全体ループの巻き戻りで 0 へ
/// - いずれでもない: queue drift として位置を変えない
export function reconcileIndex(
  items: string[],
  index: number,
  continuedVideoId: string | null,
): number {
  if (continuedVideoId === null) return index;
  if (items[index] === continuedVideoId) return index;
  if (items[index + 1] === continuedVideoId) return index + 1;
  if (items[0] === continuedVideoId) return 0;
  return index;
}
