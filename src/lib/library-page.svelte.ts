// library ページ（FR-7: 履歴・お気に入り・プレイリスト）の状態と操作。
// +page.svelte に同居していた script ロジックをここへ集約し、
// ページ側はタブ状態とイベント購読のライフサイクルだけを持つ。

import { goto } from "$app/navigation";
import { invoke } from "@tauri-apps/api/core";
import { t, type MessageKey } from "$lib/i18n";
import { notify } from "$lib/notices.svelte";
import { loadLibrary } from "$lib/library";
import { createVideoActionState } from "$lib/video-actions.svelte";
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
  rememberMeta,
  startQueue,
} from "$lib/queue.svelte";

type Options = {
  /// library://playlists_changed 受信時にプレイリストタブへ切り替える。
  /// tab 状態はページ側が持つため、切り替えだけコールバックで戻す
  goToPlaylistsTab: () => void;
};

/// library ページの状態・取得・編集ロジックをまとめて作る。
/// 履歴/お気に入りは初回ロードのみで、タブ切替では再取得しない
/// （タブより上で所有しないと切替ごとの再取得に変わるため）。
/// 世代番号（itemsReq/listsReq）・直列化 Promise（listRun/reorderRun）・
/// PL_PAGE は内部詳細として公開しない
export function createLibraryPageState(opts: Options) {
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

  async function loadHistory(): Promise<void> {
    history = await invoke<WatchHistory[]>("history_list", { limit: 500 });
  }

  async function loadFavorites(): Promise<void> {
    const lib = await loadLibrary();
    va.favIds = lib.favIds;
    va.playlists = lib.playlists;
    favorites = await invoke<FavoriteEntry[]>("favorite_list");
  }

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
    opts.goToPlaylistsTab();
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
  /// 起動し、残りをそのプレイリストのキューへ登録する。同じプレイリストの
  /// 既存キューは startQueue 側で新しいスナップショットへ上書きする
  /// （他のキューは動かさない）。起動に使う resume は通常の再生と
  /// 同じく true（暫定: キュー先頭項目もレジュームする）。
  /// 既存キューが実行中なら新しい窓を開かず、そのインスタンスへ
  /// 開始項目を読み替えてセッションを引き継ぐ（窓位置を保つ）
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
    const runningId = queueAt(plId)?.instanceId ?? null;
    let instanceId: number;
    try {
      if (runningId !== null) {
        // 実行中キューへの再開始: 同じ mpv へ読み替えて引き継ぐ
        instanceId = runningId;
        await invoke("player_switch", {
          instanceId,
          videoId: it.videoId,
          resume: true,
        });
      } else {
        instanceId = await invoke<number>("play_video", {
          videoId: it.videoId,
          resume: true,
        });
      }
    } catch (e) {
      notify(
        t("library.playlist.queueFailed", { message: asErrorMessage(e) }),
      );
      return;
    }
    try {
      // スナップショット項目の表示メタを登録する
      // （キュー内の一覧が動画 ID のまま出ないように）
      for (const i of playlistItems) rememberMeta(i);
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
      // 武装失敗は armQueue 側で畳むためここには来ない。
      // startQueue が途中で例外を投げた場合の保険として、
      // 作りかけのキューを畳んでおく
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

  return {
    va,
    get history() {
      return history;
    },
    set history(v: WatchHistory[]) {
      history = v;
    },
    get favorites() {
      return favorites;
    },
    set favorites(v: FavoriteEntry[]) {
      favorites = v;
    },
    get selectedId() {
      return selectedId;
    },
    set selectedId(v: number | null) {
      selectedId = v;
    },
    get playlistItems() {
      return playlistItems;
    },
    set playlistItems(v: PlaylistEntry[]) {
      playlistItems = v;
    },
    get plHasMore() {
      return plHasMore;
    },
    set plHasMore(v: boolean) {
      plHasMore = v;
    },
    get plLoadingMore() {
      return plLoadingMore;
    },
    set plLoadingMore(v: boolean) {
      plLoadingMore = v;
    },
    get newPlaylistName() {
      return newPlaylistName;
    },
    set newPlaylistName(v: string) {
      newPlaylistName = v;
    },
    get renamingId() {
      return renamingId;
    },
    set renamingId(v: number | null) {
      renamingId = v;
    },
    get renameText() {
      return renameText;
    },
    set renameText(v: string) {
      renameText = v;
    },
    get importUrl() {
      return importUrl;
    },
    set importUrl(v: string) {
      importUrl = v;
    },
    get importName() {
      return importName;
    },
    set importName(v: string) {
      importName = v;
    },
    get importing() {
      return importing;
    },
    set importing(v: boolean) {
      importing = v;
    },
    get dragFrom() {
      return dragFrom;
    },
    set dragFrom(v: number | null) {
      dragFrom = v;
    },
    get dragOver() {
      return dragOver;
    },
    set dragOver(v: number | null) {
      dragOver = v;
    },
    get reorderBusy() {
      return reorderBusy;
    },
    set reorderBusy(v: boolean) {
      reorderBusy = v;
    },
    get sortBusy() {
      return sortBusy;
    },
    set sortBusy(v: boolean) {
      sortBusy = v;
    },
    loadHistory,
    loadFavorites,
    play,
    removeHistory,
    removeFavorite,
    createPlaylist,
    beginRename,
    commitRename,
    deletePlaylist,
    selectPlaylist,
    loadMoreItems,
    onPlaylistsChanged,
    onPlaylistAdd,
    importPlaylist,
    moveItem,
    itemDragStart,
    itemDragOver,
    itemDragLeave,
    itemDrop,
    itemDragEnd,
    sortByPublished,
    reverseItems,
    playQueue,
    removeItem,
  };
}
