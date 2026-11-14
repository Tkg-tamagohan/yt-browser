<script lang="ts">
  // キューパネル（FR-20・FR-26・FR-46、仕様決定 AM・AS・BW）。
  // 上部のタブで表示対象キュー（一時キュー＋存在するプレイリストキュー）を
  // 切り替え、選択中キューの一覧・並べ替え（上下ボタンと DnD）・個別削除・
  // 残りシャッフル・全消去・再生開始を提供する。停止中のプレイリストキューは
  // 「再生」で保存位置から再開する。ナビ入口とキューを背負うカードから
  // 同じパネルを開く。タブ選択は「キューに追加」の追加先指定でもある
  // （FR-45、仕様決定 BV）
  // 項目メタ（タイトル・チャンネル）はセッション内メモリのみ
  import { goto } from "$app/navigation";
  import { t } from "$lib/i18n";
  import { notify } from "$lib/notices.svelte";
  import { asErrorMessage } from "$lib/players.svelte";
  import {
    liveWaits,
    liveWaitRemove,
  } from "$lib/livewait.svelte";
  import { fmtWaitStart } from "$lib/livewait-logic";
  import {
    queues,
    queueMetaOf,
    queuePanel,
    queueClear,
    queueMove,
    queuePlayStart,
    queueRemoveAt,
    queueShuffleRest,
    queueStop,
    type QueueKey,
  } from "$lib/queue.svelte";

  // 表示対象キュー。選択中のキューが消滅（消費完了・インスタンス消失・
  // 全消去・プレイリスト削除・上書き）していれば一時キューへ戻す
  const selKey = $derived<QueueKey>(
    queues.list.has(queuePanel.selected) ? queuePanel.selected : null,
  );
  const sel = $derived(queues.list.get(selKey));

  // キュータブの選択（待機タブの解除とペア）
  function selectQueue(key: QueueKey): void {
    queuePanel.selected = key;
    queuePanel.waitsTab = false;
  }

  // 上下ボタンと共通の移動。実行中は queueMove 側で武装を張り替える
  let dragFrom = $state<number | null>(null);
  let dragOver = $state<number | null>(null);

  function drop(to: number): void {
    if (dragFrom !== null && dragFrom !== to) queueMove(selKey, dragFrom, to);
    dragFrom = null;
    dragOver = null;
  }

  async function start(): Promise<void> {
    try {
      await queuePlayStart(selKey);
      goto("/");
    } catch (e) {
      notify(
        t("queue.startFailed", { message: asErrorMessage(e) }),
      );
    }
  }
</script>

<aside class="queue-panel" aria-label={t("queue.panel.title")}>
  <div class="qp-tabs">
    <button
      class="qp-tab"
      class:active={!queuePanel.waitsTab && selKey === null}
      onclick={() => selectQueue(null)}
    >
      {t("queue.tab.temp")}
    </button>
    {#each [...queues.list.entries()] as [key, q] (key)}
      {#if key !== null}
        <button
          class="qp-tab"
          class:active={!queuePanel.waitsTab && selKey === key}
          title={q.playlistName}
          onclick={() => selectQueue(key)}
        >
          {q.playlistName}
        </button>
      {/if}
    {/each}
    <!-- ライブ待機タブ（FR-44、仕様決定 BT(2)）。キュータブの隣に置く -->
    <button
      class="qp-tab"
      class:active={queuePanel.waitsTab}
      onclick={() => (queuePanel.waitsTab = true)}
    >
      {t("livewait.tab")}
    </button>
  </div>

  {#if queuePanel.waitsTab}
    <div class="qp-head">
      <h3>
        {t("livewait.tab")}
        <span class="qp-count">{liveWaits.list.length}</span>
      </h3>
      <button class="link" onclick={() => (queuePanel.open = false)}>×</button>
    </div>
    {#if liveWaits.list.length === 0}
      <p class="subtle">{t("livewait.empty")}</p>
    {:else}
      <ul class="qp-list">
        {#each liveWaits.list as w (w.videoId)}
          <li>
            <span class="qp-text">
              <span class="qp-title" title={w.title}>{w.title}</span>
              {#if w.channelTitle}
                <span class="qp-ch">{w.channelTitle}</span>
              {/if}
              <span class="qp-ch">
                {fmtWaitStart(w.scheduledStartMs) || t("livewait.scheduledUnknown")}
              </span>
            </span>
            <button
              class="link danger"
              title={t("livewait.remove")}
              onclick={() => void liveWaitRemove(w.videoId)}>×</button
            >
          </li>
        {/each}
      </ul>
    {/if}
  {:else}
  <div class="qp-head">
    <h3>
      {t("queue.panel.title")}
      <span class="qp-count">{sel?.items.length ?? 0}</span>
    </h3>
    {#if sel !== undefined && sel.instanceId === null}
      <button
        class="link"
        disabled={sel.items.length === 0}
        onclick={() => void start()}
      >
        {t("queue.play")}
      </button>
    {:else if sel !== undefined}
      <button class="link" onclick={() => queueStop(selKey)}>
        {t("library.playlist.queueStop")}
      </button>
    {/if}
    <!-- 残りをシャッフル（FR-46、仕様決定 BW）。現在位置より後の項目を
         ランダムに並べ替える。残りが 2 項目未満なら意味を持たないので無効化 -->
    <button
      class="link"
      disabled={sel === undefined || sel.items.length - sel.index - 1 < 2}
      onclick={() => queueShuffleRest(selKey)}
    >
      {t("queue.shuffle")}
    </button>
    <button
      class="link danger"
      disabled={(sel?.items.length ?? 0) === 0}
      onclick={() => queueClear(selKey)}
    >
      {t("queue.clear")}
    </button>
    <button class="link" onclick={() => (queuePanel.open = false)}>×</button>
  </div>

  {#if sel === undefined || sel.items.length === 0}
    <p class="subtle">{t("queue.empty")}</p>
  {:else}
    <ul class="qp-list">
      {#each sel.items as item, i (item.uid)}
        {@const meta = queueMetaOf(item.videoId)}
        <li
          class:playing={sel.instanceId !== null && i === sel.index}
          class:drop-target={dragOver === i && dragFrom !== i}
          draggable="true"
          ondragstart={(e) => {
            dragFrom = i;
            e.dataTransfer?.setData("text/plain", item.videoId);
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
            onclick={() => queueMove(selKey, i, i - 1)}>↑</button
          >
          <button
            class="link"
            title={t("library.playlist.moveDown")}
            disabled={i === sel.items.length - 1}
            onclick={() => queueMove(selKey, i, i + 1)}>↓</button
          >
          <button
            class="link danger"
            title={t("library.remove")}
            onclick={() => queueRemoveAt(selKey, i)}>×</button
          >
        </li>
      {/each}
    </ul>
  {/if}
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

  .qp-tabs {
    display: flex;
    gap: 4px;
    margin-bottom: 8px;
    overflow-x: auto;
    /* overflow-x:auto で min-height が 0 に解決されるため、
       項目数が多いと flex の縮小でタブ行が潰れるのを防ぐ */
    flex-shrink: 0;
  }

  .qp-tab {
    padding: 4px 10px;
    border: 1px solid #3c4043;
    border-radius: 999px;
    background: none;
    color: #9aa0a6;
    font-size: 0.8rem;
    cursor: pointer;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 160px;
  }

  .qp-tab.active {
    color: #8ab4f8;
    border-color: #8ab4f8;
  }

  .qp-head {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-shrink: 0;
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
</style>
