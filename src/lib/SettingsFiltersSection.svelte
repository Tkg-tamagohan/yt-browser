<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { t, type MessageKey } from "$lib/i18n";
  import { asErrorMessage, type Filter } from "$lib/players.svelte";
  import { FILTER_TARGETS, FILTER_KINDS } from "$lib/settings-consts";

  let { notify }: { notify: (msg: string) => void } = $props();

  // NG フィルタ（FR-9）
  let filters = $state<Filter[]>([]);
  let fTarget = $state<string>("chat_text");
  let fKind = $state<string>("literal");
  let fPattern = $state("");

  async function loadFilters(): Promise<void> {
    try {
      filters = await invoke<Filter[]>("filter_list");
    } catch {
      filters = [];
    }
  }

  async function addFilter(): Promise<void> {
    const pattern = fPattern.trim();
    if (!pattern) return;
    try {
      await invoke("filter_add", {
        target: fTarget,
        kind: fKind,
        pattern,
      });
      fPattern = "";
      await loadFilters();
      notify(t("settings.filters.added"));
    } catch (e) {
      notify(t("settings.filters.addFailed", { message: asErrorMessage(e) }));
    }
  }

  async function removeFilter(f: Filter): Promise<void> {
    try {
      await invoke("filter_remove", { id: f.id });
      filters = filters.filter((x) => x.id !== f.id);
      notify(t("settings.filters.removed"));
    } catch (e) {
      notify(t("settings.filters.removeFailed", { message: asErrorMessage(e) }));
    }
  }

  /// フィルタ対象・種別の日本語ラベル。未定義の値は生値をそのまま出す
  /// （メッセージキー欠落時のフォールバック。sponsor カテゴリと同じ方式）。
  function filterTargetLabel(target: string): string {
    const key = `filter.target.${target}` as MessageKey;
    const s = t(key);
    return s === key ? target : s;
  }

  function filterKindLabel(kind: string): string {
    const key = `filter.kind.${kind}` as MessageKey;
    const s = t(key);
    return s === key ? kind : s;
  }

  onMount(() => {
    void loadFilters();
  });
</script>

  <section class="panel">
    <h2>{t("settings.filters.title")}</h2>
    <p class="subtle desc">{t("settings.filters.desc")}</p>
    <div class="filter-form">
      <select bind:value={fTarget} aria-label={t("settings.filters.target")}>
        {#each FILTER_TARGETS as target}
          <option value={target}>{t(`filter.target.${target}`)}</option>
        {/each}
      </select>
      <select bind:value={fKind} aria-label={t("settings.filters.kind")}>
        {#each FILTER_KINDS as kind}
          <option value={kind}>{t(`filter.kind.${kind}`)}</option>
        {/each}
      </select>
      <input
        type="text"
        class="pattern-input"
        bind:value={fPattern}
        placeholder={t("settings.filters.pattern.placeholder")}
        onkeydown={(e) => e.key === "Enter" && addFilter()}
      />
      <button onclick={addFilter} disabled={!fPattern.trim()}>
        {t("settings.filters.add")}
      </button>
    </div>
    {#if filters.length === 0}
      <p class="subtle">{t("settings.filters.empty")}</p>
    {:else}
      <ul class="filter-list">
        {#each filters as f (f.id)}
          <li>
            <span class="f-target">{filterTargetLabel(f.target)}</span>
            <span class="f-kind">{filterKindLabel(f.kind)}</span>
            <code class="f-pattern">{f.pattern}</code>
            <button class="link" onclick={() => removeFilter(f)}>
              {t("settings.filters.remove")}
            </button>
          </li>
        {/each}
      </ul>
    {/if}
  </section>

<style>
  .panel {
    padding: 16px;
    border: 1px solid #3c4043;
    border-radius: 12px;
    background: #202124;
  }

  h2 {
    font-size: 1.1rem;
    margin: 0 0 4px;
  }

  .desc {
    font-size: 0.9rem;
    margin-top: 0;
  }

  .filter-form {
    display: flex;
    gap: 8px;
    margin: 12px 0;
    flex-wrap: wrap;
  }

  .filter-form select {
    padding: 4px;
    border-radius: 6px;
  }

  .pattern-input {
    flex: 1;
    min-width: 200px;
  }

  .filter-list {
    list-style: none;
    padding: 0;
    margin: 8px 0 0;
  }

  .filter-list li {
    display: flex;
    gap: 10px;
    align-items: baseline;
    padding: 4px 0;
    font-size: 0.9rem;
  }

  .f-target {
    color: #8ab4f8;
    white-space: nowrap;
  }

  .f-kind {
    color: #9aa0a6;
    font-size: 0.8rem;
    white-space: nowrap;
  }

  .f-pattern {
    color: #e8eaed;
    overflow-wrap: anywhere;
    flex: 1;
  }
</style>
