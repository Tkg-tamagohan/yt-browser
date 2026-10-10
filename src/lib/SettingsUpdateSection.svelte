<script lang="ts">
  import { t } from "$lib/i18n";
  import { appUpdate, checkForUpdate } from "$lib/updater.svelte";
</script>

  <section class="panel panel--filled">
    <h2>{t("settings.update.title")}</h2>
    <p class="subtle desc">{t("settings.update.desc")}</p>
    <div>
      <button
        onclick={() => void checkForUpdate(true)}
        disabled={appUpdate.checking}
      >
        {appUpdate.checking
          ? t("settings.update.checking")
          : t("settings.update.check")}
      </button>
    </div>
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
</style>
