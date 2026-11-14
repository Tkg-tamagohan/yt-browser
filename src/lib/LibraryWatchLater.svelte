<script lang="ts">
  // 「あとで見る」タブ（FR-59、仕様決定 CJ）。データと操作はページ側の
  // ファクトリが持ち、このコンポーネントは表示だけを受け持つ
  // （タブ切替で再取得しない現行挙動を維持するため、ロードはここでは
  // 行わない）。va（VideoActions の状態）は他タブと共有の 1 インスタンス
  import { t } from "$lib/i18n";
  import { fmtDateTime } from "$lib/format";
  import VideoActions from "$lib/VideoActions.svelte";
  import VideoRow from "$lib/VideoRow.svelte";
  import {
    createVideoActionState,
    videoRefOf,
  } from "$lib/video-actions.svelte";
  import type { WatchLaterEntry } from "$lib/players.svelte";

  type Props = {
    watchLater: WatchLaterEntry[];
    play: (videoId: string, resume: boolean) => Promise<void>;
    removeWatchLater: (videoId: string) => Promise<void>;
    /// ファクトリ内の単一インスタンス（playlists がプレイリスト一覧の本体）
    va: ReturnType<typeof createVideoActionState>;
    /// VideoActions からのプレイリスト追加通知（FR-7）
    onPlaylistAdd: (playlistId: number) => Promise<void>;
  };
  let { watchLater, play, removeWatchLater, va, onPlaylistAdd }: Props =
    $props();
</script>

    <!-- 空表示と一覧の出し分けはリマウントを避けるため hidden 切替
         （ヒットずれ対策、FR-80） -->
    <p class="subtle" class:hidden={watchLater.length > 0}>
      {t("library.watchLater.empty")}
    </p>
      <ul class="rows" class:hidden={watchLater.length === 0}>
        {#each watchLater as w (w.videoId)}
          <VideoRow
            videoId={w.videoId}
            title={w.title}
            thumbnailUrl={w.thumbnailUrl}
            onplay={() => play(w.videoId, true)}
          >
            {#snippet sub()}
              {#if w.channelTitle}{w.channelTitle}{/if}
              {#if w.addedAt}・{fmtDateTime(w.addedAt)}{/if}
            {/snippet}
            {#snippet actions()}
              <button onclick={() => play(w.videoId, true)}
                >{t("library.play")}</button
              >
              <button
                class="link danger"
                onclick={() => removeWatchLater(w.videoId)}
                >{t("library.watchLater.remove")}</button
              >
              <VideoActions
                video={videoRefOf(w)}
                faved={va.favIds.has(w.videoId)}
                playlists={va.playlists}
                watchLater={true}
                onfavchange={va.onFavChange}
                onplaylistcreated={va.onPlaylistCreated}
                onplaylistadd={onPlaylistAdd}
                onwatchlaterchange={va.onWatchLaterChange}
              />
            {/snippet}
          </VideoRow>
        {/each}
      </ul>
