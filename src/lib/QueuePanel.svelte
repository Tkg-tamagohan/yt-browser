<script lang="ts">
  // キュースタックのパネル（FR-20、仕様決定 AM）。
  // 一覧・並べ替え（上下ボタンと DnD）・個別削除・全消去・再生開始を提供する。
  // ナビ入口とキュー実行中のプレイヤーカードの双方から同じパネルを開く。
  // 項目メタ（タイトル・チャンネル）はセッション内メモリのみ
  import { goto } from "$app/navigation";
  import { t } from "$lib/i18n";
  import { notify } from "$lib/notices.svelte";
  import { asErrorMessage } from "$lib/players.svelte";
  import {
    queue,
    queueMetaOf,
    queuePanel,
    queueClear,
    queueMove,
    queuePlayStart,
    queueRemoveAt,
    stopQueue,
  } from "$lib/queue.svelte";

  // 上下ボタンと共通の移動。実行中は queueMove 側で武装を張り替える
  let dragFrom = $state<number | null>(null);
  let dragOver = $state<number | null>(null);

  function drop(to: number): void {
    if (dragFrom !== null && dragFrom !== to) queueMove(dragFrom, to);
    dragFrom = null;
    dragOver = null;
  }

  async function start(): Promise<void> {
    try {
      await queuePlayStart();
      goto("/");
    } catch (e) {
      notify(
        t("queue.startFailed", { message: asErrorMessage(e) }),
      );
    }
  }
</script>

<aside class="queue-panel" aria-label={t("queue.panel.title")}>
  <div class="qp-head">
    <h3>
      {t("queue.panel.title")}
      <span class="qp-count">{queue.items.length}</span>
    </h3>
    {#if queue.instanceId === null}
      <button
        class="link"
        disabled={queue.items.length === 0}
        onclick={() => void start()}
      >
        {t("queue.play")}
      </button>
    {:else}
      <button class="link" onclick={() => stopQueue()}>
        {t("library.playlist.queueStop")}
      </button>
    {/if}
    <button
      class="link danger"
      disabled={queue.items.length === 0}
      onclick={() => queueClear()}
    >
      {t("queue.clear")}
    </button>
    <button class="link" onclick={() => (queuePanel.open = false)}>×</button>
  </div>

  {#if queue.items.length === 0}
    <p class="subtle">{t("queue.empty")}</p>
  {:else}
    <ul class="qp-list">
      {#each queue.items as vid, i (i)}
        {@const meta = queueMetaOf(vid)}
        <li
          class:playing={queue.instanceId !== null && i === queue.index}
          class:drop-target={dragOver === i && dragFrom !== i}
          draggable="true"
          ondragstart={(e) => {
            dragFrom = i;
            e.dataTransfer?.setData("text/plain", vid);
            if (e.dataTransfer) e.dataTransfer.effectAllowed = "move";
          }}
          ondragover={(e) => {
            if (dragFrom === null) return;
            e.preventDefault();
            dragOver = i;
          }}
          ondragleave={() => {
            if (dragOver === i) dragOver = null;
          }}
          ondrop={(e) => {
            e.preventDefault();
            drop(i);
          }}
          ondragend={() => {
            dragFrom = null;
            dragOver = null;
          }}
        >
          <span class="pos">{i + 1}</span>
          <span class="qp-text">
            <span class="qp-title" title={meta.title}>{meta.title}</span>
            {#if meta.channelTitle}
              <span class="qp-ch">{meta.channelTitle}</span>
            {/if}
          </span>
          <button
            class="link"
            title={t("library.playlist.moveUp")}
            disabled={i === 0}
            onclick={() => queueMove(i, i - 1)}>↑</button
          >
          <button
            class="link"
            title={t("library.playlist.moveDown")}
            disabled={i === queue.items.length - 1}
            onclick={() => queueMove(i, i + 1)}>↓</button
          >
          <button
            class="link danger"
            title={t("library.remove")}
            onclick={() => queueRemoveAt(i)}>×</button
          >
        </li>
      {/each}
    </ul>
  {/if}
</aside>

<style>
  .queue-panel {
    position: fixed;
    top: 48px;
    right: 16px;
    z-index: 40;
    width: 380px;
    max-height: 70vh;
    display: flex;
    flex-direction: column;
    padding: 12px;
    border: 1px solid #3c4043;
    border-radius: 12px;
    background: #202124;
    box-shadow: 0 8px 24px rgb(0 0 0 / 50%);
  }

  .qp-head {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .qp-head h3 {
    margin: 0;
    font-size: 0.95rem;
    flex: 1;
  }

  .qp-count {
    color: #9aa0a6;
    font-weight: 400;
    font-size: 0.85rem;
    margin-left: 6px;
  }

  .qp-list {
    list-style: none;
    margin: 8px 0 0;
    padding: 0;
    overflow-y: auto;
  }

  .qp-list li {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 4px 0;
    border-top: 1px solid #2d2f33;
    font-size: 0.85rem;
  }

  .qp-list li.playing .pos,
  .qp-list li.playing .qp-title {
    color: #8ab4f8;
  }

  .qp-list li.drop-target {
    border-top-color: #8ab4f8;
  }

  .pos {
    color: #9aa0a6;
    min-width: 1.6em;
    text-align: right;
  }

  .qp-text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }

  .qp-title {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .qp-ch {
    color: #9aa0a6;
    font-size: 0.8rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .subtle {
    color: #9aa0a6;
  }
</style>
