<script lang="ts">
  import {
    finishPlaylistAdd,
    type PlaylistAddCallbackSet,
  } from "$lib/context-menu-logic";
  // VideoActions のプレイリスト追加メニュー部品（FR-78、仕様決定 DC）。
  // 行アクションの「＋」ボタンが開くメニューと、動画行の右クリックで
  // 開くコンテキストメニュー（VideoContextMenu）で共用する。
  // 登録状態（playlists）と変更コールバックは親が持つ
  import { invoke } from "@tauri-apps/api/core";
  import { t } from "$lib/i18n";
  import { notify } from "$lib/notices.svelte";
  import {
    asErrorMessage,
    type Playlist,
    type VideoRef,
  } from "$lib/players.svelte";

  type Props = {
    video: VideoRef;
    playlists?: Playlist[];
    /// フローティング表示（コンテキストメニュー）。absolute 配置をやめ、
    /// 呼び出し側の配置コンテナへ追従する
    floating?: boolean;
    onplaylistcreated?: (pl: Playlist) => void;
    /// プレイリストへの追加が成功したとき（一覧表示側が件数・項目を読み直すため）
    onplaylistadd?: (playlistId: number) => void;
    /// 追加成功などメニューを畳むときに呼ぶ
    onclose?: () => void;
  };
  let {
    video,
    playlists = [],
    floating = false,
    onplaylistcreated,
    onplaylistadd,
    onclose,
  }: Props = $props();

  let newName = $state("");

  function captureContext() {
    return {
      targetVideo: video,
      callbacks: { onclose, onplaylistadd } satisfies PlaylistAddCallbackSet,
      notifyCreated: onplaylistcreated,
    };
  }

  async function addTo(
    pl: Playlist,
    captured = captureContext(),
  ): Promise<void> {
    // onclose?.() 実行時に親のコンテキスト状態が null 化され、リアクティブな props が
    // undefined になるのを防ぐため、await 前に参照を確保しておく
    try {
      await invoke("playlist_add", {
        playlistId: pl.id,
        video: captured.targetVideo,
      });
      notify(t("library.playlist.added", { name: pl.name }));
      finishPlaylistAdd(pl.id, captured.callbacks);
    } catch (e) {
      notify(t("library.playlist.addFailed", { message: asErrorMessage(e) }));
    }
  }

  async function createAndAdd(): Promise<void> {
    const name = newName.trim();
    if (!name) return;
    const captured = captureContext();
    try {
      const pl = await invoke<Playlist>("playlist_create", { name });
      captured.notifyCreated?.(pl);
      newName = "";
      await addTo(pl, captured);
    } catch (e) {
      notify(t("library.playlist.createFailed", { message: asErrorMessage(e) }));
    }
  }
</script>

<div class="vact-menu" class:floating>
  {#each playlists as pl (pl.id)}
    <button class="vact-item" onclick={() => void addTo(pl)}>
      {pl.name}（{pl.itemCount}）
    </button>
  {/each}
  <span class="vact-empty" class:hidden={playlists.length > 0}
    >{t("library.playlist.none")}</span
  >
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

<style>
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
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--bg-panel);
    box-shadow: 0 4px 16px var(--shadow);
  }

  /* コンテキストメニューでは呼び出し側の固定配置コンテナへ追従し、溢れた項目は内側でスクロールさせる */
  .vact-menu.floating {
    position: static;
    max-height: var(--vctx-max-h, calc(100vh - 8px));
    overflow-y: auto;
    box-sizing: border-box;
  }

  .vact-menu.floating > * {
    flex-shrink: 0;
  }

  .vact-item {
    padding: 6px 8px;
    border: none;
    border-radius: 6px;
    background: none;
    color: var(--fg);
    text-align: left;
    font-size: 0.85rem;
    cursor: pointer;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .vact-item:hover {
    background: var(--bg-subtle);
  }

  .vact-item:disabled {
    color: var(--fg-muted);
    cursor: default;
    background: none;
  }

  .vact-empty {
    padding: 6px 8px;
    color: var(--fg-muted);
    font-size: 0.85rem;
  }

  .vact-new {
    display: flex;
    gap: 4px;
    padding-top: 6px;
    border-top: 1px solid var(--border-row);
  }

  .vact-new input {
    flex: 1;
    min-width: 0;
    padding: 4px 6px;
    font-size: 0.85rem;
  }
</style>
