<script lang="ts">
  import { t } from "$lib/i18n";

  let {
    loading,
    wheelDelta = $bindable(),
  }: {
    loading: boolean;
    wheelDelta: number | undefined;
  } = $props();
</script>

  <section class="panel">
    <h2>{t("settings.wheel.title")}</h2>
    <p class="subtle desc">{t("settings.wheel.desc")}</p>
    {#if loading}
      <p class="subtle">…</p>
    {:else}
      <label class="wheel-row">
        {t("settings.wheel.volumeDelta")}
        <input
          type="number"
          class="wheel-input"
          bind:value={wheelDelta}
          min="-100"
          max="100"
          step="1"
        />
      </label>
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

  .wheel-row {
    display: flex;
    align-items: center;
    gap: 10px;
    margin: 12px 0;
    font-size: 0.95rem;
  }

  .wheel-input {
    width: 80px;
  }
</style>
