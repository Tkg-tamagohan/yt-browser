<script lang="ts">
  import { page } from "$app/state";
  import "../app.css";
  import { t } from "$lib/i18n";
  import { appNotices } from "$lib/notices.svelte";
  import { initDeepLinks } from "$lib/deeplink.svelte";
  import PlayerCards from "$lib/PlayerCards.svelte";
  import QueuePanel from "$lib/QueuePanel.svelte";
  import { queuePanel, tempQueue } from "$lib/queue.svelte";
  import {
    appUpdate,
    applyUpdate,
    checkForUpdateAtStartup,
    dismissUpdate,
  } from "$lib/updater.svelte";

  let { children } = $props();

  // deep link（yt-browser://open?url=...）の受信を開始する
  // （FR-17、仕様決定 AC）。リスナー登録と保留分ドレインを行い、
  // アンマウント時に解除する
  $effect(() => initDeepLinks());

  // 起動時の自動更新確認（FR-15、仕様決定 AB）。更新検出時のみ
  // 下の確認ダイアログが出る
  $effect(() => {
    checkForUpdateAtStartup();
  });
</script>

<nav class="app-nav">
  <a href="/" class:active={page.url.pathname === "/"}>{t("nav.player")}</a>
  <a href="/feed" class:active={page.url.pathname === "/feed"}>{t("nav.feed")}</a>
  <a href="/search" class:active={page.url.pathname === "/search"}
    >{t("nav.search")}</a
  >
  <a href="/library" class:active={page.url.pathname === "/library"}
    >{t("nav.library")}</a
  >
  <a href="/settings" class:active={page.url.pathname === "/settings"}
    >{t("nav.settings")}</a
  >
  <!-- キュー入口（FR-20・FR-26、仕様決定 AM・AS）。
       件数は一時キューのもの。パネル側で全キューを切り替えて見られる -->
  <button
    class="nav-queue"
    class:active={queuePanel.open}
    onclick={() => (queuePanel.open = !queuePanel.open)}
  >
    {t("nav.queue")}{#if tempQueue().items.length > 0}（{tempQueue().items.length}）{/if}
  </button>
</nav>
<!-- キューパネルは常時マウントし hidden で切り替える（PlayerCards と同じ方針） -->
<div class:hidden={!queuePanel.open}>
  <QueuePanel />
</div>
{@render children()}
<!-- プレイヤーカードは常時マウント（webkit2gtk のヒットずれ対策。
     中身の表示は PlayerCards 側でパス判定して hidden にする） -->
<PlayerCards />

<div class="toasts" aria-live="polite">
  {#each appNotices as n (n.id)}
    <div class="notice">{n.msg}</div>
  {/each}
</div>

{#if appUpdate.pending && !appUpdate.dismissed}
  <!-- 更新確認ダイアログ（仕様決定 AB: 検知時は確認→承認で DL/適用/再起動） -->
  <div class="update-overlay">
    <div class="update-dialog" role="dialog" aria-label={t("update.title")}>
      <h2>{t("update.title")}</h2>
      <p>
        {t("update.body", { version: appUpdate.pending.version })}
      </p>
      <div class="update-actions">
        {#if appUpdate.installing}
          <p class="subtle">{t("update.installing")}</p>
        {:else}
          <button onclick={() => void dismissUpdate()}>
            {t("update.later")}
          </button>
          <button class="primary" onclick={() => void applyUpdate()}>
            {t("update.apply")}
          </button>
        {/if}
      </div>
    </div>
  </div>
{/if}

<style>
  .nav-queue {
    margin-left: auto;
    padding: 0;
    border: none;
    background: none;
    color: #9aa0a6;
    font: inherit;
    cursor: pointer;
  }

  .nav-queue:hover,
  .nav-queue.active {
    color: #e8e8e8;
  }

  .hidden {
    display: none;
  }

  .toasts {
    position: fixed;
    right: 16px;
    bottom: 16px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    max-width: 360px;
    z-index: 50;
  }
  .toasts .notice {
    margin-top: 0;
  }
  .update-overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.5);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 60;
  }
  .update-dialog {
    background: #202124;
    border: 1px solid #3c4043;
    border-radius: 8px;
    padding: 20px;
    max-width: 420px;
  }
  .update-dialog h2 {
    margin-top: 0;
  }
  .update-actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
  .update-actions .primary {
    font-weight: 600;
  }
</style>
