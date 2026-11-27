<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { t } from "$lib/i18n";
  import { fmtDuration, fmtViews } from "$lib/format";
  import { notify } from "$lib/notices.svelte";
  import { navigateOnPlay } from "$lib/ui-prefs";
  import VideoActions from "$lib/VideoActions.svelte";
  import VideoRow from "$lib/VideoRow.svelte";
  import {
    createVideoActionState,
    openVideoContextMenu,
    videoRefOf,
  } from "$lib/video-actions.svelte";
  import {
    asErrorMessage,
    type SearchPage,
    type SearchResult,
  } from "$lib/players.svelte";
  import {
    applyInitialSearch,
    applyLoadMoreSearch,
    consumePendingOnly,
    emptySearchPagination,
    nextSearchCount,
    removeBlockedChannelFromSearch,
    SEARCH_PAGE,
    shouldFetchMoreFromBackend,
  } from "$lib/search-page-logic";

  let query = $state("");
  let searchState = $state(emptySearchPagination());
  let results = $derived(searchState.results);
  let searching = $state(false);
  let searchedOnce = $state(false);
  // 「さらに読み込む」の状態（FR-25・FR-100、仕様決定 AR・EF）。
  // 累積 count を段階的に増やして再取得し、1 回の追記件数と最大件数（SEARCH_MAX）を制限しつつ
  // 余剰候補は pending バッファで次クリックへ繰り越す
  let searchedQuery = $state("");
  let hasMore = $derived(searchState.hasMore);

  // お気に入り・プレイリスト行アクション用（FR-7）
  const va = createVideoActionState();

  onMount(async () => {
    await va.refresh();
  });

  async function doSearch(): Promise<void> {
    const q = query.trim();
    if (!q || searching) return;
    searching = true;
    try {
      const res = await invoke<SearchPage>("search", {
        query: q,
        count: SEARCH_PAGE,
      });
      searchState = applyInitialSearch(res);
      searchedQuery = q;
      searchedOnce = true;
    } catch (e) {
      notify(t("search.failed", { message: asErrorMessage(e) }));
    }
    searching = false;
  }

  async function loadMore(): Promise<void> {
    if (searching || !searchedQuery || !searchState.hasMore) return;
    if (!shouldFetchMoreFromBackend(searchState)) {
      searchState = consumePendingOnly(searchState);
      return;
    }
    searching = true;
    const nextLimit = nextSearchCount(searchState);
    try {
      const res = await invoke<SearchPage>("search", {
        query: searchedQuery,
        count: nextLimit,
      });
      searchState = applyLoadMoreSearch(searchState, res, nextLimit);
    } catch (e) {
      notify(t("search.failed", { message: asErrorMessage(e) }));
    }
    searching = false;
  }

  async function playItem(r: SearchResult): Promise<void> {
    try {
      await invoke("play_video", { videoId: r.videoId, resume: true });
      navigateOnPlay();
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
      searchState = removeBlockedChannelFromSearch(searchState, r.channelId);
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

  <!-- 空表示と「さらに読み込む」はリマウントを避けるため hidden で
       出し分ける（ヒットずれ対策、FR-80） -->
  <p
    class="subtle"
    class:hidden={!(searchedOnce && results.length === 0 && !searching)}
  >
    {t("search.empty")}
  </p>

  <ul class="results">
    {#each results as r (r.videoId)}
      <VideoRow
        videoId={r.videoId}
        title={r.title}
        thumbnailUrl={r.thumbnailUrl}
        thumbSize="lg"
        onplay={() => playItem(r)}
        oncontextmenu={(e) => openVideoContextMenu(e, videoRefOf(r), va)}
      >
        {#snippet sub()}
          {#if r.channelTitle}{r.channelTitle}{/if}
          {#if fmtViews(r.viewCount)}・{fmtViews(r.viewCount)}{/if}
          {#if fmtDuration(r.durationSec)}・{fmtDuration(r.durationSec)}{/if}
        {/snippet}
        {#snippet actions()}
          <button onclick={() => playItem(r)}>{t("search.play")}</button>
          <button
            class:hidden={!(r.channelId || r.uploaderId)}
            onclick={() => subscribe(r)}>{t("search.subscribe")}</button
          >
          <button
            class="danger"
            class:hidden={!r.channelId}
            onclick={() => blockChannel(r)}
          >
            {t("search.block")}
          </button>
          <VideoActions
            video={videoRefOf(r)}
            faved={va.favIds.has(r.videoId)}
            playlists={va.playlists}
            watchLater={va.watchLaterIds.has(r.videoId)}
            livewait
            onfavchange={va.onFavChange}
            onplaylistcreated={va.onPlaylistCreated}
            onwatchlaterchange={va.onWatchLaterChange}
          />
        {/snippet}
      </VideoRow>
    {/each}
  </ul>

  <button
    class="load-more"
    class:hidden={!hasMore}
    onclick={loadMore}
    disabled={searching}
  >
    {searching ? t("search.searching") : t("search.loadMore")}
  </button>
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

  .results {
    list-style: none;
    padding: 0;
    margin: 0;
  }
</style>
