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
  {#if loading}
    <p class="subtle">…</p>
  {:else}
    <label class="wheel-row">
      <input type="checkbox" bind:checked={showAuthor} />
      {t("settings.chatpopup.showAuthor")}
    </label>
    <p class="subtle desc">{t("settings.chatpopup.showAuthor.desc")}</p>
    <label class="wheel-row">
      {t("settings.chatpopup.fontSize")}
      <input
        type="text"
        class="num-input"
        bind:value={fontSize}
        placeholder="14"
      />
    </label>
    <p class="subtle desc">{t("settings.chatpopup.fontSize.desc")}</p>
    <label class="wheel-row">
      {t("settings.chatpopup.opacity")}
      <input
        type="text"
        class="num-input"
        bind:value={opacity}
        placeholder="1.0"
      />
    </label>
    <p class="subtle desc">{t("settings.chatpopup.opacity.desc")}</p>
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

  .num-input {
    width: 6em;
    font-family: monospace;
  }

  .wheel-row {
    display: flex;
    align-items: center;
    gap: 10px;
    margin: 12px 0;
    font-size: 0.95rem;
  }
</style>
