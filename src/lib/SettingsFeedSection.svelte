<script lang="ts">
  import { t } from "$lib/i18n";

  let {
    loading,
    feedShowShorts = $bindable(),
    feedNotifyNew = $bindable(),
    feedAutoReadPlayed = $bindable(),
    feedAutoReadDays = $bindable(),
  }: {
    loading: boolean;
    feedShowShorts: boolean;
    feedNotifyNew: boolean;
    feedAutoReadPlayed: boolean;
    feedAutoReadDays: number | undefined;
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
      <label class="wheel-row">
        <input type="checkbox" bind:checked={feedNotifyNew} />
        {t("settings.feed.notifyNew")}
      </label>
      <p class="subtle desc">{t("settings.feed.notifyNew.desc")}</p>
      <label class="wheel-row">
        <input type="checkbox" bind:checked={feedAutoReadPlayed} />
        {t("settings.feed.autoReadPlayed")}
      </label>
      <p class="subtle desc">{t("settings.feed.autoReadPlayed.desc")}</p>
      <label class="wheel-row">
        {t("settings.feed.autoReadDays")}
        <input
          type="number"
          class="days-input"
          bind:value={feedAutoReadDays}
          min="0"
          step="1"
        />
      </label>
      <p class="subtle desc">{t("settings.feed.autoReadDays.desc")}</p>
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

  .days-input {
    width: 80px;
  }
</style>
