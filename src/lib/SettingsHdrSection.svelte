<script lang="ts">
  import { t, TONE_MAPPING_MPV } from "$lib/i18n";
  import { TONE_MAPPINGS } from "$lib/settings-consts";

  let {
    loading,
    toneMapping = $bindable(),
    computePeak = $bindable(),
    mpvExtraArgs = $bindable(),
  }: {
    loading: boolean;
    toneMapping: string;
    computePeak: string;
    mpvExtraArgs: string;
  } = $props();
</script>

  <section class="panel">
    <h2>{t("settings.hdr.title")}</h2>
    <p class="subtle desc">{t("settings.hdr.desc")}</p>
    {#if loading}
      <p class="subtle">…</p>
    {:else}
      <label class="wheel-row">
        {t("settings.hdr.toneMapping")}
        <select bind:value={toneMapping}>
          {#each TONE_MAPPINGS as m}
            <option value={m}>
              {m === "auto" ? t("settings.hdr.auto") : (TONE_MAPPING_MPV[m] ?? m)}
            </option>
          {/each}
        </select>
      </label>
      <label class="wheel-row">
        {t("settings.hdr.computePeak")}
        <select bind:value={computePeak}>
          <option value="auto">{t("settings.hdr.auto")}</option>
          <option value="yes">{t("settings.hdr.yes")}</option>
          <option value="no">{t("settings.hdr.no")}</option>
        </select>
      </label>
      <label class="wheel-row">
        {t("settings.hdr.extraArgs")}
        <input
          type="text"
          class="format-input"
          bind:value={mpvExtraArgs}
          placeholder="--target-colorspace-hint=yes"
        />
      </label>
      <p class="subtle desc">{t("settings.hdr.extraArgs.desc")}</p>
    {/if}
  </section>

<style>
  .panel {
    padding: 16px;
    border: 1px solid #3c4043;
    border-radius: 12px;
    background: #202124;
  }

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
