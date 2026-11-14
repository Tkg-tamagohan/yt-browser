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

  <section class="panel panel--filled">
    <h2>{t("settings.wheel.title")}</h2>
    <p class="subtle desc">{t("settings.wheel.desc")}</p>
    <!-- 読み込み中と設定内容の出し分けはリマウントを避けるため
         hidden 切替（ヒットずれ対策、FR-80） -->
    <p class="subtle" class:hidden={!loading}>…</p>
    <div class:hidden={loading}>
      <label class="setting-row">
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
    </div>
  </section>

<style>
  .wheel-input {
    width: 80px;
  }
</style>
