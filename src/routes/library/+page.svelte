<script lang="ts">
  // ローカルデータ画面（FR-7）: 視聴履歴・お気に入り・プレイリストの
  // 一覧・編集・削除をここで完結させる。
  import { onDestroy, onMount } from "svelte";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { t } from "$lib/i18n";
  import { fmtDateTime } from "$lib/format";
  import { notify } from "$lib/notices.svelte";
  import { createLibraryPageState } from "$lib/library-page.svelte";
  import QueueButtons from "$lib/QueueButtons.svelte";
  import VideoActions from "$lib/VideoActions.svelte";
  import VideoRow from "$lib/VideoRow.svelte";
  import { videoRefOf } from "$lib/video-actions.svelte";
  import { asErrorMessage, type Playlist } from "$lib/players.svelte";
  import { queueAt, queuePlayingAt, queueStop } from "$lib/queue.svelte";

  type Tab = "history" | "favorites" | "playlists";
  let tab = $state<Tab>("history");

  // 状態・取得・編集ロジックはファクトリ側（$lib/library-page.svelte.ts）
  // へ集約し、ここではタブ状態とイベント購読だけを管理する
  const lib = createLibraryPageState({
    goToPlaylistsTab: () => {
      tab = "playlists";
    },
  });

  // 読み取り専用の参照は $derived でエイリアス化し、マークアップ側の
  // 識別子を現行のまま維持する（$state の分割代入は初回値で固定
  // されるため getter 経由にする）。書き込みが必要な入力
  // （bind:value と renamingId の解除）だけ lib.x で直接触る
  const history = $derived(lib.history);
  const favorites = $derived(lib.favorites);
  const selectedId = $derived(lib.selectedId);
  const playlistItems = $derived(lib.playlistItems);
  const renamingId = $derived(lib.renamingId);
  const newPlaylistName = $derived(lib.newPlaylistName);
  const importUrl = $derived(lib.importUrl);
  const importing = $derived(lib.importing);
  const plHasMore = $derived(lib.plHasMore);
  const plLoadingMore = $derived(lib.plLoadingMore);
  const dragFrom = $derived(lib.dragFrom);
  const dragOver = $derived(lib.dragOver);
  const reorderBusy = $derived(lib.reorderBusy);
  const sortBusy = $derived(lib.sortBusy);

  const {
    va,
    progressOf,
    loadHistory,
    loadFavorites,
    play,
    removeHistory,
    removeFavorite,
    createPlaylist,
    beginRename,
    commitRename,
    deletePlaylist,
    selectPlaylist,
    loadMoreItems,
    onPlaylistsChanged,
    onPlaylistAdd,
    importPlaylist,
    moveItem,
    itemDragStart,
    itemDragOver,
    itemDragLeave,
    itemDrop,
    itemDragEnd,
    sortByPublished,
    reverseItems,
    playQueue,
    removeItem,
  } = lib;

  // 画面外からのプレイリスト変更通知（library://playlists_changed、
  // 設計書 §3.2）の購読解除関数。マウント中だけ listen する
  let unlistens: UnlistenFn[] = [];

  onMount(async () => {
    // 購読の失敗で初回ロードまで止めないよう、独立して試す
    try {
      unlistens.push(
        await listen<Playlist>("library://playlists_changed", (ev) => {
          void onPlaylistsChanged(ev.payload);
        }),
      );
    } catch (e) {
      notify(t("library.failed", { message: asErrorMessage(e) }));
    }
    try {
      await Promise.all([loadHistory(), loadFavorites()]);
    } catch (e) {
      notify(t("library.failed", { message: asErrorMessage(e) }));
    }
  });

  onDestroy(() => {
    unlistens.forEach((u) => u());
  });
</script>

