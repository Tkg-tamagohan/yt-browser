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
    <!-- 読み込み中と設定内容の出し分けはリマウントを避けるため
         hidden 切替（ヒットずれ対策、FR-80） -->
    <p class="subtle" class:hidden={!loading}>…</p>
    <div class:hidden={loading}>
      <label class="setting-row">
        <input type="checkbox" bind:checked={feedShowShorts} />
        {t("settings.feed.showShorts")}
      </label>
      <p class="subtle desc">{t("settings.feed.showShorts.desc")}</p>
      <label class="setting-row">
        <input type="checkbox" bind:checked={feedNotifyNew} />
        {t("settings.feed.notifyNew")}
      </label>
      <label class="setting-row">
        <input type="checkbox" bind:checked={feedAutoReadPlayed} />
        {t("settings.feed.autoReadPlayed")}
      </label>
      <p class="subtle desc">{t("settings.feed.autoReadPlayed.desc")}</p>
      <label class="setting-row">
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
    </div>
  </section>

<style>
  .days-input {
    width: 80px;
  }
</style>
