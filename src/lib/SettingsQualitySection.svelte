<script lang="ts">
  import { t } from "$lib/i18n";
  import { QUALITY_PRESETS as PRESETS } from "$lib/quality";
  import { CUSTOM } from "$lib/settings-consts";

  let {
    loading,
    selected = $bindable(),
    customFormat = $bindable(),
  }: {
    loading: boolean;
    selected: string;
    customFormat: string;
  } = $props();
</script>

  <section class="panel panel--filled">
    <h2>{t("settings.quality.title")}</h2>
    <p class="subtle desc">{t("settings.quality.desc")}</p>
    <!-- 読み込み中と設定内容の出し分けはリマウントを避けるため
         hidden 切替（ヒットずれ対策、FR-80） -->
    <p class="subtle" class:hidden={!loading}>…</p>
    <div class:hidden={loading}>
      <div class="presets">
        {#each PRESETS as p}
          <label>
            <input type="radio" bind:group={selected} value={p.format} />
            {t(p.key)}
            <code>{p.format}</code>
          </label>
        {/each}
        <label>
          <input type="radio" bind:group={selected} value={CUSTOM} />
          {t("settings.quality.custom")}
        </label>
        <!-- カスタム選択時のフォーマット入力欄も hidden 切替（FR-80） -->
        <input
          type="text"
          class="format-input"
          class:hidden={selected !== CUSTOM}
          bind:value={customFormat}
          placeholder={t("settings.quality.format.label")}
        />
      </div>
    </div>
  </section>

<style>
  .presets {
    display: flex;
    flex-direction: column;
    gap: 10px;
    margin: 12px 0 16px;
  }

  .presets label {
    display: flex;
    gap: 8px;
    align-items: baseline;
    font-size: 0.95rem;
  }

  .presets code {
    color: var(--fg-muted);
    font-size: 0.8rem;
    overflow-wrap: anywhere;
  }
</style>
