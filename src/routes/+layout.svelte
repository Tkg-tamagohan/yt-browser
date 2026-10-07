<script lang="ts">
  import { page } from "$app/state";
  import "../app.css";
  import { t } from "$lib/i18n";
  import { appNotices } from "$lib/notices.svelte";

  let { children } = $props();
</script>

<nav class="app-nav">
  <a href="/" class:active={page.url.pathname === "/"}>{t("nav.player")}</a>
  <a href="/feed" class:active={page.url.pathname === "/feed"}>{t("nav.feed")}</a>
  <a href="/settings" class:active={page.url.pathname === "/settings"}
    >{t("nav.settings")}</a
  >
</nav>
{@render children()}

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
