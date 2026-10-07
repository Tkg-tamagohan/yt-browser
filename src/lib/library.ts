// お気に入りとプレイリストを各ページで共用するロード関数（FR-7）。
// 一覧ページ・行アクションが同じ形で初期値を取るためにまとめる。
import { invoke } from "@tauri-apps/api/core";
import type { FavoriteEntry, Playlist } from "./players.svelte";

/// お気に入り登録済みの video_id 集合とプレイリスト一覧を一度に取る。
/// 失敗は呼び出し側に投げる（ページ側で通知して空のままにする運用）。
export async function loadLibrary(): Promise<{
  favIds: Set<string>;
  playlists: Playlist[];
}> {
  const [favs, pls] = await Promise.all([
    invoke<FavoriteEntry[]>("favorite_list"),
    invoke<Playlist[]>("playlist_list"),
  ]);
  return { favIds: new Set(favs.map((f) => f.videoId)), playlists: pls };
}
