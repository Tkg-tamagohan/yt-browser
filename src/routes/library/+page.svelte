<script lang="ts">
  // ローカルデータ画面（FR-7）: 視聴履歴・お気に入り・プレイリストの
  // 一覧・編集・削除をここで完結させる。
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";
  import { invoke } from "@tauri-apps/api/core";
  import { t } from "$lib/i18n";
  import { fmtDateTime } from "$lib/format";
  import { notify } from "$lib/notices.svelte";
  import { loadLibrary } from "$lib/library";
  import VideoActions from "$lib/VideoActions.svelte";
  import VideoRow from "$lib/VideoRow.svelte";
  import {
    createVideoActionState,
    videoRefOf,
  } from "$lib/video-actions.svelte";
  import {
    asErrorMessage,
    type FavoriteEntry,
    type Playlist,
    type PlaylistEntry,
    type WatchHistory,
  } from "$lib/players.svelte";
  import {
    queue,
    queueActive,
    queuePlayingAt,
    startQueue,
    stopQueue,
  } from "$lib/queue.svelte";

  type Tab = "history" | "favorites" | "playlists";
  let tab = $state<Tab>("history");

  let history = $state<WatchHistory[]>([]);
  let favorites = $state<FavoriteEntry[]>([]);

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

  // プレイリスト取り込み（FR-10、仕様決定 R）。URL と任意の名前を受ける
  let importUrl = $state("");
  let importing = $state(false);

  // 項目の並べ替え（FR-11、仕様決定 T）。DnD はドラッグ中の行番号と
  // ホバー先の番号を持ち、ドロップ時に一括 reorder として保存する
  let dragFrom = $state<number | null>(null);
  let dragOver = $state<number | null>(null);
  let reorderBusy = $state(false);
  let sortBusy = $state(false);

  // 行アクションの共通配線（FR-7）。ライブラリ固有の後処理
  // （お気に入り一覧からの除去・プレイリスト一覧の世代進め）だけ差し込む
  const va = createVideoActionState({
    onFavChange: (videoId, faved) => {
      if (!faved) {
        favorites = favorites.filter((f) => f.videoId !== videoId);
      }
    },
    onPlaylistCreated: () => {
      ++listsReq;
    },
  });

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
    va.favIds = lib.favIds;
    va.playlists = lib.playlists;
    favorites = await invoke<FavoriteEntry[]>("favorite_list");
  }

  onMount(async () => {
    try {
      await Promise.all([loadHistory(), loadFavorites()]);
    } catch (e) {
      notify(t("library.failed", { message: asErrorMessage(e) }));
    }
  });

  async function play(videoId: string, resume: boolean): Promise<void> {
    try {
      await invoke("play_video", { videoId, resume });
      goto("/");
    } catch (e) {
      notify(t("player.error", { message: asErrorMessage(e) }));
    }
  }

  async function removeHistory(videoId: string): Promise<void> {
    try {
      await invoke("history_remove", { videoId });
      history = history.filter((h) => h.videoId !== videoId);
      notify(t("library.removed"));
    } catch (e) {
      notify(t("library.removeFailed", { message: asErrorMessage(e) }));
    }
  }

  async function removeFavorite(videoId: string): Promise<void> {
    try {
      await invoke("favorite_remove", { videoId });
      va.onFavChange(videoId, false);
      notify(t("library.favorite.removed"));
    } catch (e) {
      notify(t("library.favorite.failed", { message: asErrorMessage(e) }));
    }
  }

  async function createPlaylist(): Promise<void> {
    const name = newPlaylistName.trim();
    if (!name) return;
    try {
      const pl = await invoke<Playlist>("playlist_create", { name });
      ++listsReq;
      va.playlists = [...va.playlists, pl];
      newPlaylistName = "";
      selectedId = pl.id;
      playlistItems = [];
      notify(t("library.playlist.created", { name: pl.name }));
    } catch (e) {
      notify(t("library.playlist.createFailed", { message: asErrorMessage(e) }));
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
      va.playlists = va.playlists.map((p) =>
        p.id === pl.id ? { ...p, name } : p,
      );
      renamingId = null;
      notify(t("library.playlist.renamed"));
    } catch (e) {
      notify(t("library.failed", { message: asErrorMessage(e) }));
    }
  }

  async function deletePlaylist(pl: Playlist): Promise<void> {
    try {
      await invoke("playlist_delete", { playlistId: pl.id });
      ++listsReq;
      va.playlists = va.playlists.filter((p) => p.id !== pl.id);
      if (selectedId === pl.id) {
        selectedId = null;
        playlistItems = [];
      }
      notify(t("library.playlist.deleted"));
    } catch (e) {
      notify(t("library.playlist.deleteFailed", { message: asErrorMessage(e) }));
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
      if (req === itemsReq) notify(t("library.failed", { message: asErrorMessage(e) }));
    }
  }

  // 一覧の再取得は常に一つの実行だけが所有し、並行する呼び出しは
  // 同じ Promise に合流する。互いの listsReq を失効させ合う再送や、
  // 監視者のいない再試行が発生しないようにする
  let listRun: Promise<void> | null = null;
  let listAgain = false;

  /// プレイリスト一覧の再取得（件数の最新化）。
  /// 取得中に他の更新が listsReq を進めたり新たな再取得要求が来たりしたら
  /// 取り直す。失効したまま放置すると追加した動画の件数が古いまま残るため。
  /// 失敗はこの呼び出しに合流した全員へ伝搬する（呼び出し側の catch で通知）。
  async function refreshPlaylists(): Promise<void> {
    if (listRun) {
      listAgain = true;
      return listRun;
    }
    listRun = (async () => {
      try {
        for (;;) {
          listAgain = false;
          const req = ++listsReq;
          const list = await invoke<Playlist[]>("playlist_list");
          if (req === listsReq) va.playlists = list;
          // 最新の応答を反映でき、かつ飛行中に新しい要求も無ければ終了。
          // どちらかが成り立たなければもう一周する
          if (req === listsReq && !listAgain) return;
        }
      } finally {
        listRun = null;
      }
    })();
    return listRun;
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
      notify(t("library.failed", { message: asErrorMessage(e) }));
    }
  }

  /// YouTube プレイリストの取り込み（FR-10、仕様決定 R）。
  /// `playlist_import` が作成から項目登録まで行う。名前は未指定なら
  /// バックエンドが取り込んだタイトルを使う
  async function importPlaylist(): Promise<void> {
    const url = importUrl.trim();
    if (!url || importing) return;
    importing = true;
    try {
      const pl = await invoke<Playlist>("playlist_import", { url });
      ++listsReq;
      va.playlists = [...va.playlists, pl];
      importUrl = "";
      notify(
        t("library.playlist.imported", { name: pl.name, count: pl.itemCount }),
      );
      await selectPlaylist(pl);
    } catch (e) {
      notify(
        t("library.playlist.importFailed", { message: asErrorMessage(e) }),
      );
    } finally {
      importing = false;
    }
  }

  /// 現在の項目順で playlist_reorder を呼び、失敗時は一覧を取り直す。
  /// 操作直後のローカル順を維持して再取得はしない（他操作との競合で
  /// 古い応答が上書きしないよう世代チェックは selectPlaylist 側に委ねる）
  async function persistOrder(): Promise<void> {
    if (selectedId === null || reorderBusy) return;
    reorderBusy = true;
    const plId = selectedId;
    try {
      await invoke("playlist_reorder", {
        playlistId: plId,
        videoIds: playlistItems.map((i) => i.videoId),
      });
    } catch (e) {
      notify(
        t("library.playlist.reorderFailed", { message: asErrorMessage(e) }),
      );
      // 失敗時は DB の内容に収束させる
      if (selectedId === plId) {
        try {
          playlistItems = await invoke<PlaylistEntry[]>("playlist_items", {
            playlistId: plId,
          });
        } catch {
          // 取り直しの失敗は既存表示のまま
        }
      }
    } finally {
      reorderBusy = false;
    }
  }

  /// 上下ボタンでの入れ替え（仕様決定 T）。隣と交換して保存する
  function moveItem(index: number, dir: -1 | 1): void {
    const to = index + dir;
    if (to < 0 || to >= playlistItems.length) return;
    const items = [...playlistItems];
    const [m] = items.splice(index, 1);
    items.splice(to, 0, m);
    playlistItems = items;
    void persistOrder();
  }

  /// DnD: ドラッグ開始。データ転送は識別だけ渡し、実体は dragFrom で管理
  function itemDragStart(index: number, e: DragEvent): void {
    dragFrom = index;
    e.dataTransfer?.setData("text/plain", playlistItems[index].videoId);
    if (e.dataTransfer) e.dataTransfer.effectAllowed = "move";
  }

  function itemDragOver(index: number, e: DragEvent): void {
    if (dragFrom === null) return;
    e.preventDefault();
    dragOver = index;
  }

  function itemDragLeave(index: number): void {
    if (dragOver === index) dragOver = null;
  }

  /// DnD: ドロップ位置へ移動して保存（仕様決定 T）
  function itemDrop(index: number, e: DragEvent): void {
    e.preventDefault();
    if (dragFrom !== null && dragFrom !== index) {
      const items = [...playlistItems];
      const [m] = items.splice(dragFrom, 1);
      items.splice(index, 0, m);
      playlistItems = items;
      void persistOrder();
    }
    dragFrom = null;
    dragOver = null;
  }

  function itemDragEnd(): void {
    dragFrom = null;
    dragOver = null;
  }

  /// 投稿日時の昇順で一括ソート（仕様決定 T）。取得日の無い項目は末尾
  async function sortByPublished(): Promise<void> {
    if (selectedId === null || sortBusy) return;
    sortBusy = true;
    const plId = selectedId;
    try {
      await invoke("playlist_sort", { playlistId: plId });
      playlistItems = await invoke<PlaylistEntry[]>("playlist_items", {
        playlistId: plId,
      });
      notify(t("library.playlist.sorted"));
    } catch (e) {
      notify(
        t("library.playlist.reorderFailed", { message: asErrorMessage(e) }),
      );
    } finally {
      sortBusy = false;
    }
  }

  /// その項目からの連続再生（FR-10、仕様決定 S）。選択項目を通常再生で起動し、
  /// 残りをキューに登録する。起動に使う resume は通常の再生と同じく true
  /// （暫定: キュー先頭項目もレジュームする）
  async function playQueue(index: number): Promise<void> {
    const it = playlistItems[index];
    if (!it || selectedId === null) return;
    try {
      stopQueue();
      const instanceId = await invoke<number>("play_video", {
        videoId: it.videoId,
        resume: true,
      });
      const pl = va.playlists.find((p) => p.id === selectedId);
      await startQueue(
        selectedId,
        pl?.name ?? "",
        playlistItems.map((i) => i.videoId),
        index,
        instanceId,
      );
      goto("/");
    } catch (e) {
      stopQueue();
      notify(
        t("library.playlist.queueFailed", { message: asErrorMessage(e) }),
      );
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
      va.playlists = va.playlists.map((p) =>
        p.id === selectedId ? { ...p, itemCount: p.itemCount - 1 } : p,
      );
      notify(t("library.removed"));
    } catch (e) {
      notify(t("library.removeFailed", { message: asErrorMessage(e) }));
    }
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
            {/snippet}
          </VideoRow>
        {/each}
      </ul>
    {/if}
  {:else if tab === "favorites"}
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
        <div class="pl-new">
          <input
            type="text"
            bind:value={importUrl}
            placeholder={t("library.playlist.importUrl")}
            onkeydown={(e) => e.key === "Enter" && void importPlaylist()}
          />
          <button
            onclick={() => void importPlaylist()}
            disabled={!importUrl.trim() || importing}
          >
            {importing ? t("library.playlist.importing") : t("library.playlist.import")}
          </button>
        </div>
        {#if va.playlists.length === 0}
          <p class="subtle">{t("library.playlists.empty")}</p>
        {:else}
          <ul class="pl-names">
            {#each va.playlists as pl (pl.id)}
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
          <div class="pl-tools">
            {#if selectedId !== null && queueActive(selectedId)}
              <span class="queue-badge">
                {t("library.playlist.queueActive", {
                  name: queue.playlistName,
                  index: queue.index + 1,
                  count: queue.items.length,
                })}
                <button class="link" onclick={() => stopQueue()}
                  >{t("library.playlist.queueStop")}</button
                >
              </span>
            {/if}
            <button
              class="link"
              disabled={sortBusy}
              onclick={() => void sortByPublished()}
              >{t("library.playlist.sort")}</button
            >
            <span class="subtle drag-hint">{t("library.playlist.dragHint")}</span>
          </div>
          <ul class="rows">
            {#each playlistItems as it, index (it.videoId)}
              <VideoRow
                videoId={it.videoId}
                title={it.title}
                thumbnailUrl={it.thumbnailUrl}
                onplay={() => play(it.videoId, true)}
                draggable={true}
                dropTarget={dragOver === index && dragFrom !== index}
                ondragstart={(e) => itemDragStart(index, e)}
                ondragover={(e) => itemDragOver(index, e)}
                ondragleave={() => itemDragLeave(index)}
                ondrop={(e) => itemDrop(index, e)}
                ondragend={() => itemDragEnd()}
              >
                {#snippet leading()}
                  <span class="pos">{index + 1}</span>
                {/snippet}
                {#snippet sub()}{it.channelTitle ?? ""}{/snippet}
                {#snippet actions()}
                  <button
                    onclick={() => void playQueue(index)}
                    disabled={queuePlayingAt(it.videoId)}
                    >{t("library.playlist.queue")}</button
                  >
                  <button onclick={() => play(it.videoId, true)}
                    >{t("library.play")}</button
                  >
                  <button
                    class="link"
                    title={t("library.playlist.moveUp")}
                    disabled={index === 0 || reorderBusy}
                    onclick={() => moveItem(index, -1)}>↑</button
                  >
                  <button
                    class="link"
                    title={t("library.playlist.moveDown")}
                    disabled={index === playlistItems.length - 1 || reorderBusy}
                    onclick={() => moveItem(index, 1)}>↓</button
                  >
                  <button
                    class="link danger"
                    onclick={() => removeItem(it)}
                    >{t("library.remove")}</button
                  >
                {/snippet}
              </VideoRow>
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

  .pos {
    width: 24px;
    color: #9aa0a6;
    text-align: right;
    flex-shrink: 0;
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

  .pl-tools {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-bottom: 8px;
  }

  .queue-badge {
    color: #8ab4f8;
    font-size: 0.85rem;
  }

  .drag-hint {
    font-size: 0.8rem;
    margin-left: auto;
  }

  @media (max-width: 700px) {
    .pl-grid {
      grid-template-columns: 1fr;
    }
  }
</style>
