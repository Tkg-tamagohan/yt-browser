<script lang="ts">
  // 表示設定セクション（FR-76・FR-77・FR-81、仕様決定 DA・DB・DF）。
  // ui.theme で配色テーマ、ui.density で一覧の表示密度、
  // ui.language で UI 言語を選ぶ。
  // 保存すると settings://changed 経由で即時に表示へ反映される
  // （src/lib/ui-prefs.ts が data-theme / data-density / lang と
  // i18n の現行ロケールを更新する）。
  // 既定は theme=dark / density=comfortable / language=ja
  // （未設定・その他の値も同じ扱い）
  import { t } from "$lib/i18n";

  let {
    loading,
    uiTheme = $bindable(),
    uiDensity = $bindable(),
    uiLanguage = $bindable(),
  }: {
    loading: boolean;
    uiTheme: string;
    uiDensity: string;
    uiLanguage: string;
  } = $props();
</script>

<section class="panel panel--filled">
  <h2>{t("settings.ui.title")}</h2>
  <p class="subtle desc">{t("settings.ui.desc")}</p>
  <!-- 読み込み中と設定内容の出し分けはリマウントを避けるため
       hidden 切替（ヒットずれ対策、FR-80） -->
  <p class="subtle" class:hidden={!loading}>…</p>
  <div class:hidden={loading}>
    <label class="setting-row">
      {t("settings.ui.theme")}
      <select bind:value={uiTheme}>
        <option value="dark">{t("settings.ui.theme.dark")}</option>
        <option value="light">{t("settings.ui.theme.light")}</option>
        <option value="system">{t("settings.ui.theme.system")}</option>
      </select>
    </label>
    <label class="setting-row">
      {t("settings.ui.density")}
      <select bind:value={uiDensity}>
        <option value="comfortable">
          {t("settings.ui.density.comfortable")}
        </option>
        <option value="compact">{t("settings.ui.density.compact")}</option>
      </select>
    </label>
    <label class="setting-row">
      {t("settings.ui.language")}
      <select bind:value={uiLanguage}>
        <option value="ja">{t("settings.ui.language.ja")}</option>
        <option value="en">{t("settings.ui.language.en")}</option>
      </select>
    </label>
  </div>
</section>
