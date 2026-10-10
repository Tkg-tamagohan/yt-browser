<script lang="ts">
  import { t } from "$lib/i18n";
  import { PIP_GEOMETRY_DEFAULT } from "$lib/pip";

  let {
    loading,
    pipDefault = $bindable(),
    pipFitAspect = $bindable(),
    pipGeometry = $bindable(),
    pipQuality = $bindable(),
  }: {
    loading: boolean;
    pipDefault: boolean;
    pipFitAspect: boolean;
    pipGeometry: string;
    pipQuality: string;
  } = $props();
</script>

  <section class="panel panel--filled">
    <h2>{t("settings.pip.title")}</h2>
    <p class="subtle desc">{t("settings.pip.desc")}</p>
    {#if loading}
      <p class="subtle">…</p>
    {:else}
      <label class="wheel-row">
        <input type="checkbox" bind:checked={pipDefault} />
        {t("settings.pip.default")}
      </label>
      <p class="subtle desc">{t("settings.pip.default.desc")}</p>
      <label class="wheel-row">
        <input type="checkbox" bind:checked={pipFitAspect} />
        {t("settings.pip.fitAspect")}
      </label>
      <p class="subtle desc">{t("settings.pip.fitAspect.desc")}</p>
      <label class="wheel-row">
        {t("settings.pip.geometry")}
        <input
          type="text"
          class="format-input"
          bind:value={pipGeometry}
          placeholder={PIP_GEOMETRY_DEFAULT}
        />
      </label>
      <label class="wheel-row">
        {t("settings.pip.quality")}
        <input
          type="text"
          class="format-input"
          bind:value={pipQuality}
          placeholder="bv*[height<=480]+ba/b[height<=480]"
        />
      </label>
      <p class="subtle desc">{t("settings.pip.quality.desc")}</p>
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

  .format-input {
    width: 100%;
    margin-top: 4px;
    font-family: monospace;
  }

  .wheel-row {
    display: flex;
    align-items: center;
    gap: 10px;
    margin: 12px 0;
    font-size: 0.95rem;
  }
</style>
