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
    <!-- 読み込み中と設定内容の出し分けはリマウントを避けるため
         hidden 切替（ヒットずれ対策、FR-80） -->
    <p class="subtle" class:hidden={!loading}>…</p>
    <div class:hidden={loading}>
      <label class="setting-row">
        <input type="checkbox" bind:checked={pipDefault} />
        {t("settings.pip.default")}
      </label>
      <label class="setting-row">
        <input type="checkbox" bind:checked={pipFitAspect} />
        {t("settings.pip.fitAspect")}
      </label>
      <p class="subtle desc">{t("settings.pip.fitAspect.desc")}</p>
      <label class="setting-row">
        {t("settings.pip.geometry")}
        <input
          type="text"
          class="format-input"
          bind:value={pipGeometry}
          placeholder={PIP_GEOMETRY_DEFAULT}
        />
      </label>
      <label class="setting-row">
        {t("settings.pip.quality")}
        <input
          type="text"
          class="format-input"
          bind:value={pipQuality}
          placeholder="bv*[height<=480]+ba/b[height<=480]"
        />
      </label>
      <p class="subtle desc">{t("settings.pip.quality.desc")}</p>
    </div>
  </section>
