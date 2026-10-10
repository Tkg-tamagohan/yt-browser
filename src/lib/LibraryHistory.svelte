<script lang="ts">
  // 履歴タブ（FR-7）。履歴データと操作はページ側のファクトリが持ち、
  // このコンポーネントは表示だけを受け持つ。タブ切替で再取得しない
  // 現行挙動を維持するため、ロードはここでは行わない。
  // va（VideoActions の状態）はプレイリストタブと共有の 1 インスタンス
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
    play: (videoId: string, resume: boolean) => Promise<void>;
    removeHistory: (videoId: string) => Promise<void>;
    /// ファクトリ内の単一インスタンス（playlists がプレイリスト一覧の本体）
    va: ReturnType<typeof createVideoActionState>;
    /// VideoActions からのプレイリスト追加通知（FR-7）
    onPlaylistAdd: (playlistId: number) => Promise<void>;
  };
  let { history, play, removeHistory, va, onPlaylistAdd }: Props = $props();

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

    {#if history.length === 0}
      <p class="subtle">{t("library.history.empty")}</p>
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
                onfavchange={va.onFavChange}
                onplaylistcreated={va.onPlaylistCreated}
                onplaylistadd={onPlaylistAdd}
              />
            {/snippet}
          </VideoRow>
        {/each}
      </ul>
    {/if}
