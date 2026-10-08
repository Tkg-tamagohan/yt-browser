<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { goto } from "$app/navigation";
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { t } from "$lib/i18n";
  import { fmtDateTime } from "$lib/format";
  import { notify } from "$lib/notices.svelte";
  import VideoActions from "$lib/VideoActions.svelte";
  import VideoRow from "$lib/VideoRow.svelte";
  import {
    createVideoActionState,
    videoRefOf,
  } from "$lib/video-actions.svelte";
  import { asErrorMessage } from "$lib/players.svelte";

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

  let items = $state<FeedItem[]>([]);
  let channels = $state<Channel[]>([]);
  let categories = $state<Category[]>([]);
  let subInput = $state("");
  let subCategory = $state<number | null>(null);
  let newCatName = $state("");
  let unreadOnly = $state(true);
  // null=すべて / 0=未分類 / n=カテゴリ id
  let filterCat = $state<number | null>(null);
  // 種別フィルタ（""=すべて）。"video" は未検出項目を含む（仕様決定 V）
  let filterKind = $state("");
  // 登録チャンネル一覧の表示用カテゴリ絞込（画面ローカル、FR-13）
  let chanCatFilter = $state<number | null>(null);
  let busy = $state(false);
  let unlistens: UnlistenFn[] = [];

  // お気に入り・プレイリスト行アクション用（FR-7）
  const va = createVideoActionState();

  // 登録チャンネル一覧のカテゴリ絞込（DB ではなく表示のみ）
  let visibleChannels = $derived(
    chanCatFilter === null
      ? channels
      : channels.filter((c) =>
          chanCatFilter === 0
            ? c.categoryId === null
            : c.categoryId === chanCatFilter,
        ),
  );

  // フィルタ切替を重ねたとき古い応答が後着で上書きしないよう、
  // 最後に開始した呼び出しの結果だけを反映する
  let loadSeq = 0;

  async function loadItems(): Promise<void> {
    const seq = ++loadSeq;
    const res = await invoke<FeedItem[]>("list_feed", {
      filter: {
        unreadOnly,
        categoryId: filterCat,
        days: null,
        kind: filterKind || null,
      },
    });
    if (seq === loadSeq) items = res;
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

  async function blockItem(it: FeedItem): Promise<void> {
    try {
      await invoke("block_channel", {
        channelId: it.channelId,
        title: it.channelTitle ?? it.channelId,
      });
      // ブロックしたチャンネルの項目は一覧から消える
      items = items.filter((x) => x.channelId !== it.channelId);
      notify(
        t("blocked.added", { title: it.channelTitle ?? it.channelId }),
      );
    } catch (e) {
      notify(t("blocked.addFailed", { message: asErrorMessage(e) }));
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
    // 再生が始まった項目のみ既読にする（play_video が失敗したら未読のまま残す）
    try {
      await invoke("play_video", { videoId: it.videoId, resume: false });
    } catch (e) {
      notify(t("feed.failed", { message: asErrorMessage(e) }));
      return;
    }
    // 既読化の失敗は再生を止めない（通知に留めて遷移は行う）
    try {
      await invoke("mark_read", { videoIds: [it.videoId] });
    } catch (e) {
      notify(t("feed.readMarkFailed", { message: asErrorMessage(e) }));
    }
    goto("/");
  }

  async function refreshNow(): Promise<void> {
    try {
      await invoke("feed_refresh", {});
      notify(t("feed.refreshQueued"));
    } catch (e) {
      notify(t("feed.failed", { message: asErrorMessage(e) }));
    }
  }

  onMount(async () => {
    await refreshAll();
    void va.refresh();
    unlistens.push(
      await listen<FeedNewItems>("feed://new_items", (ev) => {
        notify(t("feed.newItems", { count: ev.payload.count }));
        void loadItems();
      }),
      await listen<FeedStatus>("feed://status", (ev) => {
        notify(t("feed.statusEvent", { message: ev.payload.message }));
      }),
      // shorts の非同期判定で kind が確定した（トーストなしで静かに再取得）
      await listen<FeedNewItems>("feed://kind_updated", () => {
        void loadItems();
      }),
    );
  });

  onDestroy(() => {
    unlistens.forEach((u) => u());
  });
</script>

<main class="container feed">
  <h1>{t("feed.title")}</h1>

  <div class="feed-grid">
    <div class="feed-col">
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
          <div class="row feed-head">
            <h2>{t("feed.channels.title")}</h2>
            <select
              bind:value={chanCatFilter}
              aria-label={t("feed.channels.filter")}
              title={t("feed.channels.filter")}
            >
              <option value={null}>{t("feed.filters.all")}</option>
              <option value={0}>{t("feed.category.none")}</option>
              {#each categories as c (c.id)}
                <option value={c.id}>{c.name}</option>
              {/each}
            </select>
          </div>
          {#if visibleChannels.length === 0}
            <p class="subtle">{t("feed.channels.empty")}</p>
          {:else}
            <ul class="channel-list">
              {#each visibleChannels as ch (ch.channelId)}
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
          {/if}
        </section>
      {/if}
    </div>

    <div class="feed-col">
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
          <select
            bind:value={filterKind}
            onchange={() => void loadItems()}
            aria-label={t("feed.filters.kind")}
            title={t("feed.filters.kind")}
          >
            <option value="">{t("feed.filters.all")}</option>
            <option value="video">{t("feed.filters.kindVideo")}</option>
            <option value="short">{t("feed.filters.kindShort")}</option>
            <option value="live">{t("feed.filters.kindLive")}</option>
          </select>
          <button class="link" onclick={refreshNow}>{t("feed.refresh")}</button>
          <button class="link" onclick={markAllRead}>{t("feed.items.markAllRead")}</button>
        </div>

    {#if items.length === 0}
      <p class="subtle">{t("feed.items.empty")}</p>
    {:else}
      <ul class="item-list">
        {#each items as it (it.videoId)}
          <VideoRow
            videoId={it.videoId}
            title={it.title}
            thumbnailUrl={it.thumbnailUrl}
            thumbSize="sm"
            dimmed={it.isRead}
            actionsPlacement="side"
            onplay={() => playItem(it)}
          >
            {#snippet sub()}
              <span class="feed-meta">
                <span>{it.channelTitle ?? it.channelId}</span>
                <span>{fmtDateTime(it.publishedAt)}</span>
              </span>
            {/snippet}
            {#snippet actions()}
              {#if !it.isRead}
                <button class="link" onclick={() => markRead(it.videoId)}>
                  {t("feed.items.markRead")}
                </button>
              {/if}
              <button
                class="link danger"
                title={t("feed.item.block")}
                onclick={() => blockItem(it)}
              >
                {t("search.block")}
              </button>
              <VideoActions
                video={videoRefOf(it)}
                faved={va.favIds.has(it.videoId)}
                playlists={va.playlists}
                onfavchange={va.onFavChange}
                onplaylistcreated={va.onPlaylistCreated}
              />
            {/snippet}
          </VideoRow>
        {/each}
      </ul>
    {/if}
      </section>
    </div>
  </div>
</main>

<style>
  .feed {
    max-width: 1100px;
  }
  /* FR-13: 左=購読管理、右=フィード一覧。狭い画面では縦積み。
     .container の既定 flex 縦並びは維持するため grid を .feed-grid に掛ける */
  .feed-grid {
    display: grid;
    grid-template-columns: minmax(280px, 340px) 1fr;
    gap: 0 16px;
    align-items: start;
  }
  @media (max-width: 760px) {
    .feed-grid {
      grid-template-columns: 1fr;
    }
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
  .feed-meta {
    display: flex;
    gap: 12px;
  }
</style>
