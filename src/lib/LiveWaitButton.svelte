<script lang="ts">
  // 「配信を待機」行アクション（FR-44、仕様決定 BT）。
  // フィード・検索・関連動画の行に載せる。待機中は強調表示されて
  // 行への「待機中の印」を兼ね、押下で個別解除になる。
  // 登録可否の判定（InnerTube での放送状態確認）はバックエンドの
  // live_wait_add が担い、ここでは結果の通知だけを行う。
  import { t } from "$lib/i18n";
  import {
    isWaiting,
    liveWaitAdd,
    liveWaitRemove,
  } from "$lib/livewait.svelte";
  import type { VideoRef } from "$lib/players.svelte";

  type Props = { video: VideoRef };
  let { video }: Props = $props();

  let waiting = $derived(isWaiting(video.videoId));

  function onclick(): void {
    if (waiting) {
      void liveWaitRemove(video.videoId);
    } else {
      void liveWaitAdd(video);
    }
  }
</script>

<button
  class="icon"
  class:active={waiting}
  title={waiting ? t("livewait.remove") : t("livewait.add")}
  aria-label={waiting ? t("livewait.waiting") : t("livewait.add")}
  {onclick}
>
  {waiting ? t("livewait.waiting") : "待"}
</button>

<style>
  .icon {
    padding: 2px 6px;
    border: none;
    background: none;
    color: #9aa0a6;
    font-size: 0.8rem;
    line-height: 1.6;
    cursor: pointer;
    white-space: nowrap;
  }

  .icon:hover,
  .icon.active {
    color: #8ab4f8;
  }
</style>
