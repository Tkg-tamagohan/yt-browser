<script lang="ts">
  // ローカルデータ画面（FR-7）: 視聴履歴・お気に入り・プレイリストの
  // 一覧・編集・削除をここで完結させる。
  import { onDestroy, onMount } from "svelte";
  import { goto } from "$app/navigation";
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { t, type MessageKey } from "$lib/i18n";
  import { fmtDateTime } from "$lib/format";
  import { notify } from "$lib/notices.svelte";
  import { loadLibrary } from "$lib/library";
  import QueueButtons from "$lib/QueueButtons.svelte";
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
    discardQueue,
    queueAt,
    queuePlayingAt,
    startQueue,
    queueStop,
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
  // 「さらに読み込む」の状態（FR-25、仕様決定 AR）。
  // plTailPos は取得済み末尾の position。ローカル並べ替えで
  // playlistItems の末尾要素が入れ替わってもカーソルは動かないため、
  // 応答の末尾 position だけで別管理する
  const PL_PAGE = 100;
  let plTailPos = $state(0);
  let plHasMore = $state(false);
  let plLoadingMore = $state(false);
  let newPlaylistName = $state("");
  let renamingId = $state<number | null>(null);
  let renameText = $state("");

  // プレイリスト取り込み（FR-10、仕様決定 R）。URL と任意の名前を受ける
  let importUrl = $state("");
  let importName = $state("");
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

  // 画面外からのプレイリスト変更通知（library://playlists_changed、
  // 設計書 §3.2）の購読解除関数。マウント中だけ listen する
  let unlistens: UnlistenFn[] = [];

  onMount(async () => {
    // 購読の失敗で初回ロードまで止めないよう、独立して試す
    try {
      unlistens.push(
        await listen<Playlist>("library://playlists_changed", (ev) => {
          void onPlaylistsChanged(ev.payload);
        }),
      );
    } catch (e) {
      notify(t("library.failed", { message: asErrorMessage(e) }));
    }
    try {
      await Promise.all([loadHistory(), loadFavorites()]);
    } catch (e) {
      notify(t("library.failed", { message: asErrorMessage(e) }));
    }
  });

  onDestroy(() => {
    unlistens.forEach((u) => u());
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
      // 対象プレイリストのキューは消滅させる（FR-26）
      discardQueue(pl.id);
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
    plHasMore = false;
    const req = ++itemsReq;
    try {
      const items = await invoke<PlaylistEntry[]>("playlist_items", {
        playlistId: pl.id,
        afterPosition: null,
        limit: PL_PAGE,
      });
      // 応答が返るまでに別のプレイリストに切り替わっていたら捨てる
      if (req !== itemsReq || selectedId !== pl.id) return;
      playlistItems = items;
      const last = items[items.length - 1];
      if (last) plTailPos = last.position;
      plHasMore = items.length >= PL_PAGE;
    } catch (e) {
      if (req === itemsReq) notify(t("library.failed", { message: asErrorMessage(e) }));
    }
  }

  /// 次ページを末尾へ追記する（FR-25、仕様決定 AR）。
  /// 取得済み分を置き換えず、応答の末尾 position でカーソルを進める
  async function loadMoreItems(): Promise<void> {
    const plId = selectedId;
    if (plId === null || plLoadingMore) return;
    plLoadingMore = true;
    // 世代は進めない。選択の切替や再読み込みが始まれば
    // この追記は自動で失効し、古いプレイリストの続きが
    // 新しい一覧へ混入しない
    const req = itemsReq;
    try {
      const res = await invoke<PlaylistEntry[]>("playlist_items", {
        playlistId: plId,
        afterPosition: plTailPos,
        limit: PL_PAGE,
      });
      if (req !== itemsReq || selectedId !== plId) return;
      const seen = new Set(playlistItems.map((i) => i.videoId));
      playlistItems = [
        ...playlistItems,
        ...res.filter((i) => !seen.has(i.videoId)),
      ];
      const last = res[res.length - 1];
      if (last) plTailPos = last.position;
      plHasMore = res.length >= PL_PAGE;
    } catch (e) {
      if (req === itemsReq) {
        notify(t("library.failed", { message: asErrorMessage(e) }));
      }
    } finally {
      plLoadingMore = false;
    }
  }

  /// 全件必要な操作（並べ替え保存・連続再生）の前に、
  /// 未取得の末尾を取り切る。戻り値は選択状態が有効なまま反映できたか
  async function ensureAllItemsLoaded(plId: number): Promise<boolean> {
    if (!plHasMore) return true;
    const req = ++itemsReq;
    const res = await invoke<PlaylistEntry[]>("playlist_items", {
      playlistId: plId,
      afterPosition: plTailPos,
      limit: null,
    });
    if (req !== itemsReq || selectedId !== plId) return false;
    const seen = new Set(playlistItems.map((i) => i.videoId));
    playlistItems = [
      ...playlistItems,
      ...res.filter((i) => !seen.has(i.videoId)),
    ];
    plHasMore = false;
    return true;
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

  /// 画面外からのプレイリスト取り込み（library://playlists_changed）の反映。
  /// 一覧を再取得し、取り込まれたプレイリストを選択する
  /// （画面内取り込みの importPlaylist → selectPlaylist と同等の UX）。
  /// 一覧の再取得に失敗しても選択と項目取得は試行する
  /// （取り込み成功自体はイベントの発火が保証している）
  async function onPlaylistsChanged(pl: Playlist): Promise<void> {
    tab = "playlists";
    try {
      await refreshPlaylists();
    } catch (e) {
      notify(t("library.failed", { message: asErrorMessage(e) }));
    }
    await selectPlaylist(pl);
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
          const last = items[items.length - 1];
          if (last) plTailPos = last.position;
          plHasMore = false;
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
      const name = importName.trim();
      const pl = await invoke<Playlist>("playlist_import", {
        url,
        name: name === "" ? null : name,
      });
      ++listsReq;
      // バックエンドの emit が invoke 応答より先に届くと、イベント側の
      // refreshPlaylists が新しいプレイリストを含む一覧で上書き済みになる。
      // その場合に無条件で追加すると同 ID の行が重複するため、未登録の
      // ときだけ追加する
      if (!va.playlists.some((p) => p.id === pl.id)) {
        va.playlists = [...va.playlists, pl];
      }
      importUrl = "";
      importName = "";
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

  /// 並べ替え保存の直列化。飛行中に次の変更が来たら最新順で追い送りし、
  /// 一括操作（ソート・反転）の実行中は新しい保存を始めない。
  /// 一括操作側は reorderRun を await して保存の確定を待つ
  let reorderRun: Promise<void> | null = null;
  let reorderAgain = false;

  /// 現在の項目順で playlist_reorder を呼び、失敗時は一覧を取り直す。
  /// 操作直後のローカル順を維持して再取得はしない（他操作との競合で
  /// 古い応答が上書きしないよう世代チェックは selectPlaylist 側に委ねる）
  async function persistOrder(): Promise<void> {
    if (selectedId === null || sortBusy) return;
    if (reorderRun) {
      // 実行中の保存に合流し、最新の表示順で一周追加させる。
      // 合流で捨てると先の確定順が巻き戻って保存されるため
      reorderAgain = true;
      return reorderRun;
    }
    reorderBusy = true;
    reorderRun = (async () => {
      try {
        for (;;) {
          reorderAgain = false;
          const plId = selectedId;
          if (plId === null) return;
          // playlist_reorder は全件集合を要求するため、
          // ページングで未取得の末尾があれば先に取り切る。
          // 未取得分の取得失敗は保存を行わず通知だけ出す
          try {
            if (!(await ensureAllItemsLoaded(plId))) return;
          } catch (e) {
            notify(
              t("library.playlist.reorderFailed", {
                message: asErrorMessage(e),
              }),
            );
            return;
          }
          try {
            await invoke("playlist_reorder", {
              playlistId: plId,
              videoIds: playlistItems.map((i) => i.videoId),
            });
          } catch (e) {
            notify(
              t("library.playlist.reorderFailed", {
                message: asErrorMessage(e),
              }),
            );
            // 失敗時は DB の内容に収束させる（自分の取得を最新世代にする）
            if (selectedId === plId) {
              const req = ++itemsReq;
              try {
                const items = await invoke<PlaylistEntry[]>(
                  "playlist_items",
                  { playlistId: plId },
                );
                if (req === itemsReq && selectedId === plId) {
                  playlistItems = items;
                  const last = items[items.length - 1];
                  if (last) plTailPos = last.position;
                  plHasMore = false;
                }
              } catch {
                // 取り直しの失敗は既存表示のまま
              }
            }
            return;
          }
          if (!reorderAgain) return;
        }
      } finally {
        reorderRun = null;
        reorderBusy = false;
      }
    })();
    return reorderRun;
  }

  /// 上下ボタンでの入れ替え（仕様決定 T）。隣と交換して保存する
  function moveItem(index: number, dir: -1 | 1): void {
    // 一括操作の実行中は行操作を受け付けない（適用順を確定操作→一括操作に固定）
    if (sortBusy) return;
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
    if (sortBusy) return;
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

  /// 一括操作（ソート・反転）の共通直列化。飛行中の並べ替え保存を
  /// 先に確定させてから一括操作を実行し、結果の反映は自分の取得を
  /// 最新世代にして行う（選択切り替えや古い応答の上書きを防ぐ）。
  /// op がエラーを投げた場合だけ失敗を通知する
  async function runBulkOp(
    op: (plId: number) => Promise<void>,
    doneKey: MessageKey,
  ): Promise<void> {
    if (selectedId === null || sortBusy) return;
    sortBusy = true;
    const plId = selectedId;
    try {
      await reorderRun;
      if (selectedId !== plId) return;
      const req = ++itemsReq;
      await op(plId);
      const items = await invoke<PlaylistEntry[]>("playlist_items", {
        playlistId: plId,
      });
      if (req !== itemsReq || selectedId !== plId) return;
      playlistItems = items;
      const last = items[items.length - 1];
      if (last) plTailPos = last.position;
      plHasMore = false;
      notify(t(doneKey));
    } catch (e) {
      notify(
        t("library.playlist.reorderFailed", { message: asErrorMessage(e) }),
      );
    } finally {
      sortBusy = false;
    }
  }

  /// 投稿日時の昇順で一括ソート（仕様決定 T）。取得日の無い項目は末尾
  async function sortByPublished(): Promise<void> {
    await runBulkOp(
      (plId) => invoke("playlist_sort", { playlistId: plId }),
      "library.playlist.sorted",
    );
  }

  /// 現在の項目順を一括で反転（仕様決定 Z）。
  /// 新しい順で取り込んだプレイリストを古い順へ変える用途
  async function reverseItems(): Promise<void> {
    await runBulkOp(
      (plId) => invoke("playlist_reverse", { playlistId: plId }),
      "library.playlist.reversed",
    );
  }

  /// その項目からの連続再生（FR-10・FR-26、仕様決定 S・AS）。選択項目を
  /// 通常再生で起動し、残りをそのプレイリストのキューへ登録する。
  /// 同じプレイリストの既存キューは startQueue 側で新しいスナップショットへ
  /// 上書きする（他のキューは動かさない）。起動に使う resume は通常の再生と
  /// 同じく true（暫定: キュー先頭項目もレジュームする）
  async function playQueue(index: number): Promise<void> {
    if (selectedId === null) return;
    const plId = selectedId;
    // 連続再生はプレイリスト全件をキューにするため、
    // ページングで未取得の末尾があれば先に取り切る
    try {
      if (!(await ensureAllItemsLoaded(plId))) return;
    } catch (e) {
      notify(t("library.failed", { message: asErrorMessage(e) }));
      return;
    }
    const it = playlistItems[index];
    if (!it) return;
    // 再生の起動に失敗した段階では既存キューを維持する
    // （キューの置き換えは新しいキューの作成が進んだ時点で行う）
    let instanceId: number;
    try {
      instanceId = await invoke<number>("play_video", {
        videoId: it.videoId,
        resume: true,
      });
    } catch (e) {
      notify(
        t("library.playlist.queueFailed", { message: asErrorMessage(e) }),
      );
      return;
    }
    try {
      const pl = va.playlists.find((p) => p.id === plId);
      await startQueue(
        plId,
        pl?.name ?? "",
        playlistItems.map((i) => i.videoId),
        index,
        instanceId,
      );
      goto("/");
    } catch (e) {
      // 武装に失敗した場合は作りかけのキューを畳む
      // （既存キューは startQueue 内で既に上書き済み）
      discardQueue(plId);
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
          <input
            type="text"
            bind:value={importName}
            placeholder={t("library.playlist.importName")}
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
            {#if selectedId !== null}
              {@const selQueue = queueAt(selectedId)}
              {#if selQueue !== undefined}
                <span class="queue-badge">
                  {#if selQueue.instanceId !== null}
                    {t("library.playlist.queueActive", {
                      name: selQueue.playlistName,
                      index: selQueue.index + 1,
                      count: selQueue.items.length,
                    })}
                    <button class="link" onclick={() => queueStop(selectedId)}
                      >{t("library.playlist.queueStop")}</button
                    >
                  {:else}
                    {t("library.playlist.queueSaved", {
                      name: selQueue.playlistName,
                      index: selQueue.index + 1,
                      count: selQueue.items.length,
                    })}
                  {/if}
                </span>
              {/if}
            {/if}
            <button
              class="link"
              disabled={sortBusy}
              onclick={() => void sortByPublished()}
              >{t("library.playlist.sort")}</button
            >
            <button
              class="link"
              disabled={sortBusy}
              onclick={() => void reverseItems()}
              >{t("library.playlist.reverse")}</button
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
                draggable={!sortBusy}
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
                    disabled={queuePlayingAt(selectedId, it.videoId)}
                    >{t("library.playlist.queue")}</button
                  >
                  <button onclick={() => play(it.videoId, true)}
                    >{t("library.play")}</button
                  >
                  <button
                    class="link"
                    title={t("library.playlist.moveUp")}
                    disabled={index === 0 || reorderBusy || sortBusy}
                    onclick={() => moveItem(index, -1)}>↑</button
                  >
                  <button
                    class="link"
                    title={t("library.playlist.moveDown")}
                    disabled={index === playlistItems.length - 1 ||
                      reorderBusy ||
                      sortBusy}
                    onclick={() => moveItem(index, 1)}>↓</button
                  >
                  <button
                    class="link danger"
                    onclick={() => removeItem(it)}
                    >{t("library.remove")}</button
                  >
                  <QueueButtons video={videoRefOf(it)} />
                {/snippet}
              </VideoRow>
            {/each}
          </ul>
          {#if plHasMore}
            <button
              class="link load-more"
              onclick={() => void loadMoreItems()}
              disabled={plLoadingMore}
            >
              {plLoadingMore
                ? t("related.loading")
                : t("library.playlist.loadMore")}
            </button>
          {/if}
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
