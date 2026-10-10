<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { goto } from "$app/navigation";
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { t } from "$lib/i18n";
  import { fmtDateTime } from "$lib/format";
  import { feedItemCompare, feedItemSortsAfter } from "$lib/feed-page-logic";
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
  // ページングカーソル（FR-25、仕様決定 AR）。前ページの末尾行を指す
  interface FeedCursor {
    publishedAt: string | null;
    videoId: string;
  }
  interface BackfillOutcome {
    inserted: number;
    videosPos: number;
    streamsPos: number;
    errors: string[];
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
  // フィード一覧の束ね方。true のときチャンネル単位のセクションに分ける
  let groupByChannel = $state(false);
  // チャンネル別フィード（FR-21、仕様決定 AN）。1 件のチャンネルに絞る。
  // チャンネル名クリックで入り、× で戻る
  let filterChannel = $state<Channel | null>(null);
  let backfilling = $state(false);
  let busy = $state(false);
  // 「さらに読み込む」の状態（1 ページ = バックエンドの FEED_LIST_LIMIT）
  let feedCursor = $state<FeedCursor | null>(null);
  let feedHasMore = $state(false);
  // 再取得で保持した過去ページの手前に空白が残りうるときの
  // 状態。gapBoundary は保持分の先頭（空白補完の到達目標）、
  // deepCursor/deepHasMore は空白を埋め切った後に復帰する深い側の
  // カーソル（保持前の feedCursor/feedHasMore）
  let gapBoundary: FeedCursor | null = null;
  let deepCursor: FeedCursor | null = null;
  let deepHasMore = false;
  let loadingMore = $state(false);
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

  // チャンネル別表示用のグルーピング。初出順を維持し、
  // 各グループ内は一覧側の並び（新着順）そのままにする
  let groupedItems = $derived.by(() => {
    const map = new Map<string, { title: string; entries: FeedItem[] }>();
    for (const it of items) {
      const g = map.get(it.channelId);
      if (g) {
        g.entries.push(it);
      } else {
        map.set(it.channelId, {
          title: it.channelTitle ?? it.channelId,
          entries: [it],
        });
      }
    }
    return [...map.entries()].map(([channelId, g]) => ({
      channelId,
      ...g,
    }));
  });

  // フィルタ切替を重ねたとき古い応答が後着で上書きしないよう、
  // 最後に開始した呼び出しの結果だけを反映する
  let loadSeq = 0;
  // 表示中の一覧が属するフィルタ条件。更新イベントでの再取得で
  // 過去ページを保持するのは同一条件での再取得に限る
  // （フィルタ変更直後は旧条件で追記した分を残してはいけない）
  let itemsFilterKey = "";

  /// バックエンドのカーソル比較と同じ全順序で、項目がカーソル
  /// （publishedAt DESC NULLS LAST、video_id ASC）より後にあるかは
  /// feedItemSortsAfter で判定する（lib/feed-page-logic.ts、FR-25）

  async function loadItems(): Promise<void> {
    const seq = ++loadSeq;
    const filterKey = `${unreadOnly ? 1 : 0}|${filterCat ?? ""}|${filterKind}|${filterChannel?.channelId ?? ""}`;
    const res = await invoke<FeedItem[]>("list_feed", {
      filter: {
        unreadOnly,
        categoryId: filterCat,
        days: null,
        kind: filterKind || null,
        cursor: null,
        channelId: filterChannel?.channelId ?? null,
      },
    });
    if (seq === loadSeq) {
      const last = res[res.length - 1];
      // 同一フィルタでの再取得（更新通知など）かつ先頭ページが満杯の
      // ときだけ、既に読み込んだ過去ページ（新しい先頭ページの末尾より
      // 後に続く部分）を保持する。部分ページ（末尾まで取れた）では
      // 保持分は membership の切れた古い行になるため捨てる。
      // 保持分と先頭ページの間に入る新着の空白は loadMoreItems が
      // 整列追記で埋めるため、カーソルは常に先頭ページの末尾に戻す
      const tail =
        last && res.length >= 500 && filterKey === itemsFilterKey
          ? items.filter((i) =>
              feedItemSortsAfter(i, last.publishedAt, last.videoId),
            )
          : [];
      if (tail.length > 0) {
        // 空白補完が未完の前回状態があれば深い側はそのまま引き継ぐ
        // （gap 走査の途中で再度の再取得が来ても最深カーソルを失わない）
        if (!gapBoundary) {
          deepCursor = feedCursor;
          deepHasMore = feedHasMore;
        }
        const head = tail[0];
        gapBoundary = { publishedAt: head.publishedAt, videoId: head.videoId };
      } else {
        gapBoundary = null;
        deepCursor = null;
        deepHasMore = false;
      }
      const seen = new Set(res.map((i) => i.videoId));
      items = [...res, ...tail.filter((i) => !seen.has(i.videoId))];
      itemsFilterKey = filterKey;
      feedCursor = last
        ? { publishedAt: last.publishedAt, videoId: last.videoId }
        : null;
      feedHasMore = res.length >= 500;
    }
  }

  // 末尾の次ページを追記する。カーソルは末尾行の
  // （published_at, video_id）。フィルタ変更や既読化で並びが
  // 変わっても、バックエンドのカーソル比較が同じ全順序なので
  // 重複せず続きを取れる（新規追加分は念のため videoId で重複除去）
  async function loadMoreItems(): Promise<void> {
    const cursor = feedCursor;
    if (!cursor || loadingMore) return;
    loadingMore = true;
    // 世代は進めず、進行中の初期取得と同じ世代に属させる。
    // フィルタ切替（loadItems）が始まればこの追記は自動で失効し、
    // 古い条件の続きが新しい一覧へ混入しない
    const seq = loadSeq;
    try {
      const res = await invoke<FeedItem[]>("list_feed", {
        filter: {
          unreadOnly,
          categoryId: filterCat,
          days: null,
          kind: filterKind || null,
          cursor,
          channelId: filterChannel?.channelId ?? null,
        },
      });
      if (seq === loadSeq) {
        const seen = new Set(items.map((i) => i.videoId));
        // 再取得で保持した過去ページの手前に属する項目（先頭ページとの
        // 間に入った新着の空白）を正しい位置へ挿すため全体を整列する。
        // 通常は既に整列済みなので実質の移動は起きない
        items = [...items, ...res.filter((i) => !seen.has(i.videoId))].sort(
          feedItemCompare,
        );
        const last = res[res.length - 1];
        if (
          last &&
          gapBoundary &&
          feedItemCompare(last, gapBoundary) >= 0
        ) {
          // 空白を埋め切った（取得末尾が保持分の先頭に到達・追い越し）。
          // 保持分の区間は全て表示済みなので、保持前の深い側の
          // カーソルへ復帰し、既表示ページの再走査を省く
          feedCursor = deepCursor ?? {
            publishedAt: last.publishedAt,
            videoId: last.videoId,
          };
          feedHasMore = deepHasMore;
          gapBoundary = null;
          deepCursor = null;
          deepHasMore = false;
        } else {
          if (last) {
            feedCursor = {
              publishedAt: last.publishedAt,
              videoId: last.videoId,
            };
          }
          feedHasMore = res.length >= 500;
          if (!feedHasMore) {
            // 走査が末尾へ達した。保持分との間の空白はここまでで
            // 打ち止め（取得順の集合が尽きた）ため状態を畳む
            gapBoundary = null;
            deepCursor = null;
            deepHasMore = false;
          }
        }
      }
    } catch (e) {
      notify(t("feed.failed", { message: asErrorMessage(e) }));
    } finally {
      loadingMore = false;
    }
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
      // 絞り込み中のチャンネルを解除したら絞り込みも戻す
      if (filterChannel?.channelId === ch.channelId) filterChannel = null;
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

  // チャンネル別フィードへ切り替え（FR-21、仕様決定 AN）。
  // チャンネル名クリックで絞り込み＋カテゴリ解除＋「未読のみ」解除し、
  // そのチャンネルの RSS 即時取得を予約する（新着は feed://new_items 経由で届く）
  async function selectChannelFeed(ch: Channel): Promise<void> {
    filterChannel = ch;
    filterCat = null;
    unreadOnly = false;
    void loadItems();
    try {
      await invoke("feed_refresh", { channelId: ch.channelId });
    } catch (e) {
      notify(t("feed.failed", { message: asErrorMessage(e) }));
    }
  }

  function clearChannelFeed(): void {
    filterChannel = null;
    void loadItems();
  }

  // 手動バックフィル。1 回で /videos・/streams の各 100 件（暫定、仕様決定 AN）。
  // 取得済み位置は DB に残るため再実行で続きを遡る。バックフィル分は
  // 既読で投入されるので「未読のみ」が外れているこの画面で見える
  async function backfill(): Promise<void> {
    const ch = filterChannel;
    if (!ch || backfilling) return;
    backfilling = true;
    try {
      const res = await invoke<BackfillOutcome>("feed_backfill", {
        channelId: ch.channelId,
      });
      if (res.errors.length > 0) {
        notify(t("feed.backfillPartial", { message: res.errors.join(" / ") }));
      } else {
        notify(t("feed.backfilled", { count: res.inserted }));
      }
      await loadItems();
    } catch (e) {
      notify(t("feed.backfillFailed", { message: asErrorMessage(e) }));
    } finally {
      backfilling = false;
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
                  <button
                    class="link ch-title"
                    class:active={filterChannel?.channelId === ch.channelId}
                    title="{ch.channelId} — {t('feed.channels.showFeed')}"
                    onclick={() => selectChannelFeed(ch)}
                  >
                    {ch.title}
                  </button>
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
          <label class="filter">
            <input type="checkbox" bind:checked={groupByChannel} />
            {t("feed.filters.groupByChannel")}
          </label>
          <button class="link" onclick={refreshNow}>{t("feed.refresh")}</button>
          <button class="link" onclick={markAllRead}>{t("feed.items.markAllRead")}</button>
        </div>

        {#if filterChannel}
          <div class="row channel-filter">
            <span class="chip">
              {t("feed.channelFilter.label", { title: filterChannel.title })}
            </span>
            <button
              class="link"
              onclick={backfill}
              disabled={backfilling}
            >
              {backfilling ? t("feed.backfilling") : t("feed.backfill")}
            </button>
            <button
              class="link"
              onclick={clearChannelFeed}
              title={t("feed.channelFilter.clear")}
              aria-label={t("feed.channelFilter.clear")}
            >
              ×
            </button>
          </div>
        {/if}

    {#if items.length === 0}
      <p class="subtle">{t("feed.items.empty")}</p>
    {:else if groupByChannel}
      {#each groupedItems as group (group.channelId)}
        <section class="chan-group">
          <h3 class="chan-name">
            {group.title}
            <span class="chan-count">{group.entries.length}</span>
          </h3>
          <ul class="item-list">
            {#each group.entries as it (it.videoId)}
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
        </section>
      {/each}
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
    {#if feedHasMore}
      <button
        class="link load-more"
        onclick={loadMoreItems}
        disabled={loadingMore}
      >
        {loadingMore ? t("related.loading") : t("feed.items.loadMore")}
      </button>
    {/if}
      </section>
    </div>
  </div>
</main>

<style>
  /* 横幅は広いモニタの半分強まで使う（チャンネル名と動画タイトルが
     折り返されにくくするため 1100px から引き上げ） */
  .feed {
    max-width: 1600px;
  }
  /* FR-13: 左=購読管理、右=フィード一覧。狭い画面では縦積み。
     .container の既定 flex 縦並びは維持するため grid を .feed-grid に掛ける */
  .feed-grid {
    display: grid;
    grid-template-columns: minmax(280px, 360px) 1fr;
    gap: 0 16px;
    align-items: start;
  }
  @media (max-width: 760px) {
    .feed-grid {
      grid-template-columns: 1fr;
    }
  }
  /* 骨格はグローバル .panel。背景なし・各パネル先頭にも間隔を取る
     （.panel + .panel では列の先頭に効かないため） */
  .panel {
    margin-top: 16px;
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
    text-align: left;
  }
  .ch-title.active {
    font-weight: 600;
  }
  .channel-filter {
    margin: 4px 0 0;
  }
  .chip {
    padding: 2px 10px;
    border: 1px solid #3c4043;
    border-radius: 999px;
    font-size: 0.85rem;
    max-width: 320px;
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
  .chan-group {
    margin-top: 12px;
  }
  .chan-name {
    margin: 0 0 4px;
    font-size: 0.95rem;
    font-weight: 600;
  }
  .chan-count {
    color: #9aa0a6;
    font-weight: 400;
    font-size: 0.85rem;
    margin-left: 8px;
  }
</style>
