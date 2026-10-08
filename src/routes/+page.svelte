<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { t } from "$lib/i18n";
  import {
    playbackHooks,
    type DbStatus,
    type UiError,
    type WatchHistory,
    type YtDlpStatus,
  } from "$lib/players.svelte";

  let dbStatus = $state<DbStatus | null>(null);
  let dbError = $state("");

  let input = $state("");
  let resumeHint = $state<WatchHistory | null>(null);
  let notices = $state<string[]>([]);
  let ytdlp = $state<YtDlpStatus | null>(null);
  let ytdlpChecking = $state(true);
  let ytdlpUpdating = $state(false);

  function fmt(sec: number): string {
    if (!Number.isFinite(sec) || sec <= 0) return "0:00";
    const s = Math.floor(sec);
    const h = Math.floor(s / 3600);
    const m = Math.floor((s % 3600) / 60);
    const ss = s % 60;
    const mm = h > 0 ? String(m).padStart(2, "0") : String(m);
    return `${h > 0 ? h + ":" : ""}${mm}:${String(ss).padStart(2, "0")}`;
  }

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

  async function refreshResumeHint(): Promise<void> {
    const query = input.trim();
    resumeHint = null;
    if (!query) return;
    try {
      const h = await invoke<WatchHistory | null>("history_get", {
        videoId: query,
      });
      // 応答が返るまでに入力が変わっていたら結果は捨てる
      if (input.trim() !== query) return;
      if (h && !h.completed && h.positionSec > 0) resumeHint = h;
    } catch {
      // 入力が URL として解釈できない段階では黙って無視する
    }
  }

  async function play(resume: boolean, pip = false): Promise<void> {
    try {
      await invoke<number>("play_video", {
        videoId: input.trim(),
        resume,
        pip,
      });
      if (resume && resumeHint) {
        notify(t("player.resumeApplied", { position: fmt(resumeHint.positionSec) }));
      }
    } catch (e) {
      notify(t("player.error", { message: asErrorMessage(e) }));
    }
  }

  async function refreshYtDlp(): Promise<void> {
    ytdlpChecking = true;
    try {
      ytdlp = await invoke<YtDlpStatus>("ytdlp_status");
    } catch {
      ytdlp = null;
    }
    ytdlpChecking = false;
  }

  async function updateYtDlp(): Promise<void> {
    ytdlpUpdating = true;
    try {
      const out = await invoke<string>("ytdlp_update");
      notify(t("ytdlp.updated", { output: out }));
      await refreshYtDlp();
    } catch (e) {
      notify(t("ytdlp.updateFailed", { message: asErrorMessage(e) }));
    }
    ytdlpUpdating = false;
  }

  onMount(async () => {
    // プレイヤーカード（PlayerCards）はレイアウト側で常時マウントされるため、
    // 終了/クローズ後の resumeHint 再取得はフック登録で受け取る
    playbackHooks.add(refreshResumeHint);
    try {
      dbStatus = await invoke<DbStatus>("db_status");
    } catch (e) {
      dbError = asErrorMessage(e);
    }
    await refreshYtDlp();
  });

  onDestroy(() => {
    playbackHooks.delete(refreshResumeHint);
  });
</script>

<main class="container">
  <h1>yt-browser</h1>
  <p class="lead">{t("home.lead")}</p>

  <div class="play-form">
    <input
      class="url-input"
      type="text"
      placeholder={t("player.input.placeholder")}
      bind:value={input}
      oninput={refreshResumeHint}
    />
    <button onclick={() => play(false)} disabled={!input.trim()}>
      {t("player.play")}
    </button>
    <button onclick={() => play(true)} disabled={!resumeHint}>
      {t("player.playResume")}
    </button>
    <button
      title={t("player.pip.hint")}
      onclick={() => play(false, true)}
      disabled={!input.trim()}
    >
      {t("player.playPip")}
    </button>
  </div>
  {#if resumeHint}
    <p class="hint">
      {t("player.history.hint", { position: fmt(resumeHint.positionSec) })}
      {#if resumeHint.title}（{resumeHint.title}）{/if}
    </p>
  {/if}

  {#each notices as n}
    <p class="notice">{n}</p>
  {/each}

  <footer class="status-bar">
    {#if dbStatus}
      <span class="status ok">{t("home.db.ok", { version: dbStatus.schemaVersion })}</span>
    {:else if dbError}
      <span class="status ng">{t("home.db.ng", { error: dbError })}</span>
    {:else}
      <span class="status">{t("home.db.checking")}</span>
    {/if}
    {#if ytdlpChecking}
      <span class="status">{t("ytdlp.checking")}</span>
    {:else if ytdlp?.path}
      <span class="status">{t("ytdlp.ok", { version: ytdlp.version ?? "?", path: ytdlp.path })}</span>
      <button class="link" onclick={updateYtDlp} disabled={ytdlpUpdating}>
        {ytdlpUpdating ? t("ytdlp.updating") : t("ytdlp.update")}
      </button>
    {:else}
      <span class="status ng">{t("ytdlp.missing")}</span>
    {/if}
  </footer>
</main>

<style>
  h1 {
    font-size: 2rem;
    margin-bottom: 0.5rem;
  }

  .lead {
    color: #9aa0a6;
    margin-bottom: 2rem;
  }

  .play-form {
    display: flex;
    gap: 8px;
  }

  .url-input {
    flex: 1;
  }

  .hint {
    color: #9aa0a6;
    font-size: 0.9rem;
  }

  .status-bar {
    margin-top: 32px;
    display: flex;
    gap: 12px;
    align-items: center;
    flex-wrap: wrap;
    font-size: 0.85rem;
  }

  .status {
    padding: 4px 10px;
    border-radius: 8px;
    background: #26282c;
    font-family: monospace;
  }

  .status.ok {
    color: #7ee787;
  }

  .status.ng {
    color: #ff7b72;
  }
</style>
