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
    </div>
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

  .wheel-input {
    width: 80px;
  }
</style>
