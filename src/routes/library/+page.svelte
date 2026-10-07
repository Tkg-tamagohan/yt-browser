<script lang="ts">
  // ローカルデータ画面（FR-7）: 視聴履歴・お気に入り・プレイリストの
  // 一覧・編集・削除をここで完結させる。
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";
  import { invoke } from "@tauri-apps/api/core";
  import { t } from "$lib/i18n";
  import { notify } from "$lib/notices.svelte";
  import { loadLibrary } from "$lib/library";
  import VideoActions from "$lib/VideoActions.svelte";
  import type {
    FavoriteEntry,
    Playlist,
    PlaylistEntry,
    UiError,
    VideoRef,
    WatchHistory,
  } from "$lib/players.svelte";

  type Tab = "history" | "favorites" | "playlists";
  let tab = $state<Tab>("history");

  let history = $state<WatchHistory[]>([]);
  let favorites = $state<FavoriteEntry[]>([]);
  let playlists = $state<Playlist[]>([]);
  let favIds = $state<Set<string>>(new Set());

  // 選択中プレイリストとその中身。reqId は選択ごとに進み、
  // 遅れて返る旧リクエストが表示を上書きしないようにする世代番号
  let selectedId = $state<number | null>(null);
  let playlistItems = $state<PlaylistEntry[]>([]);
  // 選択中プレイリストの項目取得（itemsReq）とプレイリスト一覧の取得
  // （listsReq）は別の世代で管理する。片方の世代を進めても、無関係な
  // 取得中の応答を無効化しない
  let itemsReq = 0;
  let listsReq = 0;
  let newPlaylistName = $state("");
  let renamingId = $state<number | null>(null);
  let renameText = $state("");

  function err(e: unknown): string {
    if (typeof e === "object" && e !== null && "message" in e) {
      return String((e as UiError).message);
    }
    return String(e);
  }

  function thumbOf(videoId: string, url: string | null): string {
    return url ?? `https://i.ytimg.com/vi/${videoId}/mqdefault.jpg`;
  }

  function fmtAt(iso: string | null | undefined): string {
    if (!iso) return "";
    const d = new Date(iso.includes("T") ? iso : iso.replace(" ", "T") + "Z");
    return Number.isNaN(d.getTime()) ? iso : d.toLocaleString("ja-JP");
  }

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

  async function loadHistory(): Promise<void> {
    history = await invoke<WatchHistory[]>("history_list", { limit: 500 });
  }

  async function loadFavorites(): Promise<void> {
    const lib = await loadLibrary();
    favIds = lib.favIds;
    playlists = lib.playlists;
    favorites = await invoke<FavoriteEntry[]>("favorite_list");
  }

  onMount(async () => {
    try {
      await Promise.all([loadHistory(), loadFavorites()]);
    } catch (e) {
      notify(t("library.failed", { message: err(e) }));
    }
  });

  async function play(videoId: string, resume: boolean): Promise<void> {
    try {
      await invoke("play_video", { videoId, resume });
      goto("/");
    } catch (e) {
      notify(t("player.error", { message: err(e) }));
    }
  }

  async function removeHistory(videoId: string): Promise<void> {
    try {
      await invoke("history_remove", { videoId });
      history = history.filter((h) => h.videoId !== videoId);
      notify(t("library.removed"));
    } catch (e) {
      notify(t("library.removeFailed", { message: err(e) }));
    }
  }

  function onFavChange(videoId: string, faved: boolean): void {
    const next = new Set(favIds);
    if (faved) next.add(videoId);
    else next.delete(videoId);
    favIds = next;
    if (!faved) {
      favorites = favorites.filter((f) => f.videoId !== videoId);
    }
  }

  async function removeFavorite(videoId: string): Promise<void> {
    try {
      await invoke("favorite_remove", { videoId });
      onFavChange(videoId, false);
      notify(t("library.favorite.removed"));
    } catch (e) {
      notify(t("library.favorite.failed", { message: err(e) }));
    }
  }

  function onPlaylistCreated(pl: Playlist): void {
    ++listsReq;
    playlists = [...playlists, pl];
  }

  async function createPlaylist(): Promise<void> {
    const name = newPlaylistName.trim();
    if (!name) return;
    try {
      const pl = await invoke<Playlist>("playlist_create", { name });
      ++listsReq;
      playlists = [...playlists, pl];
      newPlaylistName = "";
      selectedId = pl.id;
      playlistItems = [];
      notify(t("library.playlist.created", { name: pl.name }));
    } catch (e) {
      notify(t("library.playlist.createFailed", { message: err(e) }));
    }
  }

  function beginRename(pl: Playlist): void {
    renamingId = pl.id;
    renameText = pl.name;
  }

  async function commitRename(pl: Playlist): Promise<void> {
    const name = renameText.trim();
    if (!name || name === pl.name) {
      renamingId = null;
      return;
    }
    try {
      await invoke("playlist_rename", { playlistId: pl.id, name });
      ++listsReq;
      playlists = playlists.map((p) =>
        p.id === pl.id ? { ...p, name } : p,
      );
      renamingId = null;
      notify(t("library.playlist.renamed"));
    } catch (e) {
      notify(t("library.failed", { message: err(e) }));
    }
  }

  async function deletePlaylist(pl: Playlist): Promise<void> {
    try {
      await invoke("playlist_delete", { playlistId: pl.id });
      ++listsReq;
      playlists = playlists.filter((p) => p.id !== pl.id);
      if (selectedId === pl.id) {
        selectedId = null;
        playlistItems = [];
      }
      notify(t("library.playlist.deleted"));
    } catch (e) {
      notify(t("library.playlist.deleteFailed", { message: err(e) }));
    }
  }

  async function selectPlaylist(pl: Playlist): Promise<void> {
    selectedId = pl.id;
    renamingId = null;
    playlistItems = [];
    const req = ++itemsReq;
    try {
      const items = await invoke<PlaylistEntry[]>("playlist_items", {
        playlistId: pl.id,
      });
      // 応答が返るまでに別のプレイリストに切り替わっていたら捨てる
      if (req !== itemsReq || selectedId !== pl.id) return;
      playlistItems = items;
    } catch (e) {
      if (req === itemsReq) notify(t("library.failed", { message: err(e) }));
    }
  }

  /// プレイリスト一覧の再取得（件数の最新化）。
  /// 取得中に他の更新が listsReq を進めたら、応答を捨てて取り直す。
  /// 失効したまま放置すると追加した動画の件数が古いまま残るため。
  async function refreshPlaylists(): Promise<void> {
    for (;;) {
      const req = ++listsReq;
      const list = await invoke<Playlist[]>("playlist_list");
      if (req === listsReq) {
        playlists = list;
        return;
      }
    }
  }

  /// VideoActions からのプレイリスト追加通知（FR-7）。
  /// 追加先が表示中なら項目一覧も読み直す（重複追加は冪等なので再取得で吸収）。
  /// itemsReq は選択中プレイリストの項目取得だけの世代なので、
  /// 追加先が表示中のときだけ進める（他プレイリストへの追加で
  /// 選択中の取得を無効化しない）。一覧側は独立した listsReq で管理。
  async function onPlaylistAdd(playlistId: number): Promise<void> {
    const target = selectedId === playlistId;
    const itemReq = target ? ++itemsReq : itemsReq;
    try {
      await refreshPlaylists();
      if (target) {
        const items = await invoke<PlaylistEntry[]>("playlist_items", {
          playlistId,
        });
        if (itemReq === itemsReq && selectedId === playlistId) {
          playlistItems = items;
        }
      }
    } catch (e) {
      notify(t("library.failed", { message: err(e) }));
    }
  }

  async function removeItem(entry: PlaylistEntry): Promise<void> {
    if (selectedId === null) return;
    try {
      await invoke("playlist_remove", {
        playlistId: selectedId,
        videoId: entry.videoId,
      });
      // 件数が変わるので飛行中の一覧応答は無効化する
      ++listsReq;
      playlistItems = playlistItems.filter(
        (i) => i.videoId !== entry.videoId,
      );
      playlists = playlists.map((p) =>
        p.id === selectedId ? { ...p, itemCount: p.itemCount - 1 } : p,
      );
      notify(t("library.removed"));
    } catch (e) {
      notify(t("library.removeFailed", { message: err(e) }));
    }
  }

  function videoRefOf(v: {
    videoId: string;
    title: string;
    channelId: string | null;
    channelTitle: string | null;
    thumbnailUrl: string | null;
  }): VideoRef {
    return {
      videoId: v.videoId,
      title: v.title,
      channelId: v.channelId,
      channelTitle: v.channelTitle,
      thumbnailUrl: v.thumbnailUrl,
    };
  }
