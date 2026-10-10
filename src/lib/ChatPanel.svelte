<script lang="ts">
  // チャットパネルの描画。開いている間だけマウントされる。
  // 表示済みメッセージは chat.svelte.ts の共有ストアにあり、
  // 閉じている間も受信は蓄積され続ける。
  import { t } from "$lib/i18n";
  import { chatPanels, openChatPopup } from "$lib/chat.svelte";
  import ChatList from "$lib/ChatList.svelte";

  let { instanceId, videoId }: { instanceId: number; videoId: string } =
    $props();

  const panel = $derived(chatPanels.list.get(instanceId));
</script>

<div class="chat-panel sub-panel">
  <div class="chat-head">
    <h3>{t("chat.title")}</h3>
    <!-- ポップアップ窓へ切り替える（FR-27）。開くとこのパネルは閉じる -->
    <button
      class="link"
      title={t("chat.popup.hint")}
      onclick={() => void openChatPopup(instanceId, videoId)}
    >
      {t("chat.popup")}
    </button>
  </div>
  {#if panel?.status}
    <p class="chat-status">{panel?.status}</p>
  {/if}
  <ChatList items={panel?.items ?? []} />
</div>

<style>
  .chat-head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    margin-bottom: 8px;
  }

  .chat-panel h3 {
    font-size: 1rem;
    margin: 0;
  }

  .chat-head .link {
    border: none;
    background: none;
    color: #8ab4f8;
    font-size: 0.85rem;
    cursor: pointer;
    padding: 0;
  }

  .chat-status {
    color: #f9ab00;
    font-size: 0.85rem;
    margin: 4px 0;
  }
</style>
