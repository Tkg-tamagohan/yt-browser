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
    showBadges = true,
    fillHeight = false,
  }: {
    items: ChatItem[];
    // 埋め込みパネルは投稿者名と superchat / membership のバッジを出す。
    // ポップアップ窓は投稿時刻と本文だけを出す（仕様決定 AU。
    // chat.popup_show_author が on なら投稿者名だけ出す。FR-68・CS。
    // バッジは設定化の対象外のためポップアップでは出さない）
    showAuthor?: boolean;
    // superchat / membership のバッジを出すか。showAuthor が true の
    // ときだけ効く（埋め込みパネルは true、ポップアップは false）
    showBadges?: boolean;
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
      <p
        class="chat-item"
        class:superchat={item.kind === "superchat"}
        class:highlight={item.hl}
      >
        <span class="chat-time">{fmtChatTime(item.postedAtUsec)}</span>
        {#if showAuthor}
          <span class="chat-author">
            {item.authorName ?? t("chat.anonymous")}
          </span>
          {#if showBadges}
            {#if item.kind === "superchat" && item.amountDisplay}
              <span class="chat-badge superchat-badge"
                >{item.amountDisplay}</span
              >
            {:else if item.kind === "membership"}
              <span class="chat-badge">{t("chat.membership")}</span>
            {/if}
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
    /* chat.popup_font_size（FR-68、仕様決定 CS）がポップアップ窓の
       祖先要素へ --chat-font-size として乗る。未設定（埋め込み
       パネル）は従来の 0.88rem。行内の要素は em 指定で追従する */
    font-size: var(--chat-font-size, 0.88rem);
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
    display: flex;
    gap: 8px;
    align-items: baseline;
    flex-wrap: wrap;
  }

  .chat-item.deleted {
    opacity: 0.6;
  }

  .chat-item.superchat {
    background: var(--bg-superchat);
    border-radius: 6px;
    padding: 2px 8px;
  }

  /* FR-69（仕様決定 CT）: highlight 種別フィルタにマッチした発言の強調色。
     スパチャの茶系と区別しやすい黄土色系を暫定で使う。埋め込み・
     ポップアップとも同じ行構成のため自動的に双方へ効く */
  .chat-item.highlight {
    background: var(--bg-highlight);
    border-radius: 6px;
    padding: 2px 8px;
  }

  .chat-time {
    color: var(--fg-muted);
    font-family: monospace;
    /* --chat-font-size の拡大縮小に追従させるため em 相対
       （0.88rem 基準で 0.78rem と同じ比率） */
    font-size: 0.89em;
    flex-shrink: 0;
  }

  .chat-author {
    color: var(--accent);
    /* 0.88rem 基準で 0.8rem と同じ比率 */
    font-size: 0.91em;
    flex-shrink: 0;
    max-width: 14em;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .chat-badge {
    background: var(--bg-subtle);
    border-radius: 4px;
    padding: 0 6px;
    /* 0.88rem 基準で 0.75rem と同じ比率 */
    font-size: 0.85em;
    flex-shrink: 0;
  }

  .chat-badge.superchat-badge {
    background: var(--warn);
    color: var(--on-warn);
    font-weight: 600;
  }

  .chat-msg {
    overflow-wrap: anywhere;
    min-width: 0;
  }

  .subtle {
    /* 0.88rem 基準で 0.85rem と同じ比率 */
    font-size: 0.97em;
  }
</style>
