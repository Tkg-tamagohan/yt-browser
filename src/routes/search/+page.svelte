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
  } from "$lib/video-actions.svelte";
  import {
    asErrorMessage,
    type SearchResult,
  } from "$lib/players.svelte";

  let query = $state("");
  let results = $state<SearchResult[]>([]);
  let searching = $state(false);
  let searchedOnce = $state(false);
  // 「さらに読み込む」の状態（FR-25、仕様決定 AR）。
  // ytsearch に継続が無いため、要求件数を段階的に増やして再取得し、
  // 既表示と重複しない分だけ追記する
  let searchedQuery = $state("");
  let shownLimit = $state(0);
  let hasMore = $state(false);
  const SEARCH_PAGE = 20;
  // バックエンド search の件数上限（SEARCH_COUNT_MAX）に合わせる。
  // 超過を要求しても丸められるため、到達時点でボタンを隠す
  const SEARCH_MAX = 500;

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
      const res = await invoke<SearchResult[]>("search", {
        query: q,
        count: SEARCH_PAGE,
      });
      results = res;
      searchedQuery = q;
      shownLimit = SEARCH_PAGE;
      hasMore = res.length >= SEARCH_PAGE;
      searchedOnce = true;
    } catch (e) {
      notify(t("search.failed", { message: asErrorMessage(e) }));
    }
    searching = false;
  }

  async function loadMore(): Promise<void> {
    if (searching || !searchedQuery) return;
    searching = true;
    const nextLimit = Math.min(shownLimit + SEARCH_PAGE, SEARCH_MAX);
    try {
      const res = await invoke<SearchResult[]>("search", {
        query: searchedQuery,
        count: nextLimit,
      });
      const seen = new Set(results.map((r) => r.videoId));
      results = [...results, ...res.filter((r) => !seen.has(r.videoId))];
      shownLimit = nextLimit;
      hasMore = res.length >= nextLimit && nextLimit < SEARCH_MAX;
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
    <p class="subtle">{t("search.empty")}</p>
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

  {#if hasMore}
    <button class="load-more" onclick={loadMore} disabled={searching}>
      {searching ? t("search.searching") : t("search.loadMore")}
    </button>
  {/if}
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