<main class="container library">
  <h1>{t("library.title")}</h1>

  <nav class="tabs">
    {#each ["history", "favorites", "playlists"] as const as tb}
      <button
        class="tab"
        class:active={tab === tb}
        onclick={() => (tab = tb)}
      >
        {#if tb === "history"}{t("library.tab.history")}
        {:else if tb === "favorites"}{t("library.tab.favorites")}
        {:else}{t("library.tab.playlists")}{/if}
      </button>
    {/each}
  </nav>

  {#if tab === "history"}
    {#if history.length === 0}
      <p class="subtle">{t("library.history.empty")}</p>
    {:else}
      <ul class="rows">
        {#each history as h (h.videoId)}
          <VideoRow
            videoId={h.videoId}
            title={h.title || h.videoId}
            onplay={() => play(h.videoId, true)}
          >
            {#snippet sub()}
              {#if h.channelTitle}{h.channelTitle}{/if}
              {#if progressOf(h)}・{progressOf(h)}{/if}
              {#if h.lastWatchedAt}
                ・{t("library.history.lastWatched", { at: fmtDateTime(h.lastWatchedAt) })}
              {/if}
            {/snippet}
            {#snippet actions()}
              <button onclick={() => play(h.videoId, true)}
                >{t("library.play")}</button
              >
              {#if !h.completed && h.positionSec > 0}
                <button class="link" onclick={() => play(h.videoId, false)}
                  >{t("library.playFromStart")}</button
                >
              {/if}
              <button
                class="link danger"
                onclick={() => removeHistory(h.videoId)}
                >{t("library.remove")}</button
              >
            {/snippet}
          </VideoRow>
        {/each}
      </ul>
    {/if}
  {:else if tab === "favorites"}
    {#if favorites.length === 0}
      <p class="subtle">{t("library.favorite.empty")}</p>
    {:else}
      <ul class="rows">
        {#each favorites as f (f.videoId)}
          <VideoRow
            videoId={f.videoId}
            title={f.title}
            thumbnailUrl={f.thumbnailUrl}
            onplay={() => play(f.videoId, true)}
          >
            {#snippet sub()}
              {#if f.channelTitle}{f.channelTitle}{/if}
              {#if f.addedAt}・{fmtDateTime(f.addedAt)}{/if}
            {/snippet}
            {#snippet actions()}
              <button onclick={() => play(f.videoId, true)}
                >{t("library.play")}</button
              >
              <button
                class="link danger"
                onclick={() => removeFavorite(f.videoId)}
                >{t("library.favorite.remove")}</button
              >
              <VideoActions
                video={videoRefOf(f)}
                faved={true}
                playlists={va.playlists}
                onfavchange={va.onFavChange}
                onplaylistcreated={va.onPlaylistCreated}
                onplaylistadd={onPlaylistAdd}
              />
            {/snippet}
          </VideoRow>
        {/each}
      </ul>
    {/if}
  {:else}
    <div class="pl-grid">
      <section class="pl-list">
        <div class="pl-new">
          <input
            type="text"
            bind:value={lib.newPlaylistName}
            placeholder={t("library.playlist.newPlaceholder")}
            onkeydown={(e) => e.key === "Enter" && void createPlaylist()}
          />
          <button onclick={() => void createPlaylist()} disabled={!newPlaylistName.trim()}>
            {t("library.playlist.create")}
          </button>
        </div>
        <div class="pl-new">
          <input
            type="text"
            bind:value={lib.importUrl}
            placeholder={t("library.playlist.importUrl")}
            onkeydown={(e) => e.key === "Enter" && void importPlaylist()}
          />
          <input
            type="text"
            bind:value={lib.importName}
            placeholder={t("library.playlist.importName")}
            onkeydown={(e) => e.key === "Enter" && void importPlaylist()}
          />
          <button
            onclick={() => void importPlaylist()}
            disabled={!importUrl.trim() || importing}
          >
            {importing ? t("library.playlist.importing") : t("library.playlist.import")}
          </button>
        </div>
        {#if va.playlists.length === 0}
          <p class="subtle">{t("library.playlists.empty")}</p>
        {:else}
          <ul class="pl-names">
            {#each va.playlists as pl (pl.id)}
              <li class:active={selectedId === pl.id}>
                {#if renamingId === pl.id}
                  <input
                    type="text"
                    bind:value={lib.renameText}
                    onkeydown={(e) => {
                      if (e.key === "Enter") void commitRename(pl);
                      if (e.key === "Escape") lib.renamingId = null;
                    }}
                  />
                  <button class="link" onclick={() => void commitRename(pl)}>OK</button>
                {:else}
                  <button class="pl-name" onclick={() => void selectPlaylist(pl)}>
                    {pl.name}
                    <span class="subtle">{t("library.playlist.items", { count: pl.itemCount })}</span>
                  </button>
                  <button
                    class="link"
                    title={t("library.playlist.rename")}
                    onclick={() => beginRename(pl)}>✎</button
                  >
                  <button
                    class="link danger"
                    title={t("library.playlist.delete")}
                    onclick={() => void deletePlaylist(pl)}>×</button
                  >
                {/if}
              </li>
            {/each}
          </ul>
        {/if}
      </section>
      <section class="pl-items">
        {#if selectedId === null}
          <p class="subtle">{t("library.playlist.selectHint")}</p>
        {:else if playlistItems.length === 0}
          <p class="subtle">{t("library.playlist.empty")}</p>
        {:else}
          <div class="pl-tools">
            {#if selectedId !== null}
              {@const selQueue = queueAt(selectedId)}
              {#if selQueue !== undefined}
                <span class="queue-badge">
                  {#if selQueue.instanceId !== null}
                    {t("library.playlist.queueActive", {
                      name: selQueue.playlistName,
                      index: selQueue.index + 1,
                      count: selQueue.items.length,
                    })}
                    <button class="link" onclick={() => queueStop(selectedId)}
                      >{t("library.playlist.queueStop")}</button
                    >
                  {:else}
                    {t("library.playlist.queueSaved", {
                      name: selQueue.playlistName,
                      index: selQueue.index + 1,
                      count: selQueue.items.length,
                    })}
                  {/if}
                </span>
              {/if}
            {/if}
            <button
              class="link"
              disabled={sortBusy}
              onclick={() => void sortByPublished()}
              >{t("library.playlist.sort")}</button
            >
            <button
              class="link"
              disabled={sortBusy}
              onclick={() => void reverseItems()}
              >{t("library.playlist.reverse")}</button
            >
            <span class="subtle drag-hint">{t("library.playlist.dragHint")}</span>
          </div>
          <ul class="rows">
            {#each playlistItems as it, index (it.videoId)}
              <VideoRow
                videoId={it.videoId}
                title={it.title}
                thumbnailUrl={it.thumbnailUrl}
                onplay={() => play(it.videoId, true)}
                draggable={!sortBusy}
                dropTarget={dragOver === index && dragFrom !== index}
                ondragstart={(e) => itemDragStart(index, e)}
                ondragover={(e) => itemDragOver(index, e)}
                ondragleave={() => itemDragLeave(index)}
                ondrop={(e) => itemDrop(index, e)}
                ondragend={() => itemDragEnd()}
              >
                {#snippet leading()}
                  <span class="pos">{index + 1}</span>
                {/snippet}
                {#snippet sub()}{it.channelTitle ?? ""}{/snippet}
                {#snippet actions()}
                  <button
                    onclick={() => void playQueue(index)}
                    disabled={queuePlayingAt(selectedId, it.videoId)}
                    >{t("library.playlist.queue")}</button
                  >
                  <button onclick={() => play(it.videoId, true)}
                    >{t("library.play")}</button
                  >
                  <button
                    class="link"
                    title={t("library.playlist.moveUp")}
                    disabled={index === 0 || reorderBusy || sortBusy}
                    onclick={() => moveItem(index, -1)}>↑</button
                  >
                  <button
                    class="link"
                    title={t("library.playlist.moveDown")}
                    disabled={index === playlistItems.length - 1 ||
                      reorderBusy ||
                      sortBusy}
                    onclick={() => moveItem(index, 1)}>↓</button
                  >
                  <button
                    class="link danger"
                    onclick={() => removeItem(it)}
                    >{t("library.remove")}</button
                  >
                  <QueueButtons video={videoRefOf(it)} />
                {/snippet}
              </VideoRow>
            {/each}
          </ul>
          {#if plHasMore}
            <button
              class="link load-more"
              onclick={() => void loadMoreItems()}
              disabled={plLoadingMore}
            >
              {plLoadingMore
                ? t("related.loading")
                : t("library.playlist.loadMore")}
            </button>
          {/if}
        {/if}
      </section>
    </div>
  {/if}
</main>

<style>
  .library {
    max-width: 860px;
  }

  .tabs {
    display: flex;
    gap: 4px;
    margin: 12px 0 16px;
    border-bottom: 1px solid #3c4043;
  }

  .tab {
    padding: 8px 16px;
    border: none;
    border-bottom: 2px solid transparent;
    background: none;
    color: #9aa0a6;
    cursor: pointer;
  }

  .tab.active {
    color: #e8e8e8;
    border-bottom-color: #8ab4f8;
  }

  .rows {
    list-style: none;
    margin: 0;
    padding: 0;
  }

  .pos {
    width: 24px;
    color: #9aa0a6;
    text-align: right;
    flex-shrink: 0;
  }

  .pl-grid {
    display: grid;
    grid-template-columns: 260px 1fr;
    gap: 16px;
  }

  .pl-new {
    display: flex;
    gap: 6px;
    margin-bottom: 10px;
  }

  .pl-new input {
    flex: 1;
    min-width: 0;
  }

  .pl-names {
    list-style: none;
    margin: 0;
    padding: 0;
  }

  .pl-names li {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 4px 0;
    border-bottom: 1px solid #2d2f33;
  }

  .pl-names li.active .pl-name {
    color: #8ab4f8;
  }

  .pl-name {
    flex: 1;
    min-width: 0;
    padding: 6px 4px;
    border: none;
    background: none;
    color: #e8e8e8;
    text-align: left;
    cursor: pointer;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .pl-name .subtle {
    margin-left: 6px;
    font-size: 0.8rem;
  }

  .pl-items {
    min-width: 0;
  }

  .pl-tools {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-bottom: 8px;
  }

  .queue-badge {
    color: #8ab4f8;
    font-size: 0.85rem;
  }

  .drag-hint {
    font-size: 0.8rem;
    margin-left: auto;
  }

  @media (max-width: 700px) {
    .pl-grid {
      grid-template-columns: 1fr;
    }
  }
</style>
