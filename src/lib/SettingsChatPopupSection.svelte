<script lang="ts">
  // 設定画面の「チャットポップアップ」節（FR-68、仕様決定 CS）。
  // chat.popup_show_author / chat.popup_font_size / chat.popup_opacity
  // を編集する。開いているポップアップ窓へは settings://changed 経由で
  // 即時に反映される
  import { t } from "$lib/i18n";

  let {
    loading,
    showAuthor = $bindable(),
    fontSize = $bindable(),
    opacity = $bindable(),
  }: {
    loading: boolean;
    showAuthor: boolean;
    fontSize: string;
    opacity: string;
  } = $props();
</script>

<section class="panel panel--filled">
  <h2>{t("settings.chatpopup.title")}</h2>
  <p class="subtle desc">{t("settings.chatpopup.desc")}</p>
  <!-- 読み込み中と設定内容の出し分けはリマウントを避けるため
       hidden 切替（ヒットずれ対策、FR-80） -->
  <p class="subtle" class:hidden={!loading}>…</p>
  <div class:hidden={loading}>
    <label class="setting-row">
      <input type="checkbox" bind:checked={showAuthor} />
      {t("settings.chatpopup.showAuthor")}
    </label>
    <p class="subtle desc">{t("settings.chatpopup.showAuthor.desc")}</p>
    <label class="setting-row">
      {t("settings.chatpopup.fontSize")}
      <input
        type="text"
        class="num-input"
        bind:value={fontSize}
        placeholder="14"
      />
    </label>
    <p class="subtle desc">{t("settings.chatpopup.fontSize.desc")}</p>
    <label class="setting-row">
      {t("settings.chatpopup.opacity")}
      <input
        type="text"
        class="num-input"
        bind:value={opacity}
        placeholder="1.0"
      />
    </label>
    <p class="subtle desc">{t("settings.chatpopup.opacity.desc")}</p>
  </div>
</section>

<style>
  .num-input {
    width: 6em;
    font-family: monospace;
  }
</style>