</script>

<main class="container library">
  <h1>{t("library.title")}</h1>

  <nav class="tabs">
    {#each ["history", "favorites", "playlists"] as const as tb}
      <button
        class="tab"
        class:active={tab === tb}
        onclick={() => (tab = tb)}
      >
        {#if tb === "history"}{t("library.tab.history")}
        {:else if tb === "favorites"}{t("library.tab.favorites")}
        {:else}{t("library.tab.playlists")}{/if}
      </button>
    {/each}
  </nav>

  {#if tab === "history"}
    {#if history.length === 0}
      <p class="subtle">{t("library.history.empty")}</p>
    {:else}
      <ul class="rows">
        {#each history as h (h.videoId)}
          <li class="row-item">
            <img class="thumb" src={thumbOf(h.videoId, null)} alt="" loading="lazy" />
            <div class="meta">
              <div class="title">{h.title || h.videoId}</div>
              <div class="sub">
                {#if h.channelTitle}{h.channelTitle}{/if}
                {#if progressOf(h)}・{progressOf(h)}{/if}
                {#if h.lastWatchedAt}
                  ・{t("library.history.lastWatched", { at: fmtAt(h.lastWatchedAt) })}
                {/if}
              </div>
              <div class="actions">
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
              </div>
            </div>
          </li>
        {/each}
      </ul>
    {/if}
  {:else if tab === "favorites"}
    {#if favorites.length === 0}
      <p class="subtle">{t("library.favorite.empty")}</p>
    {:else}
      <ul class="rows">
        {#each favorites as f (f.videoId)}
          <li class="row-item">
            <img
              class="thumb"
              src={thumbOf(f.videoId, f.thumbnailUrl)}
              alt=""
              loading="lazy"
            />
            <div class="meta">
              <div class="title">{f.title}</div>
              <div class="sub">
                {#if f.channelTitle}{f.channelTitle}{/if}
                {#if f.addedAt}・{fmtAt(f.addedAt)}{/if}
              </div>
              <div class="actions">
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
                  {playlists}
                  onfavchange={onFavChange}
                  onplaylistcreated={onPlaylistCreated}
                  onplaylistadd={onPlaylistAdd}
                />
              </div>
            </div>
          </li>
        {/each}
      </ul>
    {/if}
  {:else}
    <div class="pl-grid">
      <section class="pl-list">
        <div class="pl-new">
          <input
            type="text"
            bind:value={newPlaylistName}
            placeholder={t("library.playlist.newPlaceholder")}
            onkeydown={(e) => e.key === "Enter" && void createPlaylist()}
          />
          <button onclick={() => void createPlaylist()} disabled={!newPlaylistName.trim()}>
            {t("library.playlist.create")}
          </button>
        </div>
        {#if playlists.length === 0}
          <p class="subtle">{t("library.playlists.empty")}</p>
        {:else}
          <ul class="pl-names">
            {#each playlists as pl (pl.id)}
              <li class:active={selectedId === pl.id}>
                {#if renamingId === pl.id}
                  <input
                    type="text"
                    bind:value={renameText}
                    onkeydown={(e) => {
                      if (e.key === "Enter") void commitRename(pl);
                      if (e.key === "Escape") renamingId = null;
                    }}
                  />
                  <button class="link" onclick={() => void commitRename(pl)}>OK</button>
                {:else}
                  <button class="pl-name" onclick={() => void selectPlaylist(pl)}>
                    {pl.name}
                    <span class="subtle">{t("library.playlist.items", { count: pl.itemCount })}</span>
                  </button>
                  <button
                    class="link"
                    title={t("library.playlist.rename")}
                    onclick={() => beginRename(pl)}>✎</button
                  >
                  <button
                    class="link danger"
                    title={t("library.playlist.delete")}
                    onclick={() => void deletePlaylist(pl)}>×</button
                  >
                {/if}
              </li>
            {/each}
          </ul>
        {/if}
      </section>
      <section class="pl-items">
        {#if selectedId === null}
          <p class="subtle">{t("library.playlist.selectHint")}</p>
        {:else if playlistItems.length === 0}
          <p class="subtle">{t("library.playlist.empty")}</p>
        {:else}
          <ul class="rows">
            {#each playlistItems as it (it.videoId)}
              <li class="row-item">
                <span class="pos">{it.position + 1}</span>
                <img
                  class="thumb"
                  src={thumbOf(it.videoId, it.thumbnailUrl)}
                  alt=""
                  loading="lazy"
                />
                <div class="meta">
                  <div class="title">{it.title}</div>
                  <div class="sub">{it.channelTitle ?? ""}</div>
                  <div class="actions">
                    <button onclick={() => play(it.videoId, true)}
                      >{t("library.play")}</button
                    >
                    <button
                      class="link danger"
                      onclick={() => removeItem(it)}
                      >{t("library.remove")}</button
                    >
                  </div>
                </div>
              </li>
            {/each}
          </ul>
        {/if}
      </section>
    </div>
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

  .rows {
    list-style: none;
    margin: 0;
    padding: 0;
  }

  .row-item {
    display: flex;
    gap: 12px;
    align-items: flex-start;
    padding: 10px 0;
    border-bottom: 1px solid #2d2f33;
  }

  .thumb {
    width: 120px;
    aspect-ratio: 16 / 9;
    object-fit: cover;
    border-radius: 6px;
    background: #26282c;
    flex-shrink: 0;
  }

  .pos {
    width: 24px;
    color: #9aa0a6;
    text-align: right;
    flex-shrink: 0;
  }

  .meta {
    flex: 1;
    min-width: 0;
  }

  .title {
    font-weight: 600;
    overflow-wrap: anywhere;
  }

  .sub {
    color: #9aa0a6;
    font-size: 0.85rem;
    margin: 4px 0 8px;
  }

  .actions {
    display: flex;
    gap: 10px;
    align-items: center;
    flex-wrap: wrap;
  }

  .danger {
    color: #ff7b72;
  }

  .pl-grid {
    display: grid;
    grid-template-columns: 260px 1fr;
    gap: 16px;
  }

  .pl-new {
    display: flex;
    gap: 6px;
    margin-bottom: 10px;
  }

  .pl-new input {
    flex: 1;
    min-width: 0;
  }

  .pl-names {
    list-style: none;
    margin: 0;
    padding: 0;
  }

  .pl-names li {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 4px 0;
    border-bottom: 1px solid #2d2f33;
  }

  .pl-names li.active .pl-name {
    color: #8ab4f8;
  }

  .pl-name {
    flex: 1;
    min-width: 0;
    padding: 6px 4px;
    border: none;
    background: none;
    color: #e8e8e8;
    text-align: left;
    cursor: pointer;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .pl-name .subtle {
    margin-left: 6px;
    font-size: 0.8rem;
  }

  .pl-items {
    min-width: 0;
  }

  @media (max-width: 700px) {
    .pl-grid {
      grid-template-columns: 1fr;
    }
  }
</style>
