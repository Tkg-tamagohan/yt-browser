// 字幕トラック選択の表示ロジック（FR-40、仕様決定 BP）。
// mpv `track-list` の `type=sub` エントリからドロップダウンの表示名を
// 組み立てる純粋関数をまとめる（Vitest 対象）。

import type { SubTrack } from "./players.svelte";

/// 字幕トラックの表示名。title があれば優先し、lang もあれば括弧付きで
/// 併記する（YouTube の自動字幕は lang と title の両方を持つことがある）。
/// どちらも無ければトラック ID のみを出す。空文字フィールドは
/// 無いものとして扱う
export function subTrackLabel(t: SubTrack): string {
  const title = t.title ?? "";
  const lang = t.lang ?? "";
  if (title !== "" && lang !== "" && title !== lang) {
    return `${title} (${lang})`;
  }
  if (title !== "") return title;
  if (lang !== "") return lang;
  return `#${t.id}`;
}
