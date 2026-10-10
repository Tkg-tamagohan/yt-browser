<script lang="ts">
  import { t } from "$lib/i18n";
  import { QUALITY_PRESETS as PRESETS } from "$lib/quality";
  import { CUSTOM } from "$lib/settings-consts";

  let {
    loading,
    selected = $bindable(),
    customFormat = $bindable(),
  }: {
    loading: boolean;
    selected: string;
    customFormat: string;
  } = $props();
</script>

  <section class="panel panel--filled">
    <h2>{t("settings.quality.title")}</h2>
    <p class="subtle desc">{t("settings.quality.desc")}</p>
    {#if loading}
      <p class="subtle">…</p>
    {:else}
      <div class="presets">
        {#each PRESETS as p}
          <label>
            <input type="radio" bind:group={selected} value={p.format} />
            {t(p.key)}
            <code>{p.format}</code>
          </label>
        {/each}
        <label>
          <input type="radio" bind:group={selected} value={CUSTOM} />
          {t("settings.quality.custom")}
        </label>
        {#if selected === CUSTOM}
          <input
            type="text"
            class="format-input"
            bind:value={customFormat}
            placeholder={t("settings.quality.format.label")}
          />
        {/if}
      </div>
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

  .presets {
    display: flex;
    flex-direction: column;
    gap: 10px;
    margin: 12px 0 16px;
  }

  .presets label {
    display: flex;
    gap: 8px;
    align-items: baseline;
    font-size: 0.95rem;
  }

  .presets code {
    color: #9aa0a6;
    font-size: 0.8rem;
    overflow-wrap: anywhere;
  }

  .format-input {
    width: 100%;
    margin-top: 4px;
    font-family: monospace;
  }
</style>
