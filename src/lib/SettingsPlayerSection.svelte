<script lang="ts">
  // 再生の初期値（FR-39、仕様決定 BO）。mpv 起動引数へ渡すため
  // 変更は次回の再生開始から有効で、稼働中インスタンスの
  // 速度・音量はカード上の変更のみ（セッション限り）。
  // 値は文字列入力として保持し、保存時に範囲検証する
  import { t } from "$lib/i18n";

  let {
    loading,
    initialSpeed = $bindable(),
    initialVolume = $bindable(),
  }: {
    loading: boolean;
    initialSpeed: string;
    initialVolume: string;
  } = $props();
</script>

  <section class="panel panel--filled">
    <h2>{t("settings.player.title")}</h2>
    <p class="subtle desc">{t("settings.player.desc")}</p>
    {#if loading}
      <p class="subtle">…</p>
    {:else}
      <label class="row">
        {t("settings.player.initialSpeed")}
        <input
          type="number"
          class="num-input"
          bind:value={initialSpeed}
          min="0.25"
          max="4"
          step="0.25"
          placeholder="1.0"
        />
      </label>
      <p class="subtle field-desc">{t("settings.player.initialSpeed.desc")}</p>
      <label class="row">
        {t("settings.player.initialVolume")}
        <input
          type="number"
          class="num-input"
          bind:value={initialVolume}
          min="0"
          max="130"
          step="1"
          placeholder="100"
        />
      </label>
      <p class="subtle field-desc">{t("settings.player.initialVolume.desc")}</p>
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

  .row {
    display: flex;
    align-items: center;
    gap: 10px;
    margin: 12px 0 2px;
    font-size: 0.95rem;
  }

  .num-input {
    width: 80px;
  }

  .field-desc {
    font-size: 0.85rem;
    margin: 0 0 4px;
  }
</style>
