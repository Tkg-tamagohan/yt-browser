//! 連続再生キュー・ループの純粋ロジック（FR-16、仕様決定 AA）。
//! queue.svelte.ts の状態・invoke から分離した決定部分で、
//! vitest の回帰テスト（LP-NN）から直接検証する。

import type { LoopMode } from "./queue.svelte";

/// バックエンドへ登録する武装キュー（FR-16、仕様決定 AA・AD）。
/// `items` は今後再生する順序列、`loop` は取り出し分を末尾へ戻す巡回。
/// - `one`: 現在項目だけの巡回（同項目の繰り返し）
/// - `all`: 次項目以降 + 先頭〜現在項目の回転順を巡回（末尾→先頭巻き戻り）
/// - `none`: 次項目以降を順に消費（空なら武装解除）
export function armPlanFor(
  mode: LoopMode,
  items: string[],
  index: number,
): { items: string[]; loop: boolean } {
  if (mode === "one") {
    const cur = items[index];
    return { items: cur === undefined ? [] : [cur], loop: true };
  }
  if (mode === "all") {
    return { items: [...items.slice(index + 1), ...items.slice(0, index + 1)], loop: true };
  }
  return { items: items.slice(index + 1), loop: false };
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
