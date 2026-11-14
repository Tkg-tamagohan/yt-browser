<script lang="ts">
  // 概要欄パネル（FR-33、仕様決定 BB）。開くたびと再生中動画の切替
  // （連続再生・ループ遷移）で get_description を取り直す
  // （結果はキャッシュしない）。
  // 本文は改行を保持して表示し、http(s) URL は既定ブラウザで開き、
  // mm:ss / h:mm:ss の時刻表記はそのインスタンスへシークする。
  //
  // リマウント回避（webkit2gtk のヒットずれ対策、FR-80・仕様決定 DE）で
  // 常時マウントされ、表示はカード側がラッパの hidden で切り替える。
  // そのため「開くとマウントされる」に乗っていた初回取得は、open prop の
  // 立ち上がりを見る $effect（panelFetchKey 駆動）へ移した
  import { onDestroy } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { t } from "$lib/i18n";
  import { notify } from "$lib/notices.svelte";
  import { parseDescription } from "$lib/desc-logic";
  import { panelFetchKey } from "$lib/remount-logic";
  import { asErrorMessage } from "$lib/players.svelte";

  // instanceId はカード側がパネルをインスタンスへ対応づけるためのキー。
  // open はカード側の開閉状態（hidden 切替と同じ値）
  let {
    instanceId,
    videoId,
    open,
  }: { instanceId: number; videoId: string; open: boolean } = $props();

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
      // パネルが閉じられたあとも invoke の応答は届くため、
      // 見えていない結果に対して失敗通知を出さない
      if (disposed || gen !== generation || !open) return;
      const message = asErrorMessage(e);
      text = "";
      failed = message;
      notify(t("desc.failed", { message }));
    } finally {
      if (!disposed && gen === generation) loading = false;
    }
  }

  // パネルを開くたびと、開いている間に連続再生・ループで再生中動画が
  // 変わるたびに取り直す。常時マウントなので「開くとマウントされる」に
  // 乗っていた初回取得は、open が立ち上がったときを含めた
  // 「取得キーの変化」として扱う（remount-logic.ts、FR-80）。
  // videoId はカードの状態ポーリングで値が同じでも prop の再評価が走り
  // $effect が繰り返し発火するため、前回値と比較して変化時だけ取り直す
  // （取り直しループで読み込み表示と本文が往復する振動の防止）
  let lastKey: string | null = null;
  $effect(() => {
    const key = panelFetchKey(open, videoId);
    if (key === lastKey) return;
    lastKey = key;
    if (key === null) return;
    const gen = ++generation;
    void loadDescription(key, gen);
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

  // open prop と名前が衝突するため openLink とする
  async function openLink(url: string): Promise<void> {
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
  <!-- 読み込み中・失敗・空・本文の出し分けはリマウントを避けるため
       hidden 切替とする（ヒットずれ対策、FR-80） -->
  <p class="subtle" class:hidden={!loading}>{t("desc.loading")}</p>
  <p class="subtle" class:hidden={loading || failed === null}>
    {t("desc.failed", { message: failed ?? "" })}
  </p>
  <p
    class="subtle"
    class:hidden={loading || failed !== null || text.trim() !== ""}
  >
    {t("desc.empty")}
  </p>
  <p
    class="desc-body"
    class:hidden={loading || failed !== null || text.trim() === ""}
  >
    {#each parseDescription(text) as seg}
      {#if seg.kind === "url"}
        <button class="link" onclick={() => void openLink(seg.url)}>
          {seg.text}
        </button>
      {:else if seg.kind === "seek"}
        <button class="link" onclick={() => void seek(seg.seconds)}>
          {seg.text}
        </button>
      {:else}{seg.text}{/if}
    {/each}
  </p>
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
    color: var(--accent);
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
