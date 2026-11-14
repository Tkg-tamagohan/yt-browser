<script lang="ts">
  // キーボードショートカット節（FR-79、仕様決定 DD）。
  // `ui.shortcuts` で既定キーマップを有効化する。既定は off（暫定解釈）。
  // 値の解釈とキーマップの内訳は shortcuts-logic.ts が持つ
  import { t } from "$lib/i18n";

  let {
    loading,
    shortcutsEnabled = $bindable(),
  }: {
    loading: boolean;
    shortcutsEnabled: boolean;
  } = $props();
</script>

<section class="panel panel--filled">
  <h2>{t("settings.shortcuts.title")}</h2>
  <p class="subtle desc">{t("settings.shortcuts.desc")}</p>
  <!-- 読み込み中と設定内容の出し分けはリマウントを避けるため
       hidden 切替（ヒットずれ対策、FR-80） -->
  <p class="subtle" class:hidden={!loading}>…</p>
  <div class:hidden={loading}>
    <label class="setting-row">
      <input type="checkbox" bind:checked={shortcutsEnabled} />
      {t("settings.shortcuts.enabled")}
    </label>
    <p class="subtle desc">{t("settings.shortcuts.enabled.desc")}</p>
  </div>
</section>
