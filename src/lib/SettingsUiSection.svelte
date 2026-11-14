<script lang="ts">
  // 表示設定セクション（FR-76・FR-77、仕様決定 DA・DB）。
  // ui.theme で配色テーマ、ui.density で一覧の表示密度を選ぶ。
  // 保存すると settings://changed 経由で即時に表示へ反映される
  // （src/lib/ui-prefs.ts が data-theme / data-density を更新する）。
  // 既定は theme=dark / density=comfortable（未設定・その他の値も同じ扱い）
  import { t } from "$lib/i18n";

  let {
    loading,
    uiTheme = $bindable(),
    uiDensity = $bindable(),
  }: {
    loading: boolean;
    uiTheme: string;
    uiDensity: string;
  } = $props();
</script>

<section class="panel panel--filled">
  <h2>{t("settings.ui.title")}</h2>
  <p class="subtle desc">{t("settings.ui.desc")}</p>
  {#if loading}
    <p class="subtle">…</p>
  {:else}
    <label class="row">
      {t("settings.ui.theme")}
      <select bind:value={uiTheme}>
        <option value="dark">{t("settings.ui.theme.dark")}</option>
        <option value="light">{t("settings.ui.theme.light")}</option>
        <option value="system">{t("settings.ui.theme.system")}</option>
      </select>
    </label>
    <label class="row">
      {t("settings.ui.density")}
      <select bind:value={uiDensity}>
        <option value="comfortable">
          {t("settings.ui.density.comfortable")}
        </option>
        <option value="compact">{t("settings.ui.density.compact")}</option>
      </select>
    </label>
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
    margin: 12px 0;
    font-size: 0.95rem;
  }
</style>
