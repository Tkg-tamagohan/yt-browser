// チャットパネルの共有状態と `chat://` イベント購読（設計書 §6.2 のポーラー連携）。
// パネルは開閉でアンマウントされるため、閉じている間も受信を蓄積できるよう、
// players.svelte.ts と同じくコンポーネント外のストアに状態を置く。

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { t } from "$lib/i18n";
import { notify } from "$lib/notices.svelte";
import {
  asErrorMessage,
  playerStates,
  type ChatEvent,
  type ChatStatus,
} from "$lib/players.svelte";

/// 表示済みメッセージの 1 行。deleted フラグは削除アクションが届いた
/// 表示行を消さず打消し表示にするためのもの
export type ChatItem = ChatEvent & { deleted?: boolean };

/// チャットパネルの開閉と表示済みメッセージ（インスタンス ID ごと）
export const chatPanels = $state<{
  list: Map<number, { open: boolean; items: ChatItem[]; status: string | null }>;
}>({ list: new Map() });

/// 同一動画の chat_start/chat_stop を直列化するキュー。
/// invoke は到着順を保証しないため、「閉じる→すぐ開く」で
/// stop が start の後に処理されてポーラーが死ぬ競合を防ぐ。
const chatOpQueues = new Map<string, Promise<void>>();
function enqueueChatOp(videoId: string, op: () => Promise<void>): void {
  const prev = chatOpQueues.get(videoId) ?? Promise.resolve();
  const next = prev.then(() => op().catch(() => {}));
  chatOpQueues.set(videoId, next);
  void next.finally(() => {
    if (chatOpQueues.get(videoId) === next) chatOpQueues.delete(videoId);
  });
}

/// チャットパネルの開閉。開くと chat_start、閉じると chat_stop を呼ぶ。
/// メッセージの表示件数は直近 500 件に絞る（決定記録『Phase 6 で確定した事項』の表示上限）。
const CHAT_CAP = 500;
export async function toggleChat(id: number, videoId: string): Promise<void> {
  const cur = chatPanels.list.get(id);
  const next = new Map(chatPanels.list);
  if (cur?.open) {
    next.set(id, { ...cur, open: false, status: null });
    chatPanels.list = next;
    // 同じ動画を見ている他パネルがあれば共有ポーラーは維持する
    maybeStopChat(videoId);
    return;
  }
  next.set(id, { open: true, items: cur?.items ?? [], status: null });
  chatPanels.list = next;
  enqueueChatOp(videoId, async () => {
    try {
      await invoke("chat_start", { videoId });
    } catch (e) {
      notify(t("player.error", { message: asErrorMessage(e) }));
    }
  });
}

/// `chat://message` の受信処理。videoId で対応プレイヤーへ振り分け、
/// 削除イベントは既表示行の打消しに使う。ng / other は表示しない。
function onChatMessages(events: ChatEvent[]): void {
  const byVideo = new Map<string, ChatEvent[]>();
  for (const e of events) {
    const arr = byVideo.get(e.videoId) ?? [];
    arr.push(e);
    byVideo.set(e.videoId, arr);
  }
  // 同一動画を複数ウィンドウで再生している場合、開いている全パネルへ配送する
  // （ポーラーは動画 ID ごとに 1 本。設計書 §6.2）
  const vidToInstances = new Map<string, number[]>();
  for (const p of playerStates.list.values()) {
    const arr = vidToInstances.get(p.videoId) ?? [];
    arr.push(p.instanceId);
    vidToInstances.set(p.videoId, arr);
  }
  const next = new Map(chatPanels.list);
  let changed = false;
  for (const [vid, evs] of byVideo) {
    for (const inst of vidToInstances.get(vid) ?? []) {
      const cur = next.get(inst);
      if (!cur) continue;
      const items = [...cur.items];
      const known = new Set(items.map((i) => i.itemId));
      for (const e of evs) {
        if (e.kind === "deleted") {
          const idx = items.findIndex((i) => i.itemId === e.message);
          if (idx >= 0) items[idx] = { ...items[idx], deleted: true };
          continue;
        }
        if (e.ng || e.kind === "other") continue;
        // 保存失敗後の再送などで同一 item_id が二度届きうるため表示側でも dedup
        if (e.itemId && known.has(e.itemId)) continue;
        if (e.itemId) known.add(e.itemId);
        items.push(e);
      }
      next.set(inst, { ...cur, items: items.slice(-CHAT_CAP) });
      changed = true;
    }
  }
  if (changed) {
    chatPanels.list = next;
  }
}

/// 同じ動画の開いたパネルが残っていなければ共有ポーラーを止める。
/// パネルの状態変更（閉じる・削除）を反映した後に呼ぶこと。
function maybeStopChat(videoId: string): void {
  const stillOpen = [...chatPanels.list.entries()].some(
    ([inst, cp]) =>
      cp.open && playerStates.list.get(inst)?.videoId === videoId,
  );
  if (!stillOpen) {
    enqueueChatOp(videoId, async () => {
      await invoke("chat_stop", { videoId }).catch(() => {});
    });
  }
}

/// インスタンスのチャットパネルを閉じ、最後の利用者なら取得も止める
export function cleanupChatPanel(instanceId: number, videoId: string): void {
  if (chatPanels.list.delete(instanceId)) {
    chatPanels.list = new Map(chatPanels.list);
  }
  maybeStopChat(videoId);
}

/// `chat://status` の受信処理。対象動画を開いている全パネルに状態行を出し、
/// warn 以上は通知にも出す。
function onChatStatus(s: ChatStatus): void {
  if (s.videoId) {
    const next = new Map(chatPanels.list);
    let changed = false;
    for (const [inst, cp] of next) {
      if (cp.open && playerStates.list.get(inst)?.videoId === s.videoId) {
        next.set(inst, { ...cp, status: s.message });
        changed = true;
      }
    }
    if (changed) chatPanels.list = next;
  }
  if (s.level !== "info") {
    notify(t("chat.status", { message: s.message }));
  }
}

let initPromise: Promise<void> | null = null;

/// `chat://message` / `chat://status` を購読する。
/// PlayerCards はレイアウトで常時マウントのため、最初の onMount で一度だけ登録する
/// （players.svelte.ts の initPlayerEvents と同じ型）。
export function initChatEvents(): Promise<void> {
  if (!initPromise) {
    initPromise = (async () => {
      await listen<ChatEvent[]>("chat://message", (ev) =>
        onChatMessages(ev.payload),
      );
      await listen<ChatStatus>("chat://status", (ev) =>
        onChatStatus(ev.payload),
      );
    })();
  }
  return initPromise;
}
