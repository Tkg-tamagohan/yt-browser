// お気に入り・プレイリスト・あとで見るを各ページで共用するロード関数
// （FR-7、FR-59）。一覧ページ・行アクションが同じ形で初期値を取るためにまとめる。
import { invoke } from "@tauri-apps/api/core";
import type {
  FavoriteEntry,
  Playlist,
  WatchLaterEntry,
} from "./players.svelte";

/// お気に入り登録済みの video_id 集合・プレイリスト一覧・
/// あとで見る登録済みの video_id 集合を一度に取る。
/// 失敗は呼び出し側に投げる（ページ側で通知して空のままにする運用）。
export async function loadLibrary(): Promise<{
  favIds: Set<string>;
  playlists: Playlist[];
  watchLaterIds: Set<string>;
}> {
  const [favs, pls, wl] = await Promise.all([
    invoke<FavoriteEntry[]>("favorite_list"),
    invoke<Playlist[]>("playlist_list"),
    invoke<WatchLaterEntry[]>("watch_later_list"),
  ]);
  return {
    favIds: new Set(favs.map((f) => f.videoId)),
    playlists: pls,
    watchLaterIds: new Set(wl.map((w) => w.videoId)),
  };
}
