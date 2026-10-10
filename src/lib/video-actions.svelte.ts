// VideoActions 行アクション（FR-7: お気に入り・プレイリスト）の配線。
// feed・search・library・RelatedPanel に重複していた、favIds/playlists の
// 取得とコールバック受け渡しをここに集約する。

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
  /// 変更コールバックのあとに loadLibrary で DB と再同期する
  /// （関連パネルは他ページでの編集を取り込むため true にする）
  resyncOnChange?: boolean;
};

/// favIds/playlists の状態と VideoActions のコールバックをまとめて作る。
/// refresh は発行順を守る loadLibrary のラッパで、より新しい取得が
/// 走っていれば古い応答は捨てる。失敗は静かに握る
/// （行アクションが出せなくても各行の操作は継続できる）。
export function createVideoActionState(opts: Options = {}) {
  let favIds = $state<Set<string>>(new Set());
  let playlists = $state<Playlist[]>([]);
  let loadSeq = 0;

  async function refresh(): Promise<void> {
    const seq = ++loadSeq;
    try {
      const lib = await loadLibrary();
      if (seq !== loadSeq) return;
      favIds = lib.favIds;
      playlists = lib.playlists;
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
    refresh,
    onFavChange,
    onPlaylistCreated,
  };
}
