<script lang="ts">
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";
  import { invoke } from "@tauri-apps/api/core";
  import { t } from "$lib/i18n";
  import { fmtDuration, fmtViews } from "$lib/format";
  import { notify } from "$lib/notices.svelte";
  import VideoActions from "$lib/VideoActions.svelte";
  import VideoRow from "$lib/VideoRow.svelte";
  import {
    createVideoActionState,
    videoRefOf,
  } from "$lib/videoActions.svelte";
  import {
    asErrorMessage,
    type SearchResult,
  } from "$lib/players.svelte";

  let query = $state("");
  let results = $state<SearchResult[]>([]);
  let searching = $state(false);
  let searchedOnce = $state(false);

  // お気に入り・プレイリスト行アクション用（FR-7）
  const va = createVideoActionState();

  onMount(async () => {
    await va.refresh();
  });

  async function doSearch(): Promise<void> {
    const q = query.trim();
    if (!q) return;
    searching = true;
    try {
      results = await invoke<SearchResult[]>("search", { query: q });
      searchedOnce = true;
    } catch (e) {
      notify(t("search.failed", { message: asErrorMessage(e) }));
    }
    searching = false;
  }

  async function playItem(r: SearchResult): Promise<void> {
    try {
      await invoke("play_video", { videoId: r.videoId, resume: true });
      goto("/");
    } catch (e) {
      notify(t("player.error", { message: asErrorMessage(e) }));
    }
  }

  async function subscribe(r: SearchResult): Promise<void> {
    // subscribe_channel は UC ID だけでなく @handle も解決できる。
    // UC が取れない結果でも @handle があれば購読導線を出す。
    const input = r.channelId ?? r.uploaderId;
    if (!input) return;
    try {
      await invoke("subscribe_channel", { input, categoryId: null });
      notify(t("feed.subscribed", { title: r.channelTitle ?? input }));
    } catch (e) {
      notify(t("feed.subscribeFailed", { message: asErrorMessage(e) }));
    }
  }

  async function blockChannel(r: SearchResult): Promise<void> {
    if (!r.channelId) return;
    try {
      await invoke("block_channel", {
        channelId: r.channelId,
        title: r.channelTitle ?? r.channelId,
      });
      results = results.filter((x) => x.channelId !== r.channelId);
      notify(
        t("blocked.added", { title: r.channelTitle ?? r.channelId }),
      );
    } catch (e) {
      notify(t("blocked.addFailed", { message: asErrorMessage(e) }));
    }
  }
</script>

<main class="container">
  <h1>{t("search.title")}</h1>

  <form
    class="search-form"
    onsubmit={(e) => {
      e.preventDefault();
      void doSearch();
    }}
  >
    <input
      type="text"
      placeholder={t("search.placeholder")}
      bind:value={query}
    />
    <button type="submit" disabled={searching || !query.trim()}>
      {searching ? t("search.searching") : t("search.button")}
    </button>
  </form>

  {#if searchedOnce && results.length === 0 && !searching}
    <p class="empty">{t("search.empty")}</p>
  {/if}

  <ul class="results">
    {#each results as r (r.videoId)}
      <VideoRow
        videoId={r.videoId}
        title={r.title}
        thumbnailUrl={r.thumbnailUrl}
        thumbSize="lg"
        onplay={() => playItem(r)}
      >
        {#snippet sub()}
          {#if r.channelTitle}{r.channelTitle}{/if}
          {#if fmtViews(r.viewCount)}・{fmtViews(r.viewCount)}{/if}
          {#if fmtDuration(r.durationSec)}・{fmtDuration(r.durationSec)}{/if}
        {/snippet}
        {#snippet actions()}
          <button onclick={() => playItem(r)}>{t("search.play")}</button>
          {#if r.channelId || r.uploaderId}
            <button onclick={() => subscribe(r)}>{t("search.subscribe")}</button>
          {/if}
          {#if r.channelId}
            <button class="danger" onclick={() => blockChannel(r)}>
              {t("search.block")}
            </button>
          {/if}
          <VideoActions
            video={videoRefOf(r)}
            faved={va.favIds.has(r.videoId)}
            playlists={va.playlists}
            onfavchange={va.onFavChange}
            onplaylistcreated={va.onPlaylistCreated}
          />
        {/snippet}
      </VideoRow>
    {/each}
  </ul>
</main>

<style>
  .search-form {
    display: flex;
    gap: 8px;
    margin-bottom: 16px;
  }

  .search-form input {
    flex: 1;
  }

  .empty {
    color: #9aa0a6;
  }

  .results {
    list-style: none;
    padding: 0;
    margin: 0;
  }
</style>
