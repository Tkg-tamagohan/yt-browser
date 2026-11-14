<script lang="ts">
  // ローカルデータ画面（FR-7・FR-59・FR-60）: 視聴履歴・お気に入り・
  // プレイリスト・あとで見るの一覧・編集・削除とライブラリ横断検索を
  // ここで完結させる。
  import { onDestroy, onMount } from "svelte";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { t } from "$lib/i18n";
  import { notify } from "$lib/notices.svelte";
  import { createLibraryPageState } from "$lib/library-page.svelte";
  import LibraryHistory from "$lib/LibraryHistory.svelte";
  import LibraryFavorites from "$lib/LibraryFavorites.svelte";
  import LibraryPlaylists from "$lib/LibraryPlaylists.svelte";
  import LibraryWatchLater from "$lib/LibraryWatchLater.svelte";
  import LibrarySearch from "$lib/LibrarySearch.svelte";
  import { asErrorMessage, type Playlist } from "$lib/players.svelte";

  type Tab = "history" | "favorites" | "playlists" | "watchLater" | "search";
  let tab = $state<Tab>("history");

  // 状態・取得・編集ロジックはファクトリ側（$lib/library-page.svelte.ts）
  // へ集約し、ここではタブ状態とイベント購読だけを管理する。
  // 5 タブの描画は $lib の子コンポーネントへ分割し、ファクトリの
  // インスタンス（lib）をそのまま渡す。データ所有はファクトリ側の
  // ままなので、タブ切替で履歴・お気に入りを再取得しない挙動は維持される
  const lib = createLibraryPageState({
    goToPlaylistsTab: () => {
      tab = "playlists";
    },
  });

  // 画面外からのプレイリスト変更通知（library://playlists_changed、
  // 設計書 §3.2）の購読解除関数。マウント中だけ listen する
  let unlistens: UnlistenFn[] = [];

  onMount(async () => {
    // 購読の失敗で初回ロードまで止めないよう、独立して試す
    try {
      unlistens.push(
        await listen<Playlist>("library://playlists_changed", (ev) => {
          void lib.onPlaylistsChanged(ev.payload);
        }),
      );
    } catch (e) {
      notify(t("library.failed", { message: asErrorMessage(e) }));
    }
    try {
      await Promise.all([
        lib.loadHistory(),
        lib.loadFavorites(),
        lib.loadWatchLater(),
      ]);
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
    {#each ["history", "favorites", "playlists", "watchLater", "search"] as const as tb}
      <button
        class="tab"
        class:active={tab === tb}
        onclick={() => (tab = tb)}
      >
        {#if tb === "history"}{t("library.tab.history")}
        {:else if tb === "favorites"}{t("library.tab.favorites")}
        {:else if tb === "playlists"}{t("library.tab.playlists")}
        {:else if tb === "watchLater"}{t("library.tab.watchLater")}
        {:else}{t("library.tab.search")}{/if}
      </button>
    {/each}
  </nav>

  {#if tab === "history"}
    <LibraryHistory
      history={lib.history}
      searchQuery={lib.historyQuery}
      onSearchInput={lib.setHistoryQuery}
      hasMore={lib.historyHasMore}
      loadingMore={lib.historyLoadingMore}
      loadMore={lib.loadMoreHistory}
      play={lib.play}
      removeHistory={lib.removeHistory}
      va={lib.va}
      onPlaylistAdd={lib.onPlaylistAdd}
    />
  {:else if tab === "favorites"}
    <LibraryFavorites
      favorites={lib.favorites}
      play={lib.play}
      removeFavorite={lib.removeFavorite}
      va={lib.va}
      onPlaylistAdd={lib.onPlaylistAdd}
    />
  {:else if tab === "playlists"}
    <LibraryPlaylists {lib} play={lib.play} />
  {:else if tab === "watchLater"}
    <LibraryWatchLater
      watchLater={lib.watchLater}
      play={lib.play}
      removeWatchLater={lib.removeWatchLater}
      va={lib.va}
      onPlaylistAdd={lib.onPlaylistAdd}
    />
  {:else}
    <LibrarySearch
      searchQuery={lib.searchQuery}
      onSearchInput={lib.setSearchQuery}
      items={lib.searchItems}
      play={lib.play}
      va={lib.va}
      onPlaylistAdd={lib.onPlaylistAdd}
    />
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
</style>
