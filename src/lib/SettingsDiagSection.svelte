<script lang="ts">
  // 診断節（FR-74、仕様決定 CY）。mpv・yt-dlp・deno の検出状況・
  // バージョン・解決パスの一覧を表示する（一覧本体は DepsDiagList）。
  import { t } from "$lib/i18n";
  import DepsDiagList from "$lib/DepsDiagList.svelte";

  let {
    notify,
  }: {
    notify: (msg: string) => void;
  } = $props();
</script>

  <section class="panel panel--filled">
    <h2>{t("settings.diag.title")}</h2>
    <p class="subtle desc">{t("settings.diag.desc")}</p>
    <DepsDiagList {notify} />
  </section>

<style>
  h2 {
    font-size: 1.1rem;
    margin: 0 0 4px;
  }

  .desc {
    font-size: 0.9rem;
    margin-top: 0;
  }
</style>
