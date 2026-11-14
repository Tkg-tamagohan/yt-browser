<script lang="ts">
  // 履歴タブ（FR-7）。履歴データと操作はページ側のファクトリが持ち、
  // このコンポーネントは表示だけを受け持つ。タブ切替で再取得しない
  // 現行挙動を維持するため、ロードはここでは行わない。
  // va（VideoActions の状態）はプレイリストタブと共有の 1 インスタンス
  // 検索ボックス（FR-61、仕様決定 CL）と「さらに読み込む」（FR-52、
  // 仕様決定 CC）の状態もファクトリ側にあり、ここでは配線だけ行う
  import { t } from "$lib/i18n";
  import { fmtDateTime } from "$lib/format";
  import VideoActions from "$lib/VideoActions.svelte";
  import VideoRow from "$lib/VideoRow.svelte";
  import {
    createVideoActionState,
    videoRefOf,
  } from "$lib/video-actions.svelte";
  import type { WatchHistory } from "$lib/players.svelte";

  type Props = {
    history: WatchHistory[];
    /// 検索ボックスの現在値（FR-61、仕様決定 CL）
    searchQuery: string;
    /// 検索ボックスの入力。入力のたびに先頭ページが取り直される
    onSearchInput: (value: string) => void;
    /// 「さらに読み込む」の表示可否と実行（FR-52、仕様決定 CC）
    hasMore: boolean;
    loadingMore: boolean;
    loadMore: () => Promise<void>;
    play: (videoId: string, resume: boolean) => Promise<void>;
    removeHistory: (videoId: string) => Promise<void>;
    /// ファクトリ内の単一インスタンス（playlists がプレイリスト一覧の本体）
    va: ReturnType<typeof createVideoActionState>;
    /// VideoActions からのプレイリスト追加通知（FR-7）
    onPlaylistAdd: (playlistId: number) => Promise<void>;
  };
  let {
    history,
    searchQuery,
    onSearchInput,
    hasMore,
    loadingMore,
    loadMore,
    play,
    removeHistory,
    va,
    onPlaylistAdd,
  }: Props = $props();

  function progressOf(h: WatchHistory): string {
    if (h.completed) return t("library.history.completed");
    if (h.durationSec && h.durationSec > 0) {
      const pct = Math.min(
        100,
        Math.round((h.positionSec / h.durationSec) * 100),
      );
      return t("library.history.progress", { percent: pct });
    }
    return "";
  }
</script>

    <div class="history-search">
      <input
        type="text"
        value={searchQuery}
        placeholder={t("library.history.searchPlaceholder")}
        oninput={(e) => onSearchInput(e.currentTarget.value)}
      />
    </div>
    {#if history.length === 0}
      <p class="subtle">
        {searchQuery.trim()
          ? t("library.history.searchEmpty")
          : t("library.history.empty")}
      </p>
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
              <VideoActions
                video={videoRefOf({ ...h, title: h.title || h.videoId })}
                faved={va.favIds.has(h.videoId)}
                playlists={va.playlists}
                watchLater={va.watchLaterIds.has(h.videoId)}
                onfavchange={va.onFavChange}
                onplaylistcreated={va.onPlaylistCreated}
                onplaylistadd={onPlaylistAdd}
                onwatchlaterchange={va.onWatchLaterChange}
              />
            {/snippet}
          </VideoRow>
        {/each}
      </ul>
      {#if hasMore}
        <button
          class="link load-more"
          onclick={() => void loadMore()}
          disabled={loadingMore}
        >
          {loadingMore
            ? t("related.loading")
            : t("library.history.loadMore")}
        </button>
      {/if}
    {/if}

<style>
  .history-search {
    margin-bottom: 12px;
  }

  .history-search input {
    width: 100%;
    box-sizing: border-box;
  }
</style>
