<script lang="ts">
  // 通知履歴セクション（FR-83、仕様決定 DH、設計書 §4.22）。
  // 共有キュー経由で発行された通知のセッション内履歴を新しい順に一覧する。
  // 履歴はメモリ内のみ（再起動で消える。暫定解釈）で、保持上限は
  // NOTICE_HISTORY_MAX（暫定）。
  import { t } from "$lib/i18n";
  import {
    formatNoticeAt,
    historyDesc,
    NOTICE_HISTORY_MAX,
  } from "$lib/notice-logic";
  import {
    clearNoticeHistory,
    noticeHistory,
  } from "$lib/notices.svelte";

  // 新しいものを先頭にする（暫定解釈）
  const items = $derived(historyDesc(noticeHistory));
</script>

<section class="panel panel--filled">
  <h2>{t("settings.notices.title")}</h2>
  <p class="subtle desc">
    {t("settings.notices.desc", { max: NOTICE_HISTORY_MAX })}
  </p>

  {#if items.length === 0}
    <p class="subtle empty">{t("settings.notices.empty")}</p>
  {:else}
    <ul class="history">
      {#each items as n (n.id)}
        <li class="row">
          <span class="time">{formatNoticeAt(n.at)}</span>
          <span class="msg">{n.msg}</span>
          {#if n.actionLabel}
            <span class="action">
              {t("settings.notices.action", { label: n.actionLabel })}
            </span>
          {/if}
        </li>
      {/each}
    </ul>
    <button class="clear" onclick={() => clearNoticeHistory()}>
      {t("settings.notices.clear")}
    </button>
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

  .empty {
    margin: 12px 0 0;
  }

  .history {
    list-style: none;
    margin: 12px 0 0;
    padding: 0;
    max-height: 320px;
    overflow-y: auto;
    border-top: 1px solid var(--border-row);
  }

  .row {
    display: flex;
    align-items: baseline;
    gap: 10px;
    padding: 6px 0;
    border-bottom: 1px solid var(--border-row);
  }

  .time {
    color: var(--accent);
    font-variant-numeric: tabular-nums;
    flex-shrink: 0;
  }

  .msg {
    flex: 1;
    min-width: 0;
    overflow-wrap: anywhere;
  }

  .action {
    color: var(--fg-muted);
    font-size: 0.8rem;
    flex-shrink: 0;
  }

  .clear {
    margin-top: 12px;
  }
</style>
