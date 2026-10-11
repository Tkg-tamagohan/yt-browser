<script lang="ts">
  // 概要欄パネル（FR-33、仕様決定 BB）。開いている間だけマウントされ、
  // 開くたびと再生中動画の切替（連続再生・ループ遷移）で get_description を
  // 取り直す（結果はキャッシュしない）。
  // 本文は改行を保持して表示し、http(s) URL は既定ブラウザで開き、
  // mm:ss / h:mm:ss の時刻表記はそのインスタンスへシークする。
  import { onDestroy } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { t } from "$lib/i18n";
  import { notify } from "$lib/notices.svelte";
  import { parseDescription } from "$lib/desc-logic";
  import { asErrorMessage } from "$lib/players.svelte";

  // instanceId はカード側がパネルをインスタンスへ対応づけるためのキー
  let { instanceId, videoId }: { instanceId: number; videoId: string } =
    $props();

  let loading = $state(true);
  let text = $state("");
  // 失敗は通知とパネル内表示の双方で扱う。message を保持してパネルに出す
  let failed = $state<string | null>(null);
  // アンマウント後に届く非同期応答を識別するためのフラグ
  let disposed = false;
  // 動画切替の取り直しが並走するため、発行した世代の応答だけを反映する
  let generation = 0;

  async function loadDescription(vid: string, gen: number): Promise<void> {
    loading = true;
    failed = null;
    try {
      const d = await invoke<string>("get_description", { videoId: vid });
      if (disposed || gen !== generation) return;
      text = d;
    } catch (e) {
      // パネルが閉じられてアンマウントされたあとも invoke の応答は届くため、
      // 見えていない結果に対して失敗通知を出さない
      if (disposed || gen !== generation) return;
      const message = asErrorMessage(e);
      text = "";
      failed = message;
      notify(t("desc.failed", { message }));
    } finally {
      if (!disposed && gen === generation) loading = false;
    }
  }

  // パネルを開くたび（マウント）と、連続再生・ループで再生中動画が
  // 変わるたびに取り直す
  $effect(() => {
    const gen = ++generation;
    void loadDescription(videoId, gen);
  });

  async function seek(seconds: number): Promise<void> {
    try {
      await invoke("player_control", {
        instanceId,
        action: { type: "seek", seconds },
      });
    } catch (e) {
      notify(t("player.error", { message: asErrorMessage(e) }));
    }
  }

  async function open(url: string): Promise<void> {
    try {
      await openUrl(url);
    } catch (e) {
      notify(t("player.error", { message: asErrorMessage(e) }));
    }
  }

  onDestroy(() => {
    disposed = true;
  });
</script>

<div class="desc sub-panel">
  <h3>{t("desc.title")}</h3>
  {#if loading}
    <p class="subtle">{t("desc.loading")}</p>
  {:else if failed !== null}
    <p class="subtle">{t("desc.failed", { message: failed })}</p>
  {:else if text.trim() === ""}
    <p class="subtle">{t("desc.empty")}</p>
  {:else}
    <p class="desc-body">
      {#each parseDescription(text) as seg}
        {#if seg.kind === "url"}
          <button class="link" onclick={() => void open(seg.url)}>
            {seg.text}
          </button>
        {:else if seg.kind === "seek"}
          <button class="link" onclick={() => void seek(seg.seconds)}>
            {seg.text}
          </button>
        {:else}{seg.text}{/if}
      {/each}
    </p>
  {/if}
</div>

<style>
  .desc h3 {
    font-size: 1rem;
    margin: 0 0 8px;
  }

  .desc-body {
    margin: 0;
    max-height: 320px;
    overflow-y: auto;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    font-size: 0.9rem;
  }

  /* 本文中のリンク・時刻はインラインのボタンとして流し込む */
  .desc-body .link {
    display: inline;
    padding: 0;
    border: none;
    background: none;
    color: #8ab4f8;
    font: inherit;
    text-align: left;
    cursor: pointer;
    overflow-wrap: anywhere;
  }

  .desc-body .link:hover {
    text-decoration: underline;
  }

  .subtle {
    font-size: 0.85rem;
  }
</style>
