<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { t } from "$lib/i18n";
  import { asErrorMessage, type Filter } from "$lib/players.svelte";
  import {
    FILTER_TARGETS,
    FILTER_KINDS,
    filterTargetLabel,
    filterKindLabel,
  } from "$lib/settings-consts";

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

  onMount(() => {
    void loadFilters();
  });
</script>

  <section class="panel panel--filled">
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
    <!-- 空表示と一覧の出し分けはリマウントを避けるため hidden 切替
         （ヒットずれ対策、FR-80） -->
    <p class="subtle" class:hidden={filters.length > 0}>
      {t("settings.filters.empty")}
    </p>
      <ul class="filter-list" class:hidden={filters.length === 0}>
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
  </section>
