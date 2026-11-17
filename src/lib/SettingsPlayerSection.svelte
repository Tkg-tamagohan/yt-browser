<script lang="ts">
  // 再生の初期値および画面遷移設定（FR-39・FR-92、仕様決定 BO・DU）。
  // 初期速度・初期音量は mpv 起動引数へ渡すため次回の再生開始から有効で、
  // 稼働中インスタンスの速度・音量はカード上の変更のみ（セッション限り）。
  // navigateOnPlay は次の再生開始から再生画面（/）へ遷移するかを選ぶ（既定 true）
  import { t } from "$lib/i18n";

  let {
    loading,
    initialSpeed = $bindable(),
    initialVolume = $bindable(),
    navigateOnPlay = $bindable(),
  }: {
    loading: boolean;
    initialSpeed: number | undefined;
    initialVolume: number | undefined;
    navigateOnPlay: boolean;
  } = $props();
</script>

  <section class="panel panel--filled">
    <h2>{t("settings.player.title")}</h2>
    <p class="subtle desc">{t("settings.player.desc")}</p>
    <!-- 読み込み中と設定内容の出し分けはリマウントを避けるため
         hidden 切替（ヒットずれ対策、FR-80） -->
    <p class="subtle" class:hidden={!loading}>…</p>
    <div class:hidden={loading}>
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
      <label class="setting-row">
        <input type="checkbox" bind:checked={navigateOnPlay} />
        {t("settings.player.navigateOnPlay")}
      </label>
    </div>
  </section>

<style>
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
