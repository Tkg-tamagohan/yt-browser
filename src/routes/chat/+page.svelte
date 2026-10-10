<script lang="ts">
  // チャットポップアップ窓の本体（FR-27、仕様決定 AT・AU）。
  // chat_popup_open が /chat?v=<videoId>&i=<instanceId> で開く別
  // WebviewWindow にだけ表示される。表示行は投稿時刻と本文のみで、
  // 投稿者名と superchat / membership のバッジは出さない。
  import { page } from "$app/state";
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { t } from "$lib/i18n";
  import ChatList from "$lib/ChatList.svelte";
  import {
    chatPopups,
    initChatEvents,
    openChatPopupView,
  } from "$lib/chat.svelte";
  import {
    asErrorMessage,
    initPlayerEvents,
    playerStates,
  } from "$lib/players.svelte";
  import { chatPopupOntopEnabled } from "$lib/pip";

  const videoId = page.url.searchParams.get("v") ?? "";
  const instanceId = Number(page.url.searchParams.get("i") ?? "0");

  const panel = $derived(chatPopups.list.get(videoId));
  // 「パネルに戻す」は起票元インスタンスが同じ動画を再生中のときだけ有効
  // （終了済み・別動画へ切替済みなら戻ってもチャットは繋がらない）
  const canReturn = $derived(
    playerStates.list.get(instanceId)?.videoId === videoId,
  );

  let ontop = $state(true);
  let error = $state<string | null>(null);

  onMount(async () => {
    openChatPopupView(videoId);
    await initChatEvents();
    await initPlayerEvents();
    try {
      const raw = await invoke<string | null>("settings_get", {
        key: "chat.popup_ontop",
      });
      ontop = chatPopupOntopEnabled(raw);
    } catch {
      // 設定が読めなくても既定（最前面 ON）のまま表示する
    }
  });

  async function toggleOntop(): Promise<void> {
    const next = !ontop;
    try {
      await getCurrentWindow().setAlwaysOnTop(next);
      await invoke("settings_set", {
        key: "chat.popup_ontop",
        value: next ? "on" : "off",
      });
      ontop = next;
      error = null;
    } catch (e) {
      error = asErrorMessage(e);
    }
  }

  async function returnToPanel(): Promise<void> {
    try {
      // 成功時はバックエンド側でパネルの利用者登録・メイン窓への
      // `chat://open_panel` 発行・この窓のクローズまで行う
      await invoke("chat_popup_return", { videoId, instanceId });
    } catch (e) {
      error = asErrorMessage(e);
    }
  }
</script>

<div class="popup">
  <div class="popup-head">
    <h1>{t("chat.title")}</h1>
    <div class="popup-actions">
      <button
        class:active={ontop}
        title={t("chat.pin.hint")}
        onclick={() => void toggleOntop()}
      >
        {t("chat.pin")}
      </button>
      <button disabled={!canReturn} onclick={() => void returnToPanel()}>
        {t("chat.toPanel")}
      </button>
    </div>
  </div>
  {#if panel?.status}
    <p class="chat-status">{panel.status}</p>
  {/if}
  {#if error}
    <p class="chat-status">{error}</p>
  {/if}
  <ChatList items={panel?.items ?? []} showAuthor={false} fillHeight />
</div>

<style>
  .popup {
    display: flex;
    flex-direction: column;
    height: 100vh;
    box-sizing: border-box;
    padding: 8px 12px;
  }

  .popup-head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 8px;
    flex-shrink: 0;
  }

  .popup-head h1 {
    font-size: 1rem;
    margin: 0;
  }

  .popup-actions {
    display: flex;
    gap: 8px;
  }

  .popup-actions button {
    border: 1px solid #3c4043;
    background: none;
    color: #c8c9cc;
    border-radius: 6px;
    padding: 2px 8px;
    font-size: 0.85rem;
    cursor: pointer;
  }

  .popup-actions button.active {
    border-color: #8ab4f8;
    color: #8ab4f8;
  }

  .popup-actions button:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .chat-status {
    color: #f9ab00;
    font-size: 0.85rem;
    margin: 4px 0;
    flex-shrink: 0;
  }
</style>
