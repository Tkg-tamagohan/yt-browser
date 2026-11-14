// NG ユーザー節（FR-70、仕様決定 CU）の行選択ロジック。
// Tauri API へ触れない純粋関数だけを置き、vitest から直接検証できるようにする。

import type { Filter } from "$lib/players.svelte";

// NG 判定（発言の非表示化）に使われるフィルタ種別。
// highlight 種別はマッチした発言を除去しないため NG ユーザーの対象外
// （highlight 種別の chat_author 行は NG フィルタ節側で管理する。
//   暫定解釈。決定記録『Phase 68 で確定した事項』を参照）
const NG_KINDS = new Set(["literal", "regex"]);

/// `chat_author` フィルタのうち NG 系種別（部分一致・正規表現）の行を返す。
/// 並びは `filter_list` の返却順（id 昇順）を維持する
export function ngAuthorRows(filters: Filter[]): Filter[] {
  return filters.filter(
    (f) => f.target === "chat_author" && NG_KINDS.has(f.kind),
  );
}
