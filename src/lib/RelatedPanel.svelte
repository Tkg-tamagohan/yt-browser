<script lang="ts">
  // 関連動画パネル。開くたびに get_related を取り直す
  // （ブロック状態の変化と前回失敗の再試行に対応するためキャッシュしない）。
  //
  // リマウント回避（webkit2gtk のヒットずれ対策、FR-80・仕様決定 DE）で
  // 常時マウントされ、表示はカード側がラッパの hidden で切り替える。
  // 「開くとマウントされる」に乗っていた初回取得は、open prop の
  // 立ち上がりを見る $effect（panelFetchKey 駆動）へ移した
  import { page } from "$app/state";
  import { onDestroy } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { t } from "$lib/i18n";
  import { notify } from "$lib/notices.svelte";
  import VideoActions from "$lib/VideoActions.svelte";
  import VideoRow from "$lib/VideoRow.svelte";
  import { panelFetchKey } from "$lib/remount-logic";
  import {
    createVideoActionState,
    videoRefOf,
  } from "$lib/video-actions.svelte";
  import {
    asErrorMessage,
    type SearchResult,
  } from "$lib/players.svelte";

  // instanceId はカード側がパネルをインスタンスへ対応づけるためのキー。
  // open はカード側の開閉状態（hidden 切替と同じ値）
  let {
    instanceId,
    videoId,
    open,
  }: { instanceId: number; videoId: string; open: boolean } = $props();

  let loading = $state(true);
  let items = $state<SearchResult[]>([]);
  // アンマウント後に届く非同期応答を識別するためのフラグ
  let disposed = false;
  // 閉じ→開き直しで取り直しが並走するため、発行した世代の応答だけを
  // 反映する（DescPanel と同じ方式）
  let generation = 0;

  // お気に入り・プレイリスト行アクション用（FR-7）。
  // 他ページ（ライブラリ等）でも編集されるため、パネルを開くたびに再取得する
  // （従来はページのリマウントと「/」復帰が暗黙の再取得トリガーだった）。
  // resyncOnChange で、ローカル編集の表示即時反映のあと DB の truth に収束させる
  // （コールバックは書き込みコミット後に発火するため、再取得は編集済みの値を含む）。
  const va = createVideoActionState({ resyncOnChange: true });

  async function loadRelated(gen: number): Promise<void> {
    loading = true;
    try {
      const res = await invoke<SearchResult[]>("get_related", { videoId });
      if (disposed || gen !== generation) return;
      items = res;
    } catch (e) {
      if (disposed || gen !== generation) return;
      items = [];
      // パネルが閉じられたあとも invoke の応答は届くため、
      // 見えていない結果に対して失敗通知を出さない
      if (open) {
        notify(t("related.failed", { message: asErrorMessage(e) }));
      }
    } finally {
      if (!disposed && gen === generation) loading = false;
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

  // 「開くたびに取り直す」は、常時マウント化で onMount が使えなくなった
  // ため open の立ち上がり（取得キーの変化）で再現する（FR-80）。
  // 固定キーなので、開いている間に再生中動画が切り替わっても取り直さない
  // （従来はマウント時の videoId でしか取らなかった挙動を維持する）
  let lastKey: string | null = null;
  $effect(() => {
    const key = panelFetchKey(open, "open");
    if (key === lastKey) return;
    lastKey = key;
    if (key === null) return;
    const gen = ++generation;
    void loadRelated(gen);
  });

  // 開いている間も他ページ（ライブラリ等）での編集を取り込めるよう、
  // 「/」へ戻るたびに再取得する（分割前の暗黙リフレッシュ契機を維持）。
  // 常時マウントなので open で絞り、閉じているカードでは走らせない
  // （開いたときの初回取得もこのエフェクトが担う）
  $effect(() => {
    if (open && page.url.pathname === "/") void va.refresh();
  });

  onDestroy(() => {
    disposed = true;
  });
</script>

<div class="related sub-panel">
  <h3>{t("related.title")}</h3>
  <!-- 読み込み中・空・一覧の出し分けはリマウントを避けるため
       hidden 切替とする（ヒットずれ対策、FR-80） -->
  <p class="subtle" class:hidden={!loading}>{t("related.loading")}</p>
  <p class="subtle" class:hidden={loading || items.length > 0}>
    {t("related.empty")}
  </p>
  <ul class="related-list" class:hidden={loading || items.length === 0}>
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
            <button
              class="danger"
              class:hidden={!r.channelId}
              onclick={() => blockRelated(r)}
            >
              {t("search.block")}
            </button>
            <VideoActions
              video={videoRefOf(r)}
              faved={va.favIds.has(r.videoId)}
              playlists={va.playlists}
              watchLater={va.watchLaterIds.has(r.videoId)}
              livewait
              onfavchange={va.onFavChange}
              onplaylistcreated={va.onPlaylistCreated}
              onwatchlaterchange={va.onWatchLaterChange}
            />
          {/snippet}
        </VideoRow>
      {/each}
  </ul>
</div>

<style>
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
