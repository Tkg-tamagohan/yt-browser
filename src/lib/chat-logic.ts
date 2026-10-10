// チャット表示行の併合ロジック（設計書 §6.2〜6.3）。
// Tauri API へ触れない純粋関数だけを置き、vitest から直接検証できるようにする。
// 呼び出し側の共有状態は chat.svelte.ts が持つ。

import type { ChatEvent } from "$lib/players.svelte";

/// 表示済みメッセージの 1 行。deleted フラグは削除アクションが届いた
/// 表示行を消さず打消し表示にするためのもの
export type ChatItem = ChatEvent & { deleted?: boolean };

/// 既表示行へ新着イベントを併合する。削除イベントは既表示行の打消しに
/// 使い、ng / other は表示しない。表示件数は直近 500 件に絞る
/// （決定記録『Phase 6 で確定した事項』の表示上限）
const CHAT_CAP = 500;
export function mergeChatItems(
  items: ChatItem[],
  evs: ChatEvent[],
): ChatItem[] {
  const out = [...items];
  const known = new Set(out.map((i) => i.itemId));
  for (const e of evs) {
    if (e.kind === "deleted") {
      const idx = out.findIndex((i) => i.itemId === e.message);
      if (idx >= 0) out[idx] = { ...out[idx], deleted: true };
      continue;
    }
    if (e.ng || e.kind === "other") continue;
    // 保存失敗後の再送などで同一 item_id が二度届きうるため表示側でも dedup
    if (e.itemId && known.has(e.itemId)) continue;
    if (e.itemId) known.add(e.itemId);
    out.push(e);
  }
  return out.slice(-CHAT_CAP);
}
