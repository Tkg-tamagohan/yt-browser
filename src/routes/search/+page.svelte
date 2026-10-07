<script lang="ts">
  import { goto } from "$app/navigation";
  import { invoke } from "@tauri-apps/api/core";
  import { t } from "$lib/i18n";
  import { notify } from "$lib/notices.svelte";
  import type { SearchResult, UiError } from "$lib/players.svelte";

  let query = $state("");
  let results = $state<SearchResult[]>([]);
  let searching = $state(false);
  let searchedOnce = $state(false);

  function asErrorMessage(e: unknown): string {
    if (typeof e === "object" && e !== null && "message" in e) {
      return String((e as UiError).message);
    }
    return String(e);
  }

  function fmtDuration(sec: number | null): string {
    if (sec === null) return "";
    const h = Math.floor(sec / 3600);
    const m = Math.floor((sec % 3600) / 60);
    const s = sec % 60;
    const mm = h > 0 ? String(m).padStart(2, "0") : String(m);
    return `${h > 0 ? h + ":" : ""}${mm}:${String(s).padStart(2, "0")}`;
  }

  function fmtViews(n: number | null): string {
    if (n === null) return "";
    if (n >= 100_000_000) return `${(n / 100_000_000).toFixed(1)}${t("search.views.oku")}`;
    if (n >= 10_000) return `${(n / 10_000).toFixed(1)}${t("search.views.man")}`;
    return t("search.views.count", { count: n.toLocaleString() });
  }

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
    if (!r.channelId) return;
    try {
      await invoke("subscribe_channel", { input: r.channelId, categoryId: null });
      notify(t("feed.subscribed", { title: r.channelTitle ?? r.channelId }));
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
      <li class="result">
        {#if r.thumbnailUrl}
          <img class="thumb" src={r.thumbnailUrl} alt="" />
        {/if}
        <div class="meta">
          <div class="title">{r.title}</div>
          <div class="sub">
            {#if r.channelTitle}{r.channelTitle}{/if}
            {#if fmtViews(r.viewCount)}・{fmtViews(r.viewCount)}{/if}
            {#if fmtDuration(r.durationSec)}・{fmtDuration(r.durationSec)}{/if}
          </div>
          <div class="actions">
            <button onclick={() => playItem(r)}>{t("search.play")}</button>
            {#if r.channelId}
              <button onclick={() => subscribe(r)}>{t("search.subscribe")}</button>
              <button class="danger" onclick={() => blockChannel(r)}>
                {t("search.block")}
              </button>
            {/if}
          </div>
        </div>
      </li>
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

  .result {
    display: flex;
    gap: 12px;
    padding: 10px 0;
    border-bottom: 1px solid #3c4043;
    align-items: flex-start;
  }

  .thumb {
    width: 160px;
    aspect-ratio: 16 / 9;
    object-fit: cover;
    border-radius: 8px;
    background: #26282c;
  }

  .meta {
    flex: 1;
    min-width: 0;
  }

  .title {
    font-weight: 600;
    overflow-wrap: anywhere;
  }

  .sub {
    color: #9aa0a6;
    font-size: 0.85rem;
    margin: 4px 0 8px;
  }

  .actions {
    display: flex;
    gap: 8px;
  }

  .actions .danger {
    color: #ff7b72;
  }
</style>
