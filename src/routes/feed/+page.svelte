<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { goto } from "$app/navigation";
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { t } from "$lib/i18n";

  interface Channel {
    channelId: string;
    title: string;
    thumbnailUrl: string | null;
    categoryId: number | null;
    subscribedAt: string;
    lastPolledAt: string | null;
  }
  interface Category {
    id: number;
    name: string;
    sortOrder: number;
  }
  interface FeedItem {
    videoId: string;
    channelId: string;
    channelTitle: string | null;
    title: string;
    thumbnailUrl: string | null;
    publishedAt: string | null;
    kind: string;
    isRead: boolean;
  }
  interface FeedNewItems {
    count: number;
  }
  interface FeedStatus {
    channelId: string | null;
    level: string;
    message: string;
  }
  interface UiError {
    code?: string;
    message?: string;
  }

  let items = $state<FeedItem[]>([]);
  let channels = $state<Channel[]>([]);
  let categories = $state<Category[]>([]);
  let subInput = $state("");
  let subCategory = $state<number | null>(null);
  let newCatName = $state("");
  let unreadOnly = $state(true);
  // null=すべて / 0=未分類 / n=カテゴリ id
  let filterCat = $state<number | null>(null);
  let busy = $state(false);
  let notices = $state<string[]>([]);
  let unlistens: UnlistenFn[] = [];

  function notify(msg: string): void {
    notices = [...notices.slice(-4), msg];
    setTimeout(() => {
      notices = notices.filter((n) => n !== msg);
    }, 6000);
  }

  function asErrorMessage(e: unknown): string {
    if (typeof e === "object" && e !== null && "message" in e) {
      return String((e as UiError).message);
    }
    return String(e);
  }

  async function loadItems(): Promise<void> {
    items = await invoke<FeedItem[]>("list_feed", {
      filter: {
        unreadOnly,
        categoryId: filterCat,
        days: null,
      },
    });
  }

  async function refreshAll(): Promise<void> {
    const [chs, cats] = await Promise.all([
      invoke<Channel[]>("list_channels"),
      invoke<Category[]>("list_categories"),
    ]);
    channels = chs;
    categories = cats;
    await loadItems();
  }

  async function subscribe(): Promise<void> {
    const input = subInput.trim();
    if (!input || busy) return;
    busy = true;
    try {
      const ch = await invoke<Channel>("subscribe_channel", {
        input,
        categoryId: subCategory,
      });
      subInput = "";
      notify(t("feed.subscribed", { title: ch.title }));
      await refreshAll();
    } catch (e) {
      notify(t("feed.subscribeFailed", { message: asErrorMessage(e) }));
    } finally {
      busy = false;
    }
  }

  async function unsubscribe(ch: Channel): Promise<void> {
    try {
      await invoke("unsubscribe_channel", { channelId: ch.channelId });
      channels = channels.filter((c) => c.channelId !== ch.channelId);
      notify(t("feed.unsubscribed", { title: ch.title }));
      await loadItems();
    } catch (e) {
      notify(t("feed.failed", { message: asErrorMessage(e) }));
    }
  }

  async function setCategory(ch: Channel, categoryId: number | null): Promise<void> {
    try {
      await invoke("set_channel_category", {
        channelId: ch.channelId,
        categoryId,
      });
      ch.categoryId = categoryId;
    } catch (e) {
      notify(t("feed.failed", { message: asErrorMessage(e) }));
    }
  }

  async function createCategory(): Promise<void> {
    const name = newCatName.trim();
    if (!name) return;
    try {
      const cat = await invoke<Category>("create_category", { name });
      categories = [...categories, cat];
      newCatName = "";
      notify(t("feed.categoryAdded", { name: cat.name }));
    } catch (e) {
      notify(t("feed.failed", { message: asErrorMessage(e) }));
    }
  }

  async function markRead(videoId: string): Promise<void> {
    try {
      await invoke("mark_read", { videoIds: [videoId] });
      const it = items.find((i) => i.videoId === videoId);
      if (it) it.isRead = true;
      if (unreadOnly) items = items.filter((i) => i.videoId !== videoId);
    } catch (e) {
      notify(t("feed.failed", { message: asErrorMessage(e) }));
    }
  }

  async function markAllRead(): Promise<void> {
    try {
      await invoke("mark_read", { all: true });
      await loadItems();
      notify(t("feed.markedAllRead"));
    } catch (e) {
      notify(t("feed.failed", { message: asErrorMessage(e) }));
    }
  }

  async function playItem(it: FeedItem): Promise<void> {
    // 再生した項目は既読にする（個別既読の一形態として）
    try {
      await invoke("mark_read", { videoIds: [it.videoId] });
      await invoke("play_video", { videoId: it.videoId, resume: false });
      goto("/");
    } catch (e) {
      notify(t("feed.failed", { message: asErrorMessage(e) }));
    }
  }

  async function refreshNow(): Promise<void> {
    try {
      await invoke("feed_refresh", {});
      notify(t("feed.refreshQueued"));
    } catch (e) {
      notify(t("feed.failed", { message: asErrorMessage(e) }));
    }
  }

  function formatPublished(iso: string | null): string {
    if (!iso) return "";
    const d = new Date(iso);
    if (Number.isNaN(d.getTime())) return iso;
    return d.toLocaleString("ja-JP");
  }

  onMount(async () => {
    await refreshAll();
    unlistens.push(
      await listen<FeedNewItems>("feed://new_items", (ev) => {
        notify(t("feed.newItems", { count: ev.payload.count }));
        void loadItems();
      }),
      await listen<FeedStatus>("feed://status", (ev) => {
        notify(t("feed.statusEvent", { message: ev.payload.message }));
      }),
    );
  });

  onDestroy(() => {
    unlistens.forEach((u) => u());
  });
