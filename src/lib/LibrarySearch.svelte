<script lang="ts">
  // 横断検索タブ（FR-60、仕様決定 CK）。履歴・お気に入り・プレイリスト
  // 項目・あとで見るをタイトルとチャンネル名の部分一致で一括検索する。
  // データと操作はページ側のファクトリが持ち、このコンポーネントは
  // 検索ボックスの入力と結果の描画だけを受け持つ。
  // 各行はヒットした区分（sources）をバッジで示す
  import { t } from "$lib/i18n";
  import VideoActions from "$lib/VideoActions.svelte";
  import VideoRow from "$lib/VideoRow.svelte";
  import {
    sortSources,
    sourceLabelKey,
  } from "$lib/library-search-logic";
  import {
    createVideoActionState,
    openVideoContextMenu,
    videoRefOf,
  } from "$lib/video-actions.svelte";
  import type { LibrarySearchItem } from "$lib/players.svelte";

  type Props = {
    /// 検索ボックスの現在値
    searchQuery: string;
    /// 検索ボックスの入力。入力のたびに取り直される
    onSearchInput: (value: string) => void;
    /// 現在の検索条件に対する結果（世代違いの応答は捨て済み）
    items: LibrarySearchItem[];
    play: (videoId: string, resume: boolean) => Promise<void>;
    /// ファクトリ内の単一インスタンス（playlists がプレイリスト一覧の本体）
    va: ReturnType<typeof createVideoActionState>;
    /// VideoActions からのプレイリスト追加通知（FR-7）
    onPlaylistAdd: (playlistId: number) => Promise<void>;
  };
  let { searchQuery, onSearchInput, items, play, va, onPlaylistAdd }: Props =
    $props();
</script>

    <div class="library-search">
      <input
        type="text"
        value={searchQuery}
        placeholder={t("library.search.placeholder")}
        oninput={(e) => onSearchInput(e.currentTarget.value)}
      />
    </div>
    {#if searchQuery.trim() === ""}
      <p class="subtle">{t("library.search.hint")}</p>
    {:else if items.length === 0}
      <p class="subtle">{t("library.search.empty")}</p>
    {:else}
      <ul class="rows">
        {#each items as it (it.videoId)}
          <VideoRow
            videoId={it.videoId}
            title={it.title || it.videoId}
            thumbnailUrl={it.thumbnailUrl}
            onplay={() => play(it.videoId, true)}
            oncontextmenu={(e) =>
              openVideoContextMenu(
                e,
                videoRefOf({ ...it, title: it.title || it.videoId }),
                va,
                onPlaylistAdd,
              )}
          >
            {#snippet sub()}
              {#each sortSources(it.sources) as src (src)}
                <span class="src">{t(sourceLabelKey(src))}</span>
              {/each}
              {#if it.channelTitle}{it.channelTitle}{/if}
            {/snippet}
            {#snippet actions()}
              <button onclick={() => play(it.videoId, true)}
                >{t("library.play")}</button
              >
              <VideoActions
                video={videoRefOf({ ...it, title: it.title || it.videoId })}
                faved={va.favIds.has(it.videoId)}
                playlists={va.playlists}
                watchLater={va.watchLaterIds.has(it.videoId)}
                onfavchange={va.onFavChange}
                onplaylistcreated={va.onPlaylistCreated}
                onplaylistadd={onPlaylistAdd}
                onwatchlaterchange={va.onWatchLaterChange}
              />
            {/snippet}
          </VideoRow>
        {/each}
      </ul>
    {/if}

<style>
  .library-search {
    margin-bottom: 12px;
  }

  .library-search input {
    width: 100%;
    box-sizing: border-box;
  }

  .src {
    display: inline-block;
    margin-right: 4px;
    padding: 0 6px;
    border: 1px solid var(--border);
    border-radius: 8px;
    color: var(--accent);
    font-size: 0.75rem;
  }
</style>
