<script lang="ts">
  // お気に入りタブ（FR-7）。データと操作はページ側のファクトリが持ち、
  // このコンポーネントは表示だけを受け持つ（タブ切替で再取得しない
  // 現行挙動を維持するため、ロードはここでは行わない）。
  // va（VideoActions の状態）はプレイリストタブと共有の 1 インスタンス
  import { t } from "$lib/i18n";
  import { fmtDateTime } from "$lib/format";
  import VideoActions from "$lib/VideoActions.svelte";
  import VideoRow from "$lib/VideoRow.svelte";
  import {
    createVideoActionState,
    videoRefOf,
  } from "$lib/video-actions.svelte";
  import type { FavoriteEntry } from "$lib/players.svelte";

  type Props = {
    favorites: FavoriteEntry[];
    play: (videoId: string, resume: boolean) => Promise<void>;
    removeFavorite: (videoId: string) => Promise<void>;
    /// ファクトリ内の単一インスタンス（playlists がプレイリスト一覧の本体）
    va: ReturnType<typeof createVideoActionState>;
    /// VideoActions からのプレイリスト追加通知（FR-7）
    onPlaylistAdd: (playlistId: number) => Promise<void>;
  };
  let { favorites, play, removeFavorite, va, onPlaylistAdd }: Props =
    $props();
</script>

    {#if favorites.length === 0}
      <p class="subtle">{t("library.favorite.empty")}</p>
    {:else}
      <ul class="rows">
        {#each favorites as f (f.videoId)}
          <VideoRow
            videoId={f.videoId}
            title={f.title}
            thumbnailUrl={f.thumbnailUrl}
            onplay={() => play(f.videoId, true)}
          >
            {#snippet sub()}
              {#if f.channelTitle}{f.channelTitle}{/if}
              {#if f.addedAt}・{fmtDateTime(f.addedAt)}{/if}
            {/snippet}
            {#snippet actions()}
              <button onclick={() => play(f.videoId, true)}
                >{t("library.play")}</button
              >
              <button
                class="link danger"
                onclick={() => removeFavorite(f.videoId)}
                >{t("library.favorite.remove")}</button
              >
              <VideoActions
                video={videoRefOf(f)}
                faved={true}
                playlists={va.playlists}
                onfavchange={va.onFavChange}
                onplaylistcreated={va.onPlaylistCreated}
                onplaylistadd={onPlaylistAdd}
              />
            {/snippet}
          </VideoRow>
        {/each}
      </ul>
    {/if}
