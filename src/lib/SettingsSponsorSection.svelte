<script lang="ts">
  import { t } from "$lib/i18n";
  import {
    SPONSOR_CATEGORIES,
    type SponsorCategory,
  } from "$lib/settings-consts";

  let {
    loading,
    sponsorActions = $bindable(),
  }: {
    loading: boolean;
    sponsorActions: Record<SponsorCategory, string>;
  } = $props();
</script>

  <section class="panel panel--filled">
    <h2>{t("settings.sponsor.title")}</h2>
    <p class="subtle desc">{t("settings.sponsor.desc")}</p>
    {#if loading}
      <p class="subtle">…</p>
    {:else}
      <div class="sponsor-table">
        {#each SPONSOR_CATEGORIES as cat}
          <label>
            <span class="cat-label">{t(`sponsor.cat.${cat}`)}</span>
            <select bind:value={sponsorActions[cat]}>
              <option value="skip">{t("settings.sponsor.action.skip")}</option>
              <option value="notify">
                {t("settings.sponsor.action.notify")}
              </option>
              <option value="off">{t("settings.sponsor.action.off")}</option>
            </select>
          </label>
        {/each}
      </div>
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

  .sponsor-table {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin: 12px 0;
  }

  .sponsor-table label {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 12px;
    font-size: 0.95rem;
  }

  .cat-label {
    flex: 1;
  }
</style>
