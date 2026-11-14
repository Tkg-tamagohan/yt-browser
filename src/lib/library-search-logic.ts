// ライブラリ横断検索タブ（FR-60、仕様決定 CK）の純粋ロジック。
// バックエンドの library_search は既に正準順の sources と件数上限を返すが、
// 表示側では防御的に正準順へ並べ替えて描画する。

import type { MessageKey } from "./i18n";
import type { LibrarySource } from "./players.svelte";

/// 区分バッジの正準順（ライブラリのタブ順）。`library_search` の
/// sources もこの順で返る仕様で、ここは表示側の防御的並べ替えに使う
export const LIBRARY_SOURCE_ORDER: readonly LibrarySource[] = [
  "history",
  "favorites",
  "playlists",
  "watchLater",
];

/// 区分バッジの i18n キー（動的連結ではなく写像で型を確定させる）
const SOURCE_LABEL_KEYS: Record<LibrarySource, MessageKey> = {
  history: "library.search.source.history",
  favorites: "library.search.source.favorites",
  playlists: "library.search.source.playlists",
  watchLater: "library.search.source.watchLater",
};

/// ヒット区分の表示ラベル用メッセージキー
export function sourceLabelKey(src: LibrarySource): MessageKey {
  return SOURCE_LABEL_KEYS[src];
}

/// 検索語の正規化。前後の空白を落とす（空・空白のみは検索を
/// 送らず 0 件として扱う暫定解釈の判定に使う）
export function normalizeSearchQuery(raw: string): string {
  return raw.trim();
}

/// ヒット区分を正準順に並べ替える。既知外の値は末尾へ寄せる
/// （バックエンドが将来区分を増やしても既知区分の順は保たれる）
export function sortSources(sources: LibrarySource[]): LibrarySource[] {
  const order = (s: LibrarySource): number => {
    const i = LIBRARY_SOURCE_ORDER.indexOf(s);
    return i === -1 ? LIBRARY_SOURCE_ORDER.length : i;
  };
  return [...sources].sort((a, b) => order(a) - order(b));
}
