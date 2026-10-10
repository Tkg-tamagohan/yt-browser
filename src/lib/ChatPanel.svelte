<script lang="ts">
  // チャットパネルの描画。開いている間だけマウントされる。
  // 表示済みメッセージは chat.svelte.ts の共有ストアにあり、
  // 閉じている間も受信は蓄積され続ける。
  import { t } from "$lib/i18n";
  import { fmtChatTime } from "$lib/format";
  import { chatPanels, openChatPopup } from "$lib/chat.svelte";

  let { instanceId, videoId }: { instanceId: number; videoId: string } =
    $props();

  const panel = $derived(chatPanels.list.get(instanceId));

  let listEl = $state<HTMLDivElement>();

  // 受信で items が変わるたびに末尾へ追従する。開いた時点でも
  // 最新行が見えるよう、マウント直後の初回実行でも末尾へスクロールする
  $effect(() => {
    const items = panel?.items;
    if (!items || !listEl) return;
    listEl.scrollTop = listEl.scrollHeight;
  });

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
  <div class="chat-list" bind:this={listEl}>
    {#each panel?.items ?? [] as item (item.itemId || item)}
      {#if item.deleted}
        <p class="chat-item deleted">
          <span class="subtle">{t("chat.deleted")}</span>
        </p>
      {:else}
        <p class="chat-item" class:superchat={item.kind === "superchat"}>
          <span class="chat-time">{fmtChatTime(item.postedAtUsec)}</span>
          <span class="chat-author">
            {item.authorName ?? t("chat.anonymous")}
          </span>
          {#if item.kind === "superchat" && item.amountDisplay}
            <span class="chat-badge superchat-badge">{item.amountDisplay}</span>
          {:else if item.kind === "membership"}
            <span class="chat-badge">{t("chat.membership")}</span>
          {/if}
          <span class="chat-msg">{item.message}</span>
        </p>
      {/if}
    {/each}
    {#if (panel?.items.length ?? 0) === 0}
      <p class="subtle">{t("chat.empty")}</p>
    {/if}
  </div>
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

  .chat-list {
    max-height: 320px;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .chat-item {
    margin: 0;
    padding: 2px 0;
    font-size: 0.88rem;
    display: flex;
    gap: 8px;
    align-items: baseline;
    flex-wrap: wrap;
  }

  .chat-item.deleted {
    opacity: 0.6;
  }

  .chat-item.superchat {
    background: #3d2b1f;
    border-radius: 6px;
    padding: 2px 8px;
  }

  .chat-time {
    color: #9aa0a6;
    font-family: monospace;
    font-size: 0.78rem;
    flex-shrink: 0;
  }

  .chat-author {
    color: #8ab4f8;
    font-size: 0.8rem;
    flex-shrink: 0;
    max-width: 14em;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .chat-badge {
    background: #3c4043;
    border-radius: 4px;
    padding: 0 6px;
    font-size: 0.75rem;
    flex-shrink: 0;
  }

  .chat-badge.superchat-badge {
    background: #f9ab00;
    color: #202124;
    font-weight: 600;
  }

  .chat-msg {
    overflow-wrap: anywhere;
    min-width: 0;
  }

  .subtle {
    font-size: 0.85rem;
  }
</style>
