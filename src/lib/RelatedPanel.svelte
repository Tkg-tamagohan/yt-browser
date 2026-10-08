<script lang="ts">
  // 関連動画パネル。開いている間だけマウントされ、開くたびに get_related を
  // 取り直す（ブロック状態の変化と前回失敗の再試行に対応するためキャッシュしない）。
  import { page } from "$app/state";
  import { onDestroy, onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { t } from "$lib/i18n";
  import { notify } from "$lib/notices.svelte";
  import VideoActions from "$lib/VideoActions.svelte";
  import VideoRow from "$lib/VideoRow.svelte";
  import {
    createVideoActionState,
    videoRefOf,
  } from "$lib/video-actions.svelte";
  import {
    asErrorMessage,
    type SearchResult,
  } from "$lib/players.svelte";

  // instanceId はカード側がパネルをインスタンスへ対応づけるためのキー
  let { instanceId, videoId }: { instanceId: number; videoId: string } =
    $props();

  let loading = $state(true);
  let items = $state<SearchResult[]>([]);
  // アンマウント後に届く非同期応答を識別するためのフラグ
  let disposed = false;

  // お気に入り・プレイリスト行アクション用（FR-7）。
  // 他ページ（ライブラリ等）でも編集されるため、パネルを開くたびに再取得する
  // （従来はページのリマウントと「/」復帰が暗黙の再取得トリガーだった）。
  // resyncOnChange で、ローカル編集の表示即時反映のあと DB の truth に収束させる
  // （コールバックは書き込みコミット後に発火するため、再取得は編集済みの値を含む）。
  const va = createVideoActionState({ resyncOnChange: true });

  async function loadRelated(): Promise<void> {
    loading = true;
    try {
      items = await invoke<SearchResult[]>("get_related", { videoId });
    } catch (e) {
      items = [];
      // パネルが閉じられてアンマウントされたあとも invoke の応答は届くため、
      // 見えていない結果に対して失敗通知を出さない
      if (!disposed) {
        notify(t("related.failed", { message: asErrorMessage(e) }));
      }
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

  onMount(() => {
    void loadRelated();
  });

  // 開いている間も他ページ（ライブラリ等）での編集を取り込めるよう、
  // 「/」へ戻るたびに再取得する（分割前の暗黙リフレッシュ契機を維持）
  $effect(() => {
    if (page.url.pathname === "/") void va.refresh();
  });

  onDestroy(() => {
    disposed = true;
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
        <VideoRow
          videoId={r.videoId}
          title={r.title}
          thumbnailUrl={r.thumbnailUrl}
          dense={true}
          onplay={() => playRelated(r)}
        >
          {#snippet sub()}{r.channelTitle ?? ""}{/snippet}
          {#snippet actions()}
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
              faved={va.favIds.has(r.videoId)}
              playlists={va.playlists}
              onfavchange={va.onFavChange}
              onplaylistcreated={va.onPlaylistCreated}
            />
          {/snippet}
        </VideoRow>
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

  .subtle {
    font-size: 0.85rem;
  }
</style>
