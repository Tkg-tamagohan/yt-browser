<script lang="ts">
  // 依存コマンドの検出一覧（FR-74、仕様決定 CY）。
  // 設定画面の診断節（SettingsDiagSection）と初回起動の依存検出画面
  // （SetupWizard、FR-75）で共用する。`deps_status` の結果を
  // 名前・検出状況・バージョン・解決パスの行で表示する。
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { t } from "$lib/i18n";
  import { asErrorMessage, type DepStatus } from "$lib/players.svelte";

  let {
    notify,
  }: {
    // エラー通知先。省略時は失敗を行内表示に留める
    notify?: (msg: string) => void;
  } = $props();

  let deps = $state<DepStatus[]>([]);
  let loading = $state(true);
  let error = $state<string | null>(null);

  async function refresh(): Promise<void> {
    loading = true;
    try {
      deps = await invoke<DepStatus[]>("deps_status");
      error = null;
    } catch (e) {
      error = asErrorMessage(e);
      notify?.(t("deps.failed", { message: error }));
    }
    loading = false;
  }

  onMount(() => {
    void refresh();
  });
</script>

<div class="deps-list">
  <!-- 読み込み中・失敗・一覧の出し分けはリマウントを避けるため
       hidden 切替（ヒットずれ対策、FR-80） -->
  <p class="subtle" class:hidden={!(loading && deps.length === 0)}>
    {t("deps.refreshing")}
  </p>
  <div class:hidden={loading && deps.length === 0}>
    {#each deps as d (d.name)}
      <div class="dep-row">
        <span class="dep-name">{d.name}</span>
        <span class="dep-status" class:ok={d.detected} class:ng={!d.detected}>
          {d.detected ? t("deps.detected") : t("deps.missing")}
        </span>
        <span class="dep-version">{d.version ?? "—"}</span>
        <span class="dep-path">{d.path ?? "—"}</span>
      </div>
    {/each}
    <p class="subtle ng" class:hidden={!error}>
      {t("deps.failed", { message: error ?? "" })}
    </p>
    <button class="refresh" onclick={() => void refresh()} disabled={loading}>
      {loading ? t("deps.refreshing") : t("deps.refresh")}
    </button>
  </div>
</div>

<style>
  .deps-list {
    margin-top: 10px;
  }

  .dep-row {
    display: grid;
    grid-template-columns: 72px 56px 1fr 2fr;
    gap: 10px;
    align-items: baseline;
    padding: 6px 0;
    border-top: 1px solid var(--border-row);
    font-size: 0.9rem;
  }

  .dep-name {
    font-weight: 600;
  }

  .dep-status.ok {
    color: var(--success);
  }

  .dep-status.ng {
    color: var(--danger);
  }

  .dep-version {
    color: var(--fg-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .dep-path {
    font-family: monospace;
    font-size: 0.8rem;
    color: var(--fg-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .ng {
    color: var(--danger);
  }

  .refresh {
    margin-top: 10px;
  }
</style>
