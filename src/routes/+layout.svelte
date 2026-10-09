<script lang="ts">
  import { page } from "$app/state";
  import "../app.css";
  import { t } from "$lib/i18n";
  import { appNotices } from "$lib/notices.svelte";
  import { initDeepLinks } from "$lib/deeplink.svelte";
  import PlayerCards from "$lib/PlayerCards.svelte";

  let { children } = $props();

  // deep link（yt-browser://open?url=...）の受信を開始する
  // （FR-17、仕様決定 AC）。リスナー登録と保留分ドレインを行い、
  // アンマウント時に解除する
  $effect(() => initDeepLinks());
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
</nav>
{@render children()}
<!-- プレイヤーカードは常時マウント（webkit2gtk のヒットずれ対策。
     中身の表示は PlayerCards 側でパス判定して hidden にする） -->
<PlayerCards />

<div class="toasts" aria-live="polite">
  {#each appNotices as n (n.id)}
    <div class="notice">{n.msg}</div>
  {/each}
</div>

<style>
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
</style>
