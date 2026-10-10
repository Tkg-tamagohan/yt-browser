<script lang="ts">
  // プレイリストタブ（FR-7, FR-10, FR-11, FR-25, FR-26）。
  // 状態・取得・編集ロジックはページ側のファクトリ（lib）が持ち、
  // このコンポーネントは表示とイベント配線だけを受け持つ。
  // 参照は props 爆発を避けるため lib.x へ直接アクセスする（D-2 方式）
  import { t } from "$lib/i18n";
  import type { createLibraryPageState } from "$lib/library-page.svelte";
  import QueueButtons from "$lib/QueueButtons.svelte";
  import VideoRow from "$lib/VideoRow.svelte";
  import { videoRefOf } from "$lib/video-actions.svelte";
  import { queueAt, queuePlayingAt, queueStop } from "$lib/queue.svelte";

  type Props = {
    lib: ReturnType<typeof createLibraryPageState>;
    play: (videoId: string, resume: boolean) => Promise<void>;
  };
  let { lib, play }: Props = $props();
</script>

    <div class="pl-grid">
      <section class="pl-list">
        <div class="pl-new">
          <input
            type="text"
            bind:value={lib.newPlaylistName}
            placeholder={t("library.playlist.newPlaceholder")}
            onkeydown={(e) => e.key === "Enter" && void lib.createPlaylist()}
          />
          <button onclick={() => void lib.createPlaylist()} disabled={!lib.newPlaylistName.trim()}>
            {t("library.playlist.create")}
          </button>
        </div>
        <div class="pl-new">
          <input
            type="text"
            bind:value={lib.importUrl}
            placeholder={t("library.playlist.importUrl")}
            onkeydown={(e) => e.key === "Enter" && void lib.importPlaylist()}
          />
          <input
            type="text"
            bind:value={lib.importName}
            placeholder={t("library.playlist.importName")}
            onkeydown={(e) => e.key === "Enter" && void lib.importPlaylist()}
          />
          <button
            onclick={() => void lib.importPlaylist()}
            disabled={!lib.importUrl.trim() || lib.importing}
          >
            {lib.importing ? t("library.playlist.importing") : t("library.playlist.import")}
          </button>
        </div>
        {#if lib.va.playlists.length === 0}
          <p class="subtle">{t("library.playlists.empty")}</p>
        {:else}
          <ul class="pl-names">
            {#each lib.va.playlists as pl (pl.id)}
              <li class:active={lib.selectedId === pl.id}>
                {#if lib.renamingId === pl.id}
                  <input
                    type="text"
                    bind:value={lib.renameText}
                    onkeydown={(e) => {
                      if (e.key === "Enter") void lib.commitRename(pl);
                      if (e.key === "Escape") lib.renamingId = null;
                    }}
                  />
                  <button class="link" onclick={() => void lib.commitRename(pl)}>OK</button>
                {:else}
                  <button class="pl-name" onclick={() => void lib.selectPlaylist(pl)}>
                    {pl.name}
                    <span class="subtle">{t("library.playlist.items", { count: pl.itemCount })}</span>
                  </button>
                  <button
                    class="link"
                    title={t("library.playlist.rename")}
                    onclick={() => lib.beginRename(pl)}>✎</button
                  >
                  <button
                    class="link danger"
                    title={t("library.playlist.delete")}
                    onclick={() => void lib.deletePlaylist(pl)}>×</button
                  >
                {/if}
              </li>
            {/each}
          </ul>
        {/if}
      </section>
      <section class="pl-items">
        {#if lib.selectedId === null}
          <p class="subtle">{t("library.playlist.selectHint")}</p>
        {:else if lib.playlistItems.length === 0}
          <p class="subtle">{t("library.playlist.empty")}</p>
        {:else}
          <div class="pl-tools">
            {#if lib.selectedId !== null}
              {@const selQueue = queueAt(lib.selectedId)}
              {#if selQueue !== undefined}
                <span class="queue-badge">
                  {#if selQueue.instanceId !== null}
                    {t("library.playlist.queueActive", {
                      name: selQueue.playlistName,
                      index: selQueue.index + 1,
                      count: selQueue.items.length,
                    })}
                    <button class="link" onclick={() => queueStop(lib.selectedId)}
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
              disabled={lib.sortBusy}
              onclick={() => void lib.sortByPublished()}
              >{t("library.playlist.sort")}</button
            >
            <button
              class="link"
              disabled={lib.sortBusy}
              onclick={() => void lib.reverseItems()}
              >{t("library.playlist.reverse")}</button
            >
            <span class="subtle drag-hint">{t("library.playlist.dragHint")}</span>
          </div>
          <ul class="rows">
            {#each lib.playlistItems as it, index (it.videoId)}
              <VideoRow
                videoId={it.videoId}
                title={it.title}
                thumbnailUrl={it.thumbnailUrl}
                onplay={() => play(it.videoId, true)}
                draggable={!lib.sortBusy}
                dropTarget={lib.dragOver === index && lib.dragFrom !== index}
                ondragstart={(e) => lib.itemDragStart(index, e)}
                ondragover={(e) => lib.itemDragOver(index, e)}
                ondragleave={() => lib.itemDragLeave(index)}
                ondrop={(e) => lib.itemDrop(index, e)}
                ondragend={() => lib.itemDragEnd()}
              >
                {#snippet leading()}
                  <span class="pos">{index + 1}</span>
                {/snippet}
                {#snippet sub()}{it.channelTitle ?? ""}{/snippet}
                {#snippet actions()}
                  <button
                    onclick={() => void lib.playQueue(index)}
                    disabled={queuePlayingAt(lib.selectedId, it.videoId)}
                    >{t("library.playlist.queue")}</button
                  >
                  <button onclick={() => play(it.videoId, true)}
                    >{t("library.play")}</button
                  >
                  <button
                    class="link"
                    title={t("library.playlist.moveUp")}
                    disabled={index === 0 || lib.reorderBusy || lib.sortBusy}
                    onclick={() => lib.moveItem(index, -1)}>↑</button
                  >
                  <button
                    class="link"
                    title={t("library.playlist.moveDown")}
                    disabled={index === lib.playlistItems.length - 1 ||
                      lib.reorderBusy ||
                      lib.sortBusy}
                    onclick={() => lib.moveItem(index, 1)}>↓</button
                  >
                  <button
                    class="link danger"
                    onclick={() => lib.removeItem(it)}
                    >{t("library.remove")}</button
                  >
                  <QueueButtons video={videoRefOf(it)} />
                {/snippet}
              </VideoRow>
            {/each}
          </ul>
          {#if lib.plHasMore}
            <button
              class="link load-more"
              onclick={() => void lib.loadMoreItems()}
              disabled={lib.plLoadingMore}
            >
              {lib.plLoadingMore
                ? t("related.loading")
                : t("library.playlist.loadMore")}
            </button>
          {/if}
        {/if}
      </section>
    </div>

<style>
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
