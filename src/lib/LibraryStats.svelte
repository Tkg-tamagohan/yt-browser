<script lang="ts">
  // 統計タブ（FR-65、仕様決定 CP）。`history_stats` の集計を
  // 累計視聴時間・チャンネル別・日別の 3 区画で表示する。
  // データはこのタブを開いたときに取り直す（履歴は再生のたびに
  // 変わるためキャッシュしない。再読み込みボタンでも取り直せる）
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { t } from "$lib/i18n";
  import { notify } from "$lib/notices.svelte";
  import { fmtWatchSec } from "$lib/stats-logic";
  import { asErrorMessage, type HistoryStats } from "$lib/players.svelte";

  let stats = $state<HistoryStats | null>(null);
  let loading = $state(false);

  async function load(): Promise<void> {
    loading = true;
    try {
      stats = await invoke<HistoryStats>("history_stats");
    } catch (e) {
      notify(t("library.stats.failed", { message: asErrorMessage(e) }));
    } finally {
      loading = false;
    }
  }

  onMount(() => {
    void load();
  });
</script>

{#if loading && stats === null}
  <p class="subtle">{t("desc.loading")}</p>
{:else if stats === null || stats.entryCount === 0}
  <p class="subtle">{t("library.stats.empty")}</p>
{:else}
  <div class="stats-head">
    <div>
      <div class="total-label subtle">{t("library.stats.total")}</div>
      <div class="total-value">{fmtWatchSec(stats.totalSec)}</div>
      <div class="subtle">
        {t("library.stats.entries", { count: stats.entryCount })}
      </div>
    </div>
    <button class="link" onclick={() => void load()} disabled={loading}>
      {loading ? t("settings.ops.busy") : t("library.stats.refresh")}
    </button>
  </div>

  <div class="stats-grid">
    <section>
      <h2>{t("library.stats.byChannel")}</h2>
      <ul class="stat-list">
        {#each stats.byChannel as c (c.channelId ?? "")}
          <li>
            <span class="stat-name" title={c.channelId ?? ""}>
              {c.channelTitle || c.channelId || t("library.stats.unknownChannel")}
            </span>
            <span class="stat-value">
              {fmtWatchSec(c.totalSec)}
              <span class="subtle">
                {t("library.stats.entries", { count: c.entryCount })}
              </span>
            </span>
          </li>
        {/each}
      </ul>
    </section>
    <section>
      <h2>{t("library.stats.byDay")}</h2>
      <ul class="stat-list">
        {#each stats.byDay as d (d.day)}
          <li>
            <span class="stat-name">{d.day}</span>
            <span class="stat-value">
              {fmtWatchSec(d.totalSec)}
              <span class="subtle">
                {t("library.stats.entries", { count: d.entryCount })}
              </span>
            </span>
          </li>
        {/each}
      </ul>
    </section>
  </div>
{/if}

<style>
  .stats-head {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    margin-bottom: 16px;
  }

  .total-label {
    font-size: 0.85rem;
  }

  .total-value {
    font-size: 1.8rem;
    color: #8ab4f8;
  }

  .stats-grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 16px;
  }

  h2 {
    font-size: 1.05rem;
    margin: 0 0 8px;
  }

  .stat-list {
    list-style: none;
    margin: 0;
    padding: 0;
    max-height: 50vh;
    overflow-y: auto;
  }

  .stat-list li {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 12px;
    padding: 4px 0;
    border-top: 1px solid #2d2f33;
  }

  .stat-name {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .stat-value {
    flex-shrink: 0;
    text-align: right;
  }

  .stat-value .subtle {
    margin-left: 6px;
    font-size: 0.8rem;
  }

  @media (max-width: 700px) {
    .stats-grid {
      grid-template-columns: 1fr;
    }
  }
</style>
