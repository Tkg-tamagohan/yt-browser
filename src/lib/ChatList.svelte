<script lang="ts">
  // チャットメッセージ一覧の共有描画。埋め込みパネル（ChatPanel）と
  // ポップアップ窓（/chat）で同じ行構成を使う（FR-27、仕様決定 AT・AU）。
  // 新着が届くたび末尾へ追従するスクロールもここで持つ
  import { t } from "$lib/i18n";
  import { fmtChatTime } from "$lib/format";
  import type { ChatItem } from "$lib/chat.svelte";

  let {
    items,
    showAuthor = true,
    fillHeight = false,
  }: {
    items: ChatItem[];
    // 埋め込みパネルは投稿者名と superchat / membership のバッジを出す。
    // ポップアップ窓は投稿時刻と本文だけを出す（仕様決定 AU）
    showAuthor?: boolean;
    // true なら flex コンテナの残り高いっぱいに伸びる（ポップアップ窓）。
    // false なら高さは最大 320px まで（埋め込みパネル）
    fillHeight?: boolean;
  } = $props();

  let listEl = $state<HTMLDivElement>();

  // 受信で items が変わるたびに末尾へ追従する。マウント直後の
  // 初回実行でも最新行が見えるよう末尾へスクロールする
  $effect(() => {
    if (!items || !listEl) return;
    listEl.scrollTop = listEl.scrollHeight;
  });
</script>

<div class="chat-list" class:fill={fillHeight} bind:this={listEl}>
  {#each items as item (item.itemId || item)}
    {#if item.deleted}
      <p class="chat-item deleted">
        <span class="subtle">{t("chat.deleted")}</span>
      </p>
    {:else}
      <p class="chat-item" class:superchat={item.kind === "superchat"}>
        <span class="chat-time">{fmtChatTime(item.postedAtUsec)}</span>
        {#if showAuthor}
          <span class="chat-author">
            {item.authorName ?? t("chat.anonymous")}
          </span>
          {#if item.kind === "superchat" && item.amountDisplay}
            <span class="chat-badge superchat-badge">{item.amountDisplay}</span>
          {:else if item.kind === "membership"}
            <span class="chat-badge">{t("chat.membership")}</span>
          {/if}
        {/if}
        <span class="chat-msg">{item.message}</span>
      </p>
    {/if}
  {/each}
  {#if items.length === 0}
    <p class="subtle">{t("chat.empty")}</p>
  {/if}
</div>

<style>
  .chat-list {
    max-height: 320px;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  /* ポップアップ窓では高さ上限ではなく flex コンテナの残りを使う */
  .chat-list.fill {
    max-height: none;
    flex: 1;
    min-height: 0;
    margin-top: 8px;
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
