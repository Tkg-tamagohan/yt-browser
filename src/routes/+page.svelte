<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { t } from "$lib/i18n";
  import { fmtDuration } from "$lib/format";
  import { parseTimeParam } from "$lib/deeplink.svelte";
  import { pipDefaultEnabled } from "$lib/pip";
  import {
    asErrorMessage,
    playbackHooks,
    type DbStatus,
    type WatchHistory,
    type YtDlpStatus,
  } from "$lib/players.svelte";

  let dbStatus = $state<DbStatus | null>(null);
  let dbError = $state("");

  let input = $state("");
  let resumeHint = $state<WatchHistory | null>(null);
  let notices = $state<string[]>([]);
  // 再生の既定表示モード（設定 `pip.default`。未設定は PiP 既定、仕様決定 AJ）
  let pipDefault = $state(true);
  let ytdlp = $state<YtDlpStatus | null>(null);
  let ytdlpChecking = $state(true);
  let ytdlpUpdating = $state(false);

  function notify(msg: string): void {
    notices = [...notices.slice(-4), msg];
    setTimeout(() => {
      notices = notices.filter((n) => n !== msg);
    }, 6000);
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

  // pip を省略して呼ぶとバックエンドが設定 `pip.default` で解決する
  // （false の明示は「強制通常窓」の意味を持つため既定値にしない、仕様決定 AJ）
  async function play(resume: boolean, pip?: boolean): Promise<void> {
    try {
      await invoke<number>("play_video", {
        videoId: input.trim(),
        resume,
        pip,
      });
      // t=/start= が有効な値を持つ URL は履歴位置よりそちらが優先されるため、
      // 実際の開始位置と違う再開通知を出さない。無効値（t=abc 等）は backend が
      // 無視して履歴から再開するため、通知も出す（Devin Review #48 指摘）
      let hasValidStart = false;
      try {
        hasValidStart = parseTimeParam(new URL(input.trim())) !== null;
      } catch {
        // URL でない入力は t= を持たないので通知判定には影響しない
      }
      if (resume && resumeHint && !hasValidStart) {
        notify(t("player.resumeApplied", { position: fmtDuration(resumeHint.positionSec) }));
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
      const raw = await invoke<string | null>("settings_get", {
        key: "pip.default",
      });
      pipDefault = pipDefaultEnabled(raw);
    } catch {
      // 読み取り失敗時は PiP 既定のままにする
    }
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
      title={pipDefault ? t("player.window.hint") : t("player.pip.hint")}
      onclick={() => play(false, !pipDefault)}
      disabled={!input.trim()}
    >
      {pipDefault ? t("player.playWindow") : t("player.playPip")}
    </button>
  </div>
  {#if resumeHint}
    <p class="hint">
      {t("player.history.hint", { position: fmtDuration(resumeHint.positionSec) })}
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
