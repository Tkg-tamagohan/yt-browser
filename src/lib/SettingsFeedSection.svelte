<script lang="ts">
  import { t } from "$lib/i18n";

  let {
    loading,
    feedShowShorts = $bindable(),
  }: {
    loading: boolean;
    feedShowShorts: boolean;
  } = $props();
</script>

  <section class="panel panel--filled">
    <h2>{t("settings.feed.title")}</h2>
    <p class="subtle desc">{t("settings.feed.desc")}</p>
    {#if loading}
      <p class="subtle">…</p>
    {:else}
      <label class="wheel-row">
        <input type="checkbox" bind:checked={feedShowShorts} />
        {t("settings.feed.showShorts")}
      </label>
      <p class="subtle desc">{t("settings.feed.showShorts.desc")}</p>
    {/if}
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

  .wheel-row {
    display: flex;
    align-items: center;
    gap: 10px;
    margin: 12px 0;
    font-size: 0.95rem;
  }
</style>
