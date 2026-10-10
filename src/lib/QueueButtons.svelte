<script lang="ts">
  // キュースタック行アクションの共有部品（FR-20、仕様決定 AM）:
  // 「キューに追加」（末尾）と「次に再生」（実行中は現在項目の直後、
  // 未実行は先頭）。追加は再生を開始せず積み上げるだけ。
  import { t } from "$lib/i18n";
  import { notify } from "$lib/notices.svelte";
  import { queueAdd, queuePlayNext } from "$lib/queue.svelte";
  import type { VideoRef } from "$lib/players.svelte";

  type Props = { video: VideoRef };
  let { video }: Props = $props();

  function add(): void {
    queueAdd(video);
    notify(t("queue.added"));
  }

  function next(): void {
    queuePlayNext(video);
    notify(t("queue.addedNext"));
  }
</script>

<button
  class="icon"
  title={t("queue.add")}
  aria-label={t("queue.add")}
  onclick={add}>Q＋</button
>
<button
  class="icon"
  title={t("queue.playNext")}
  aria-label={t("queue.playNext")}
  onclick={next}>Q▶</button
>

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

  .icon:hover {
    color: #8ab4f8;
  }
</style>
