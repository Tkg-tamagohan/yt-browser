<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { t } from "$lib/i18n";
  import {
    initPlayerEvents,
    playerStates,
    type UiError,
  } from "$lib/players.svelte";

  // 設計書 §4.3 のプリセット表
  const PRESETS = [
    {
      key: "settings.quality.preset.1080" as const,
      format: "bv*[height<=1080]+ba/b[height<=1080]",
    },
    {
      key: "settings.quality.preset.1080p60" as const,
      format:
        "bv*[height<=1080][fps>30]+ba/bv*[height<=1080]+ba/b[height<=1080]",
    },
    {
      key: "settings.quality.preset.av1" as const,
      format:
        "bv*[vcodec^=av01][height<=1080]+ba/bv*[vcodec^=vp9][height<=1080]+ba/b[height<=1080]",
    },
    { key: "settings.quality.preset.best" as const, format: "bv*+ba/b" },
  ];
  const CUSTOM = "custom";

  let selected = $state<string>(PRESETS[0].format);
  let customFormat = $state("");
  let loading = $state(true);
  let saving = $state(false);
  let notices = $state<string[]>([]);

  const effectiveFormat = $derived(
    selected === CUSTOM ? customFormat.trim() : selected,
  );

  function notify(msg: string): void {
    notices = [...notices.slice(-4), msg];
    setTimeout(() => {
      notices = notices.filter((n) => n !== msg);
    }, 6000);
  }

  function asErrorMessage(e: unknown): string {
    if (typeof e === "object" && e !== null && "message" in e) {
      return String((e as UiError).message);
    }
    return String(e);
  }

  async function save(): Promise<void> {
    const format = effectiveFormat;
    if (!format) return;
    saving = true;
    try {
      await invoke("settings_set", { key: "quality.format", value: format });
      // 再生中のインスタンスへ即時適用（設計書 §4.3: set_property + loadfile replace）
      let applied = 0;
      const failed: number[] = [];
      for (const id of playerStates.list.keys()) {
        try {
          await invoke("player_control", {
            instanceId: id,
            action: { type: "quality", format },
          });
          applied += 1;
        } catch {
          // 適用に失敗し、かつまだ稼働中なら失敗として記録する
          // （直前に終了したインスタンスは一覧から消えているので除外）
          if (playerStates.list.has(id)) failed.push(id);
        }
      }
      if (failed.length > 0) {
        notify(t("settings.applyPartial", { count: failed.length }));
      } else if (applied > 0) {
        notify(t("settings.applied", { count: applied }));
      } else {
        notify(t("settings.saved"));
      }
    } catch (e) {
      notify(t("settings.failed", { message: asErrorMessage(e) }));
    }
    saving = false;
  }

  onMount(async () => {
    await initPlayerEvents();
    try {
      const stored = await invoke<string | null>("settings_get", {
        key: "quality.format",
      });
      if (stored) {
        if (PRESETS.some((p) => p.format === stored)) {
          selected = stored;
        } else {
          selected = CUSTOM;
          customFormat = stored;
        }
      }
    } catch {
      // 読み取り失敗時はプリセット既定のままにする
    }
    loading = false;
  });
</script>

<main class="container">
  <h1>{t("settings.title")}</h1>

  <section class="panel">
    <h2>{t("settings.quality.title")}</h2>
    <p class="subtle desc">{t("settings.quality.desc")}</p>
    {#if loading}
      <p class="subtle">…</p>
    {:else}
      <div class="presets">
        {#each PRESETS as p}
          <label>
            <input type="radio" bind:group={selected} value={p.format} />
            {t(p.key)}
            <code>{p.format}</code>
          </label>
        {/each}
        <label>
          <input type="radio" bind:group={selected} value={CUSTOM} />
          {t("settings.quality.custom")}
        </label>
        {#if selected === CUSTOM}
          <input
            type="text"
            class="format-input"
            bind:value={customFormat}
            placeholder={t("settings.quality.format.label")}
          />
        {/if}
      </div>
      <button
        onclick={save}
        disabled={saving || !effectiveFormat}
      >
        {saving ? t("settings.saving") : t("settings.save")}
      </button>
    {/if}
  </section>

  {#each notices as n}
    <p class="notice">{n}</p>
  {/each}
</main>

<style>
  h1 {
    font-size: 2rem;
    margin-bottom: 1.5rem;
  }

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

  .presets {
    display: flex;
    flex-direction: column;
    gap: 10px;
    margin: 12px 0 16px;
  }

  .presets label {
    display: flex;
    gap: 8px;
    align-items: baseline;
    font-size: 0.95rem;
  }

  .presets code {
    color: #9aa0a6;
    font-size: 0.8rem;
    overflow-wrap: anywhere;
  }

  .format-input {
    width: 100%;
    margin-top: 4px;
    font-family: monospace;
  }
</style>
