<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { t } from "$lib/i18n";
  import { fmtChatDateTime } from "$lib/format";
  import { asErrorMessage, type ChatEvent } from "$lib/players.svelte";

  let { notify }: { notify: (msg: string) => void } = $props();

  // チャット履歴検索（FR-6）
  let chatQuery = $state("");
  let chatVideoId = $state("");
  let chatResults = $state<ChatEvent[] | null>(null);
  let chatSearching = $state(false);

  async function searchChat(): Promise<void> {
    const query = chatQuery.trim();
    if (!query) return;
    chatSearching = true;
    try {
      chatResults = await invoke<ChatEvent[]>("chat_history_search", {
        videoId: chatVideoId.trim() || null,
        query,
        limit: 200,
      });
    } catch (e) {
      chatResults = null;
      notify(t("settings.chatSearch.failed", { message: asErrorMessage(e) }));
    }
    chatSearching = false;
  }
</script>

  <section class="panel">
    <h2>{t("settings.chatSearch.title")}</h2>
    <p class="subtle desc">{t("settings.chatSearch.desc")}</p>
    <div class="filter-form">
      <input
        type="text"
        class="vid-input"
        bind:value={chatVideoId}
        placeholder={t("settings.chatSearch.videoId")}
      />
      <input
        type="text"
        class="pattern-input"
        bind:value={chatQuery}
        placeholder={t("settings.chatSearch.placeholder")}
        onkeydown={(e) => e.key === "Enter" && searchChat()}
      />
      <button onclick={searchChat} disabled={chatSearching || !chatQuery.trim()}>
        {chatSearching ? t("settings.chatSearch.searching") : t("settings.chatSearch.button")}
      </button>
    </div>
    {#if chatResults !== null}
      {#if chatResults.length === 0}
        <p class="subtle">{t("settings.chatSearch.empty")}</p>
      {:else}
        <p class="subtle">
          {t("settings.chatSearch.count", { count: chatResults.length })}
        </p>
        <ul class="chat-hits">
          {#each chatResults as e (e)}
            <li>
              <span class="chat-time">{fmtChatDateTime(e.postedAtUsec)}</span>
              <span class="ch-title">{e.authorName ?? "-"}</span>
              {#if e.kind !== "text"}<span class="f-kind">{e.kind}</span>{/if}
              <span class="hit-msg">{e.message}</span>
            </li>
          {/each}
        </ul>
      {/if}
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

  .filter-form {
    display: flex;
    gap: 8px;
    margin: 12px 0;
    flex-wrap: wrap;
  }

  .pattern-input {
    flex: 1;
    min-width: 200px;
  }

  .vid-input {
    width: 220px;
    font-family: monospace;
  }

  .chat-hits {
    list-style: none;
    padding: 0;
    margin: 8px 0 0;
  }

  .chat-hits li {
    display: flex;
    gap: 10px;
    align-items: baseline;
    padding: 4px 0;
    font-size: 0.9rem;
  }

  .f-kind {
    color: #9aa0a6;
    font-size: 0.8rem;
    white-space: nowrap;
  }

  .chat-time {
    color: #9aa0a6;
    font-family: monospace;
    font-size: 0.8rem;
    white-space: nowrap;
  }

  .hit-msg {
    overflow-wrap: anywhere;
    min-width: 0;
  }

  .chat-hits {
    max-height: 320px;
    overflow-y: auto;
  }
</style>
