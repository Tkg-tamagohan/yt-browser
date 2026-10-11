<script lang="ts">
  import { t } from "$lib/i18n";
  import type { MonitorInfo } from "$lib/monitors";

  let {
    loading,
    monitors,
    windowScreen = $bindable(),
  }: {
    loading: boolean;
    monitors: MonitorInfo[];
    windowScreen: string;
  } = $props();

  // モニター名が取れない環境では名前で選択できないため、選択肢は名前付きのみ
  const named = $derived(monitors.filter((m) => m.name !== null));

  function monitorLabel(m: MonitorInfo): string {
    const name = m.name ?? `#${m.index + 1}`;
    const dims = `${m.width}x${m.height}`;
    if (m.primary) {
      return t("settings.window.monitorPrimary", { name, dims });
    }
    return t("settings.window.monitor", { name, dims });
  }
</script>

<section class="panel panel--filled">
  <h2>{t("settings.window.title")}</h2>
  <p class="subtle desc">{t("settings.window.desc")}</p>
  {#if loading}
    <p class="subtle">…</p>
  {:else}
    <label class="wheel-row">
      {t("settings.window.screen")}
      <select bind:value={windowScreen} disabled={named.length === 0}>
        <option value="">{t("settings.window.screen.auto")}</option>
        {#each named as m}
          <option value={m.name}>{monitorLabel(m)}</option>
        {/each}
        <!-- 保存済みのモニターが現在未接続のときも設定値を見失わないよう残す -->
        {#if windowScreen !== "" && !named.some((m) => m.name === windowScreen)}
          <option value={windowScreen}>
            {t("settings.window.screen.missing", { name: windowScreen })}
          </option>
        {/if}
      </select>
    </label>
    {#if named.length === 0}
      <p class="subtle desc">{t("settings.window.screen.none")}</p>
    {/if}
    <p class="subtle desc">{t("settings.window.screen.desc")}</p>
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

  .wheel-row {
    display: flex;
    align-items: center;
    gap: 10px;
    margin: 12px 0;
    font-size: 0.95rem;
  }
</style>
