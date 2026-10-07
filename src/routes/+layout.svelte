<script lang="ts">
  import { page } from "$app/state";
  import "../app.css";
  import { t } from "$lib/i18n";

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
