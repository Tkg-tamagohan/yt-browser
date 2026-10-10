<script lang="ts">
  import { t } from "$lib/i18n";
  import { appUpdate, checkForUpdate } from "$lib/updater.svelte";
</script>

  <section class="panel">
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
</style>