</script>

<main class="container feed">
  <h1>{t("feed.title")}</h1>

  <section class="panel">
    <h2>{t("feed.subscribe.title")}</h2>
    <div class="row">
      <input
        type="text"
        bind:value={subInput}
        placeholder={t("feed.subscribe.placeholder")}
        onkeydown={(e) => e.key === "Enter" && subscribe()}
      />
      <select bind:value={subCategory} aria-label={t("feed.category.label")}>
        <option value={null}>{t("feed.category.none")}</option>
        {#each categories as c (c.id)}
          <option value={c.id}>{c.name}</option>
        {/each}
      </select>
      <button onclick={subscribe} disabled={busy || !subInput.trim()}>
        {t("feed.subscribe.button")}
      </button>
    </div>
    <div class="row">
      <input
        type="text"
        bind:value={newCatName}
        placeholder={t("feed.category.placeholder")}
        onkeydown={(e) => e.key === "Enter" && createCategory()}
      />
      <button class="link" onclick={createCategory} disabled={!newCatName.trim()}>
        {t("feed.category.add")}
      </button>
    </div>
  </section>

  {#if channels.length > 0}
    <section class="panel">
      <h2>{t("feed.channels.title")}</h2>
      <ul class="channel-list">
        {#each channels as ch (ch.channelId)}
          <li>
            <span class="ch-title" title={ch.channelId}>{ch.title}</span>
            <select
              value={ch.categoryId}
              onchange={(e) =>
                setCategory(
                  ch,
                  e.currentTarget.value === ""
                    ? null
                    : Number(e.currentTarget.value),
                )}
              aria-label={t("feed.category.label")}
            >
              <option value="">{t("feed.category.none")}</option>
              {#each categories as c (c.id)}
                <option value={c.id}>{c.name}</option>
              {/each}
            </select>
            <button class="link danger" onclick={() => unsubscribe(ch)}>
              {t("feed.unsubscribe")}
            </button>
          </li>
        {/each}
      </ul>
    </section>
  {/if}

  <section class="panel">
    <div class="row feed-head">
      <h2>{t("feed.items.title")}</h2>
      <label class="filter">
        <input
          type="checkbox"
          bind:checked={unreadOnly}
          onchange={() => void loadItems()}
        />
        {t("feed.filters.unreadOnly")}
      </label>
      <select
        bind:value={filterCat}
        onchange={() => void loadItems()}
        aria-label={t("feed.filters.category")}
      >
        <option value={null}>{t("feed.filters.all")}</option>
        <option value={0}>{t("feed.category.none")}</option>
        {#each categories as c (c.id)}
          <option value={c.id}>{c.name}</option>
        {/each}
      </select>
      <button class="link" onclick={refreshNow}>{t("feed.refresh")}</button>
      <button class="link" onclick={markAllRead}>{t("feed.items.markAllRead")}</button>
    </div>

    {#if items.length === 0}
      <p class="subtle">{t("feed.items.empty")}</p>
    {:else}
      <ul class="item-list">
        {#each items as it (it.videoId)}
          <li class:read={it.isRead}>
            {#if it.thumbnailUrl}
              <img class="thumb" src={it.thumbnailUrl} alt="" loading="lazy" />
            {/if}
            <div class="item-main">
              <button class="item-title" onclick={() => playItem(it)}>
                {it.title}
              </button>
              <div class="subtle meta">
                {it.channelTitle ?? it.channelId}
                {formatPublished(it.publishedAt)}
              </div>
            </div>
            {#if !it.isRead}
              <button class="link" onclick={() => markRead(it.videoId)}>
                {t("feed.items.markRead")}
              </button>
            {/if}
          </li>
        {/each}
      </ul>
    {/if}
  </section>

  {#each notices as n (n)}
    <div class="notice">{n}</div>
  {/each}
</main>

<style>
  .feed {
    max-width: 860px;
  }
  .panel {
    margin-top: 16px;
    padding: 16px;
    border: 1px solid #3c4043;
    border-radius: 12px;
  }
  .panel h2 {
    margin: 0 0 8px;
    font-size: 1rem;
  }
  .row {
    display: flex;
    gap: 8px;
    align-items: center;
    margin: 8px 0;
    flex-wrap: wrap;
  }
  .row input[type="text"] {
    flex: 1;
    min-width: 200px;
  }
  .channel-list,
  .item-list {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .channel-list li {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 6px 0;
    border-top: 1px solid #2d2f33;
  }
  .ch-title {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .feed-head {
    justify-content: flex-start;
  }
  .feed-head h2 {
    margin-right: auto;
  }
  .filter {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 0.9rem;
  }
  .item-list li {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 10px 0;
    border-top: 1px solid #2d2f33;
  }
  .item-list li.read {
    opacity: 0.55;
  }
  .thumb {
    width: 96px;
    border-radius: 6px;
    flex-shrink: 0;
  }
  .item-main {
    flex: 1;
    min-width: 0;
  }
  .item-title {
    display: block;
    padding: 0;
    border: none;
    background: none;
    text-align: left;
    font-size: 0.95rem;
    color: #e8e8e8;
    cursor: pointer;
  }
  .item-title:hover {
    text-decoration: underline;
  }
  .meta {
    font-size: 0.8rem;
    display: flex;
    gap: 12px;
  }
</style>
