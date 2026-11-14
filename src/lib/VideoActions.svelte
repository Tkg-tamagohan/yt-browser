<script lang="ts">
  // 行アクションの共有部品（FR-7・FR-59）: お気に入りトグル・プレイリスト
  // 追加メニュー・あとで見るトグル。
  // 検索・関連・フィード・ライブラリの各行に載せる。
  // 登録状態とプレイリスト一覧は親が持ち、変更はコールバックで受け取る。
  // プレイリスト追加メニューの本体は VideoActionsMenu に切り出し、
  // 動画行のコンテキストメニュー（FR-78）と共用する
  import { invoke } from "@tauri-apps/api/core";
  import { t } from "$lib/i18n";
  import LiveWaitButton from "$lib/LiveWaitButton.svelte";
  import { notify } from "$lib/notices.svelte";
  import QueueButtons from "$lib/QueueButtons.svelte";
  import VideoActionsMenu from "$lib/VideoActionsMenu.svelte";
  import {
    asErrorMessage,
    type FavoriteEntry,
    type Playlist,
    type VideoRef,
  } from "$lib/players.svelte";

  type Props = {
    video: VideoRef;
    faved?: boolean;
    playlists?: Playlist[];
    /// あとで見るに登録済みか（FR-59、仕様決定 CJ）
    watchLater?: boolean;
    /// 「配信を待機」行アクションを出すか（FR-44、仕様決定 BT(1)）。
    /// フィード・検索・関連動画の行で有効にする
    livewait?: boolean;
    onfavchange?: (videoId: string, faved: boolean) => void;
    onplaylistcreated?: (pl: Playlist) => void;
    /// プレイリストへの追加が成功したとき（一覧表示側が件数・項目を読み直すため）
    onplaylistadd?: (playlistId: number) => void;
    /// あとで見るの登録・解除が成功したとき（新タブの一覧更新に使う）
    onwatchlaterchange?: (videoId: string, watchLater: boolean) => void;
  };
  let {
    video,
    faved = false,
    playlists = [],
    watchLater = false,
    livewait = false,
    onfavchange,
    onplaylistcreated,
    onplaylistadd,
    onwatchlaterchange,
  }: Props = $props();

  let menuOpen = $state(false);

  async function toggleFav(): Promise<void> {
    try {
      if (faved) {
        // 削除した行のスナップショットが返る（FR-54、仕様決定 CE）。
        // トーストの「元に戻す」で削除前の行を復元する
        const removed = await invoke<FavoriteEntry | null>(
          "favorite_remove",
          { videoId: video.videoId },
        );
        if (removed === null) {
          notify(t("library.favorite.removed"));
        } else {
          notify(t("library.favorite.removed"), {
            label: t("library.undo"),
            run: () => {
              void (async () => {
                try {
                  await invoke("favorite_restore", { entry: removed });
                  onfavchange?.(video.videoId, true);
                  notify(t("library.restored"));
                } catch (e) {
                  notify(
                    t("library.restoreFailed", {
                      message: asErrorMessage(e),
                    }),
                  );
                }
              })();
            },
          });
        }
      } else {
        await invoke("favorite_add", { video });
        notify(t("library.favorite.added"));
      }
      onfavchange?.(video.videoId, !faved);
    } catch (e) {
      notify(t("library.favorite.failed", { message: asErrorMessage(e) }));
    }
  }

  async function toggleWatchLater(): Promise<void> {
    try {
      if (watchLater) {
        await invoke("watch_later_remove", { videoId: video.videoId });
        notify(t("library.watchLater.removed"));
      } else {
        await invoke("watch_later_add", { video });
        notify(t("library.watchLater.added"));
      }
      onwatchlaterchange?.(video.videoId, !watchLater);
    } catch (e) {
      notify(t("library.watchLater.failed", { message: asErrorMessage(e) }));
    }
  }
</script>

<span class="vact">
  <button
    class="icon"
    class:active={faved}
    title={faved ? t("library.favorite.remove") : t("library.favorite.add")}
    aria-label={faved ? t("library.favorite.remove") : t("library.favorite.add")}
    onclick={() => void toggleFav()}
  >
    {faved ? "★" : "☆"}
  </button>
  <button
    class="icon"
    title={t("library.playlist.addTo")}
    aria-label={t("library.playlist.addTo")}
    onclick={() => (menuOpen = !menuOpen)}
  >
    ＋
  </button>
  <button
    class="icon"
    class:active={watchLater}
    title={watchLater
      ? t("library.watchLater.remove")
      : t("library.watchLater.add")}
    aria-label={watchLater
      ? t("library.watchLater.remove")
      : t("library.watchLater.add")}
    onclick={() => void toggleWatchLater()}
  >
    後
  </button>
  <QueueButtons {video} />
  {#if livewait}
    <LiveWaitButton {video} />
  {/if}
  {#if menuOpen}
    <VideoActionsMenu
      {video}
      {playlists}
      {onplaylistcreated}
      {onplaylistadd}
      onclose={() => (menuOpen = false)}
    />
  {/if}
</span>

<style>
  .vact {
    position: relative;
    display: inline-flex;
    gap: 2px;
  }

  .icon {
    padding: 2px 6px;
    border: none;
    background: none;
    color: #9aa0a6;
    font-size: 1rem;
    line-height: 1;
    cursor: pointer;
  }

  .icon:hover,
  .icon.active {
    color: #fdd663;
  }
</style>
