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
import { mergeChatItems, type ChatItem } from "$lib/chat-logic";

export type { ChatItem };

/// チャットパネルの開閉と表示済みメッセージ（インスタンス ID ごと）
export const chatPanels = $state<{
  list: Map<number, { open: boolean; items: ChatItem[]; status: string | null }>;
}>({ list: new Map() });

/// チャットポップアップ窓の表示済みメッセージ（動画 ID ごと、FR-27）。
/// 窓ごとに JS コンテキストが分かれるため、ポップアップ窓側でだけ
/// エントリを持ち、メイン窓では常に空のままになる
export const chatPopups = $state<{
  list: Map<string, { items: ChatItem[]; status: string | null }>;
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
export async function toggleChat(id: number, videoId: string): Promise<void> {
  const cur = chatPanels.list.get(id);
  const next = new Map(chatPanels.list);
  if (cur?.open) {
    next.set(id, { ...cur, open: false, status: null });
    chatPanels.list = next;
    releaseChatPanel(videoId, id);
    return;
  }
  next.set(id, { open: true, items: cur?.items ?? [], status: null });
  chatPanels.list = next;
  enqueueChatOp(videoId, async () => {
    try {
      // リプレイの同期先をこのインスタンスに固定するため起票を渡す
      // （同一動画の複数窓がある場合でもずれない。FR-24）
      await invoke("chat_start", { videoId, instanceId: id });
    } catch (e) {
      notify(t("player.error", { message: asErrorMessage(e) }));
    }
  });
}

/// 埋め込みパネルからチャットポップアップ窓へ切り替える（FR-27、
/// 仕様決定 AT）。バックエンドがポップアップの利用者を先に登録するため、
/// その後にパネル利用者を解除しても共有ポーラーは止まらない
export async function openChatPopup(id: number, videoId: string): Promise<void> {
  try {
    await invoke("chat_popup_open", { videoId, instanceId: id });
  } catch (e) {
    notify(t("player.error", { message: asErrorMessage(e) }));
    return;
  }
  const cur = chatPanels.list.get(id);
  if (cur?.open) {
    const next = new Map(chatPanels.list);
    next.set(id, { ...cur, open: false, status: null });
    chatPanels.list = next;
  }
  releaseChatPanel(videoId, id);
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
  // ポップアップ窓側の表示（別 JS コンテキスト）は動画 ID で直接振り分ける
  // （FR-27。メイン窓では chatPopups が空のため実質 no-op）
  const nextPopups = new Map(chatPopups.list);
  let popupChanged = false;
  for (const [vid, evs] of byVideo) {
    for (const inst of vidToInstances.get(vid) ?? []) {
      const cur = next.get(inst);
      if (!cur) continue;
      next.set(inst, { ...cur, items: mergeChatItems(cur.items, evs) });
      changed = true;
    }
    const popup = nextPopups.get(vid);
    if (popup) {
      nextPopups.set(vid, { ...popup, items: mergeChatItems(popup.items, evs) });
      popupChanged = true;
    }
  }
  if (changed) {
    chatPanels.list = next;
  }
  if (popupChanged) {
    chatPopups.list = nextPopups;
  }
}

/// パネルの利用者解除をバックエンドへ送る。同じ動画の利用者
/// （他パネル・ポップアップ）が残っていれば共有ポーラーは維持される
/// （FR-27、仕様決定 AT）
function releaseChatPanel(videoId: string, instanceId: number): void {
  enqueueChatOp(videoId, async () => {
    await invoke("chat_stop", { videoId, instanceId }).catch(() => {});
  });
}

/// インスタンスのチャットパネルを閉じ、最後の利用者なら取得も止める
export function cleanupChatPanel(instanceId: number, videoId: string): void {
  if (chatPanels.list.delete(instanceId)) {
    chatPanels.list = new Map(chatPanels.list);
  }
  releaseChatPanel(videoId, instanceId);
}

/// `chat://reset` の受信処理（FR-24）。リプレイの後方シーク再アンカー時に
/// 届き、対象動画を再生中の全パネルの既表示行を消す。
/// この直後に再送分が届くため、シーク先より未来の発言が残らない。
/// 閉じたパネルも受信分を保持しているため、開き直しで古い発言が
/// 出ないよう open に関わらず消す
function onChatReset(p: { videoId: string }): void {
  const next = new Map(chatPanels.list);
  let changed = false;
  for (const [inst, cp] of next) {
    if (playerStates.list.get(inst)?.videoId === p.videoId) {
      next.set(inst, { ...cp, items: [] });
      changed = true;
    }
  }
  if (changed) chatPanels.list = next;
  const popup = chatPopups.list.get(p.videoId);
  if (popup) {
    const nextPopups = new Map(chatPopups.list);
    nextPopups.set(p.videoId, { ...popup, items: [] });
    chatPopups.list = nextPopups;
  }
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
    const popup = chatPopups.list.get(s.videoId);
    if (popup) {
      const nextPopups = new Map(chatPopups.list);
      nextPopups.set(s.videoId, { ...popup, status: s.message });
      chatPopups.list = nextPopups;
    }
  }
  if (s.level !== "info") {
    notify(t("chat.status", { message: s.message }));
  }
}

/// ポップアップ窓側の表示エントリを用意する（FR-27）。ポップアップ窓の
/// コンテキストでのみ呼ばれ、以後届く `chat://` イベントがこのエントリへ
/// 蓄積される（窓を開く前に流れた分は受信できない。埋め込みパネルとは
/// 違い、過去分の引き継ぎはない）
export function openChatPopupView(videoId: string): void {
  if (!chatPopups.list.has(videoId)) {
    const next = new Map(chatPopups.list);
    next.set(videoId, { items: [], status: null });
    chatPopups.list = next;
  }
}

let initPromise: Promise<void> | null = null;

/// `chat://message` / `chat://status` を購読する。
/// PlayerCards はレイアウトで常時マウントのため、最初の onMount で一度だけ登録する
/// （players.svelte.ts の initPlayerEvents と同じ型）。
/// ポップアップ窓では /chat ページが同じく一度だけ呼ぶ。
export function initChatEvents(): Promise<void> {
  if (!initPromise) {
    initPromise = (async () => {
      await listen<ChatEvent[]>("chat://message", (ev) =>
        onChatMessages(ev.payload),
      );
      await listen<ChatStatus>("chat://status", (ev) =>
        onChatStatus(ev.payload),
      );
      await listen<{ videoId: string }>("chat://reset", (ev) =>
        onChatReset(ev.payload),
      );
      // ポップアップ側の「パネルに戻す」でバックエンドが発行する
      // パネル復帰要求。対象インスタンスが同じ動画を再生中のときだけ
      // 埋め込みパネルを開き直す（FR-27、仕様決定 AT）
      await listen<{ instanceId: number; videoId: string }>(
        "chat://open-panel",
        (ev) => {
          const { instanceId, videoId } = ev.payload;
          if (playerStates.list.get(instanceId)?.videoId !== videoId) return;
          const cur = chatPanels.list.get(instanceId);
          const next = new Map(chatPanels.list);
          next.set(instanceId, {
            open: true,
            items: cur?.items ?? [],
            status: null,
          });
          chatPanels.list = next;
        },
      );
    })();
  }
  return initPromise;
}
