<script lang="ts">
  // 行アクションの共有部品（FR-7）: お気に入りトグルとプレイリスト追加メニュー。
  // 検索・関連・フィード・ライブラリの各行に載せる。
  // お気に入り状態とプレイリスト一覧は親が持ち、変更はコールバックで受け取る。
  import { invoke } from "@tauri-apps/api/core";
  import { t } from "$lib/i18n";
  import { notify } from "$lib/notices.svelte";
  import QueueButtons from "$lib/QueueButtons.svelte";
  import {
    asErrorMessage,
    type Playlist,
    type VideoRef,
  } from "$lib/players.svelte";

  type Props = {
    video: VideoRef;
    faved?: boolean;
    playlists?: Playlist[];
    onfavchange?: (videoId: string, faved: boolean) => void;
    onplaylistcreated?: (pl: Playlist) => void;
    /// プレイリストへの追加が成功したとき（一覧表示側が件数・項目を読み直すため）
    onplaylistadd?: (playlistId: number) => void;
  };
  let {
    video,
    faved = false,
    playlists = [],
    onfavchange,
    onplaylistcreated,
    onplaylistadd,
  }: Props = $props();

  let menuOpen = $state(false);
  let newName = $state("");

  async function toggleFav(): Promise<void> {
    try {
      if (faved) {
        await invoke("favorite_remove", { videoId: video.videoId });
        notify(t("library.favorite.removed"));
      } else {
        await invoke("favorite_add", { video });
        notify(t("library.favorite.added"));
      }
      onfavchange?.(video.videoId, !faved);
    } catch (e) {
      notify(t("library.favorite.failed", { message: asErrorMessage(e) }));
    }
  }

  async function addTo(pl: Playlist): Promise<void> {
    try {
      await invoke("playlist_add", { playlistId: pl.id, video });
      notify(t("library.playlist.added", { name: pl.name }));
      menuOpen = false;
      onplaylistadd?.(pl.id);
    } catch (e) {
      notify(t("library.playlist.addFailed", { message: asErrorMessage(e) }));
    }
  }

  async function createAndAdd(): Promise<void> {
    const name = newName.trim();
    if (!name) return;
    try {
      const pl = await invoke<Playlist>("playlist_create", { name });
      onplaylistcreated?.(pl);
      newName = "";
      await addTo(pl);
    } catch (e) {
      notify(t("library.playlist.createFailed", { message: asErrorMessage(e) }));
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
  <QueueButtons {video} />
  {#if menuOpen}
    <div class="vact-menu">
      {#each playlists as pl (pl.id)}
        <button class="vact-item" onclick={() => void addTo(pl)}>
          {pl.name}（{pl.itemCount}）
        </button>
      {/each}
      {#if playlists.length === 0}
        <span class="vact-empty">{t("library.playlist.none")}</span>
      {/if}
      <div class="vact-new">
        <input
          type="text"
          bind:value={newName}
          placeholder={t("library.playlist.newPlaceholder")}
          onkeydown={(e) => e.key === "Enter" && void createAndAdd()}
        />
        <button
          class="vact-item"
          onclick={() => void createAndAdd()}
          disabled={!newName.trim()}
        >
          {t("library.playlist.createAdd")}
        </button>
      </div>
    </div>
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

  .vact-menu {
    position: absolute;
    top: 100%;
    left: 0;
    z-index: 30;
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 220px;
    padding: 6px;
    border: 1px solid #3c4043;
    border-radius: 8px;
    background: #202124;
    box-shadow: 0 4px 16px rgb(0 0 0 / 50%);
  }

  .vact-item {
    padding: 6px 8px;
    border: none;
    border-radius: 6px;
    background: none;
    color: #e8e8e8;
    text-align: left;
    font-size: 0.85rem;
    cursor: pointer;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .vact-item:hover {
    background: #3c4043;
  }

  .vact-item:disabled {
    color: #9aa0a6;
    cursor: default;
    background: none;
  }

  .vact-empty {
    padding: 6px 8px;
    color: #9aa0a6;
    font-size: 0.85rem;
  }

  .vact-new {
    display: flex;
    gap: 4px;
    padding-top: 6px;
    border-top: 1px solid #2d2f33;
  }

  .vact-new input {
    flex: 1;
    min-width: 0;
    padding: 4px 6px;
    font-size: 0.85rem;
  }
</style>
