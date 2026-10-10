<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { t } from "$lib/i18n";
  import { asErrorMessage, type BlockedChannel } from "$lib/players.svelte";

  let { notify }: { notify: (msg: string) => void } = $props();

  // ブロック中チャンネル（FR-5: 設定画面での解除）
  let blocked = $state<BlockedChannel[]>([]);

  async function loadBlocked(): Promise<void> {
    try {
      blocked = await invoke<BlockedChannel[]>("blocked_channels");
    } catch {
      blocked = [];
    }
  }

  async function unblock(b: BlockedChannel): Promise<void> {
    try {
      await invoke("unblock_channel", { channelId: b.channelId });
      blocked = blocked.filter((x) => x.channelId !== b.channelId);
      notify(t("blocked.unblocked", { title: b.title }));
    } catch (e) {
      notify(t("blocked.unblockFailed", { message: asErrorMessage(e) }));
    }
  }

  onMount(() => {
    void loadBlocked();
  });
</script>

  <section class="panel">
    <h2>{t("blocked.title")}</h2>
    <p class="subtle desc">{t("blocked.desc")}</p>
    {#if blocked.length === 0}
      <p class="subtle">{t("blocked.empty")}</p>
    {:else}
      <ul class="blocked-list">
        {#each blocked as b (b.channelId)}
          <li>
            <span class="ch-title" title={b.channelId}>{b.title}</span>
            <button class="link" onclick={() => unblock(b)}>
              {t("blocked.unblock")}
            </button>
          </li>
        {/each}
      </ul>
    {/if}
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
