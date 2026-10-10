//! 連続再生キュー・ループの純粋ロジック（FR-16、仕様決定 AA）。
//! queue.svelte.ts の状態・invoke から分離した決定部分で、
//! vitest の回帰テスト（LP-NN）から直接検証する。

import type { LoopMode } from "./queue.svelte";

/// バックエンドへ登録する武装キュー（FR-16、仕様決定 AA・AD）。
/// `items` は今後再生する順序列、`loop` は取り出し分を末尾へ戻す巡回。
/// - `one`: 現在項目だけの巡回（同項目の繰り返し）
/// - `all`: 次項目以降 + 先頭〜現在項目の回転順を巡回（末尾→先頭巻き戻り）
/// - `none`: 次項目以降を順に消費（空なら武装解除）
/// `indexIsPlaying` は `items[index]` が再生中かどうか。実行中キューで
/// 再生中項目を削除した直後は index が未再生の次項目を指すため
/// false を渡し、その項目も武装に含める（FR-20、仕様決定 AM）
export function armPlanFor(
  mode: LoopMode,
  items: string[],
  index: number,
  indexIsPlaying = true,
): { items: string[]; loop: boolean } {
  // 武装対象の先頭位置。index が未再生項目を指すときはその項目から含める
  const nextFrom = indexIsPlaying ? index + 1 : index;
  if (mode === "one") {
    const cur = items[index];
    return { items: cur === undefined ? [] : [cur], loop: true };
  }
  if (mode === "all") {
    return {
      items: [...items.slice(nextFrom), ...items.slice(0, nextFrom)],
      loop: true,
    };
  }
  return { items: items.slice(nextFrom), loop: false };
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

/// 項目削除に伴うキュー位置の調整（FR-20、仕様決定 AM）。
/// 再生中より前を消したら 1 つ前へ。再生中項目そのものを消しても
/// index は据え置き（配列上は次項目を指す）。`newLen` は削除後の件数で、
/// 末尾を超えないよう収める
export function indexAfterRemove(
  index: number,
  removed: number,
  newLen: number,
): number {
  const i = removed < index ? index - 1 : index;
  return newLen === 0 ? 0 : Math.min(i, newLen - 1);
}

/// 項目削除後に `items[index]`（調整後の位置）が実際に再生中かどうか
/// （queue.playingCurrent の遷移。index は削除前の位置を渡す）。
/// - 別位置を消した: 変わらない（wasPlaying を維持）
/// - 現位置を消し次項目が残った: index が指すのは未再生の次項目 → false
/// - 末尾の現位置を消した: index は消費済み項目へ戻る → true
///   （未再生の次項目は残っていない）
export function headPlayingAfterRemove(
  wasPlaying: boolean,
  index: number,
  removed: number,
  newLen: number,
): boolean {
  if (removed !== index) return wasPlaying;
  return removed >= newLen;
}

/// 終端イベント（`player://ended` の `continued=false`、インスタンス消失）
/// に対するキューの処置（FR-26、仕様決定 AS）。
/// - 一時キュー: 理由に関わらず消滅（消費完了・消失・全消去で畳む現行のまま）
/// - プレイリストキュー: 末尾項目の `eof` 終了なら消費完了とみなし、
///   位置を先頭に戻して残す（"reset"）。それ以外の終端はインスタンス消失と
///   してキューを消滅させる（"destroy"）
/// 末尾判定は「現在位置が末尾項目」かつ「終了した項目が末尾項目」の双方で
/// 行う。途中項目の `eof` 直後に継続先の読み込みが失敗した場合も同じ
/// `eof`＋非継続で届くため、`reason` だけでは消費完了と消失を区別できない
export function queueEndOutcome(
  playlistId: number | null,
  reason: string,
  items: string[],
  index: number,
  endedVideoId: string,
): "reset" | "destroy" {
  if (playlistId === null) return "destroy";
  const consumed =
    reason === "eof" &&
    index >= items.length - 1 &&
    items[items.length - 1] === endedVideoId;
  return consumed ? "reset" : "destroy";
}

/// 「連続再生を止める」後のキュー位置（FR-26、仕様決定 AS）。
/// 一時キューは項目だけ残して位置を先頭に戻す（現行どおり）。
/// プレイリストキューは位置を保って残し、キューパネルの「再生」で
/// 保存位置から再開する
export function indexAfterStop(
  playlistId: number | null,
  index: number,
): number {
  return playlistId === null ? 0 : index;
}

/// 項目移動（from の項目を to の位置へ挿入）に伴うキュー位置の調整。
/// 再生中項目は配列上の位置ではなく項目そのものを追従させる
export function indexAfterMove(
  index: number,
  from: number,
  to: number,
): number {
  if (from === index) return to;
  if (from < index && to >= index) return index - 1;
  if (from > index && to <= index) return index + 1;
  return index;
}
