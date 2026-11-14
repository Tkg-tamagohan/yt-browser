// VideoActions 行アクション（FR-7: お気に入り・プレイリスト、
// FR-59: あとで見る）の配線。
// feed・search・library・RelatedPanel に重複していた、favIds/playlists/
// watchLaterIds の取得とコールバック受け渡しをここに集約する。
// 動画行のコンテキストメニュー（FR-78、仕様決定 DC）の共有状態もここに置く

import { loadLibrary } from "$lib/library";
import type { Playlist, VideoRef } from "$lib/players.svelte";

/// 各行型（SearchResult / FeedItem / FavoriteEntry / WatchHistory など、
/// 同じフィールド名を持つ型）から VideoActions へ渡す VideoRef を作る。
/// 履歴行のようにサムネイルを持たない行型は省略可で、null として渡す
/// （videos 台帳の既存値は video_upsert の COALESCE で維持される）。
export function videoRefOf(v: {
  videoId: string;
  title: string;
  channelId: string | null;
  channelTitle: string | null;
  thumbnailUrl?: string | null;
}): VideoRef {
  return {
    videoId: v.videoId,
    title: v.title,
    channelId: v.channelId,
    channelTitle: v.channelTitle,
    thumbnailUrl: v.thumbnailUrl ?? null,
  };
}

type Options = {
  /// お気に入りトグル後の追加処理（ライブラリは一覧からの除去に使う）
  onFavChange?: (videoId: string, faved: boolean) => void;
  /// プレイリスト新規作成後の追加処理（ライブラリは世代番号の更新に使う）
  onPlaylistCreated?: (pl: Playlist) => void;
  /// あとで見るトグル後の追加処理（ライブラリは新タブの一覧更新に使う）
  onWatchLaterChange?: (videoId: string, watchLater: boolean) => void;
  /// 変更コールバックのあとに loadLibrary で DB と再同期する
  /// （関連パネルは他ページでの編集を取り込むため true にする）
  resyncOnChange?: boolean;
};

/// favIds/playlists/watchLaterIds の状態と VideoActions のコールバックを
/// まとめて作る。refresh は発行順を守る loadLibrary のラッパで、
/// より新しい取得が走っていれば古い応答は捨てる。失敗は静かに握る
/// （行アクションが出せなくても各行の操作は継続できる）。
export function createVideoActionState(opts: Options = {}) {
  let favIds = $state<Set<string>>(new Set());
  let playlists = $state<Playlist[]>([]);
  let watchLaterIds = $state<Set<string>>(new Set());
  let loadSeq = 0;

  async function refresh(): Promise<void> {
    const seq = ++loadSeq;
    try {
      const lib = await loadLibrary();
      if (seq !== loadSeq) return;
      favIds = lib.favIds;
      playlists = lib.playlists;
      watchLaterIds = lib.watchLaterIds;
    } catch {
      // 行アクションが出せなくても再生など各行の操作は使えるため静かに握る
    }
  }

  function onFavChange(videoId: string, faved: boolean): void {
    const next = new Set(favIds);
    if (faved) next.add(videoId);
    else next.delete(videoId);
    favIds = next;
    opts.onFavChange?.(videoId, faved);
    if (opts.resyncOnChange) void refresh();
  }

  function onPlaylistCreated(pl: Playlist): void {
    playlists = [...playlists, pl];
    opts.onPlaylistCreated?.(pl);
    if (opts.resyncOnChange) void refresh();
  }

  function onWatchLaterChange(videoId: string, watchLater: boolean): void {
    const next = new Set(watchLaterIds);
    if (watchLater) next.add(videoId);
    else next.delete(videoId);
    watchLaterIds = next;
    opts.onWatchLaterChange?.(videoId, watchLater);
    if (opts.resyncOnChange) void refresh();
  }

  return {
    get favIds() {
      return favIds;
    },
    set favIds(v: Set<string>) {
      favIds = v;
    },
    get playlists() {
      return playlists;
    },
    set playlists(v: Playlist[]) {
      playlists = v;
    },
    get watchLaterIds() {
      return watchLaterIds;
    },
    set watchLaterIds(v: Set<string>) {
      watchLaterIds = v;
    },
    refresh,
    onFavChange,
    onPlaylistCreated,
    onWatchLaterChange,
  };
}

/// `createVideoActionState` が返す行アクション状態の型。
/// コンテキストメニューは開いた側ページの 1 インスタンスを借用する
export type VideoActionState = ReturnType<typeof createVideoActionState>;

/// コンテキストメニュー（FR-78、仕様決定 DC）の開く対象。
/// video は対象行の VideoRef、va は開いた側の行アクション状態
/// （プレイリスト一覧と作成コールバックをそのまま使う）。
/// onplaylistadd はページ固有の追加通知（ライブラリは件数・項目の
/// 読み直しに使う）で、無いページでは省略する
export type VideoContextMenuTarget = {
  video: VideoRef;
  /// メニューを出すクリック位置（clientX/clientY）
  x: number;
  y: number;
  va: VideoActionState;
  onplaylistadd?: (playlistId: number) => void;
};

/// 開いているコンテキストメニューの対象。レイアウトに 1 つだけ
/// マウントされた VideoContextMenu がここを読んで表示する。
/// 同時に開けるメニューは 1 つで、別行での右クリックは開き直しになる
export const videoContextMenu = $state<{
  current: VideoContextMenuTarget | null;
}>({ current: null });

/// 動画行の右クリックでコンテキストメニューを開く。
/// OS 既定のコンテキストメニューは抑止し、行アクションと同じ
/// プレイリスト追加メニューをクリック位置に出す
export function openVideoContextMenu(
  e: MouseEvent,
  video: VideoRef,
  va: VideoActionState,
  onplaylistadd?: (playlistId: number) => void,
): void {
  e.preventDefault();
  videoContextMenu.current = { video, va, onplaylistadd, x: e.clientX, y: e.clientY };
}

/// コンテキストメニューを閉じる。メニュー外の操作・Escape・画面遷移で畳む
export function closeVideoContextMenu(): void {
  videoContextMenu.current = null;
}
