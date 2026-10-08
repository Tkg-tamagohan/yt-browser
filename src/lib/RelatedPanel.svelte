<script lang="ts">
  // 関連動画パネル。開いている間だけマウントされ、開くたびに get_related を
  // 取り直す（ブロック状態の変化と前回失敗の再試行に対応するためキャッシュしない）。
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { t } from "$lib/i18n";
  import { loadLibrary } from "$lib/library";
  import { notify } from "$lib/notices.svelte";
  import VideoActions from "$lib/VideoActions.svelte";
  import {
    asErrorMessage,
    type Playlist,
    type SearchResult,
    type VideoRef,
  } from "$lib/players.svelte";

  // instanceId はカード側がパネルをインスタンスへ対応づけるためのキー
  let { instanceId, videoId }: { instanceId: number; videoId: string } =
    $props();

  let loading = $state(true);
  let items = $state<SearchResult[]>([]);

  // お気に入り・プレイリスト行アクション用（FR-7）。
  // 他ページ（ライブラリ等）でも編集されるため、パネルを開くたびに再取得する
  // （従来はページのリマウントと「/」復帰が暗黙の再取得トリガーだった）。
  // loadSeq は発行順：より新しいロードが走っていれば古い応答は丸ごと捨てる。
  // ローカル編集は表示を即時反映したうえで再取得を投げ、DB の truth に収束させる
  // （コールバックは書き込みコミット後に発火するため、再取得は編集済みの値を含む）。
  let favIds = $state<Set<string>>(new Set());
  let playlists = $state<Playlist[]>([]);
  let loadSeq = 0;

  async function refreshLibrary(): Promise<void> {
    const seq = ++loadSeq;
    try {
      const lib = await loadLibrary();
      if (seq !== loadSeq) return;
      favIds = lib.favIds;
      playlists = lib.playlists;
    } catch {
      // 行アクションが出せなくても再生は使えるため静かに握る
    }
  }

  async function loadRelated(): Promise<void> {
    loading = true;
    try {
      items = await invoke<SearchResult[]>("get_related", { videoId });
    } catch (e) {
      items = [];
      notify(t("related.failed", { message: asErrorMessage(e) }));
    } finally {
      loading = false;
    }
  }

  async function playRelated(r: SearchResult): Promise<void> {
    try {
      await invoke("play_video", { videoId: r.videoId, resume: true });
    } catch (e) {
      notify(t("player.error", { message: asErrorMessage(e) }));
    }
  }

  async function blockRelated(r: SearchResult): Promise<void> {
    if (!r.channelId) return;
    try {
      await invoke("block_channel", {
        channelId: r.channelId,
        title: r.channelTitle ?? r.channelId,
      });
      items = items.filter((x) => x.channelId !== r.channelId);
      notify(t("blocked.added", { title: r.channelTitle ?? r.channelId }));
    } catch (e) {
      notify(t("blocked.addFailed", { message: asErrorMessage(e) }));
    }
  }

  function videoRefOf(r: SearchResult): VideoRef {
    return {
      videoId: r.videoId,
      title: r.title,
      channelId: r.channelId,
      channelTitle: r.channelTitle,
      thumbnailUrl: r.thumbnailUrl,
    };
  }

  function onFavChange(videoId: string, faved: boolean): void {
    const next = new Set(favIds);
    if (faved) next.add(videoId);
    else next.delete(videoId);
    favIds = next;
    // 他ページでの編集も含め DB と再同期（古い進行中ロードは loadSeq で捨てる）
    void refreshLibrary();
  }

  function onPlaylistCreated(pl: Playlist): void {
    playlists = [...playlists, pl];
    void refreshLibrary();
  }

  onMount(() => {
    void loadRelated();
    void refreshLibrary();
  });
</script>

<div class="related">
  <h3>{t("related.title")}</h3>
  {#if loading}
    <p class="subtle">{t("related.loading")}</p>
  {:else if items.length === 0}
    <p class="subtle">{t("related.empty")}</p>
  {:else}
    <ul class="related-list">
      {#each items as r (r.videoId)}
        <li class="related-item">
          {#if r.thumbnailUrl}
            <img class="thumb" src={r.thumbnailUrl} alt="" />
          {/if}
          <div class="meta">
            <div class="title">{r.title}</div>
            <div class="sub">{r.channelTitle ?? ""}</div>
            <div class="actions">
              <button onclick={() => playRelated(r)}>{t("search.play")}</button>
              {#if r.channelId}
                <button
                  class="danger"
                  onclick={() => blockRelated(r)}
                >
                  {t("search.block")}
                </button>
              {/if}
              <VideoActions
                video={videoRefOf(r)}
                faved={favIds.has(r.videoId)}
                {playlists}
                onfavchange={onFavChange}
                onplaylistcreated={onPlaylistCreated}
              />
            </div>
          </div>
        </li>
      {/each}
    </ul>
  {/if}
</div>

<style>
  .related {
    margin-top: 12px;
    border-top: 1px solid #3c4043;
    padding-top: 8px;
  }

  .related h3 {
    font-size: 1rem;
    margin: 0 0 8px;
  }

  .related-list {
    list-style: none;
    padding: 0;
    margin: 0;
    max-height: 320px;
    overflow-y: auto;
  }

  .related-item {
    display: flex;
    gap: 10px;
    padding: 6px 0;
    align-items: flex-start;
  }

  .related-item .thumb {
    width: 120px;
    aspect-ratio: 16 / 9;
    object-fit: cover;
    border-radius: 6px;
    background: #26282c;
  }

  .related-item .meta {
    flex: 1;
    min-width: 0;
  }

  .related-item .title {
    font-size: 0.9rem;
    overflow-wrap: anywhere;
  }

  .related-item .sub {
    color: #9aa0a6;
    font-size: 0.8rem;
    margin: 2px 0 6px;
  }

  .related-item .actions {
    display: flex;
    gap: 8px;
    font-size: 0.85rem;
  }

  .related-item .actions .danger {
    color: #ff7b72;
  }

  .subtle {
    font-size: 0.85rem;
  }
</style>
