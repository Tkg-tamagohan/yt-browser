<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { t } from "$lib/i18n";

  type DbStatus = { schemaVersion: number };
  type PlayStatus = "idle" | "playing" | "paused" | "buffering" | "ended";
  type PlayerState = {
    instanceId: number;
    videoId: string;
    pause: boolean;
    position: number;
    duration: number;
    fps: number;
    state: PlayStatus;
    volume: number;
    speed: number;
    mediaTitle: string;
  };
  type PlayerEnded = { instanceId: number; videoId: string; reason: string };
  type WatchHistory = {
    videoId: string;
    title: string;
    positionSec: number;
    durationSec: number | null;
    completed: boolean;
  };
  type UiError = { code: string; message: string };
  type YtDlpStatus = { path: string | null; version: string | null };
  type PlayerAction =
    | { type: "pause"; value: boolean }
    | { type: "seek"; seconds: number }
    | { type: "volume"; value: number }
    | { type: "speed"; value: number }
    | { type: "quality"; format: string }
    | { type: "frame_step" }
    | { type: "frame_back_step" };

  let dbStatus = $state<DbStatus | null>(null);
  let dbError = $state("");

  let input = $state("");
  let resumeHint = $state<WatchHistory | null>(null);
  let players = $state<Map<number, PlayerState>>(new Map());
  // シークバーはドラッグ中に state 更新で暴れないよう、操作中の値を別で持つ
  let seekPreview = $state<Map<number, number>>(new Map());
  let notices = $state<string[]>([]);
  let ytdlp = $state<YtDlpStatus | null>(null);
  let ytdlpChecking = $state(true);
  let ytdlpUpdating = $state(false);

  const SPEED_OPTIONS = [0.5, 0.75, 1, 1.25, 1.5, 2];

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
    resumeHint = null;
    if (!input.trim()) return;
    try {
      const h = await invoke<WatchHistory | null>("history_get", {
        videoId: input.trim(),
      });
      if (h && !h.completed && h.positionSec > 0) resumeHint = h;
    } catch {
      // 入力が URL として解釈できない段階では黙って無視する
    }
  }

  async function play(resume: boolean): Promise<void> {
    try {
      await invoke<number>("play_video", { videoId: input.trim(), resume });
      if (resume && resumeHint) {
        notify(t("player.resumeApplied", { position: fmt(resumeHint.positionSec) }));
      }
    } catch (e) {
      notify(t("player.error", { message: asErrorMessage(e) }));
    }
  }

  async function control(id: number, action: PlayerAction): Promise<void> {
    try {
      await invoke("player_control", { instanceId: id, action });
    } catch (e) {
      notify(t("player.error", { message: asErrorMessage(e) }));
    }
  }

  async function closePlayer(id: number): Promise<void> {
    try {
      await invoke("player_close", { instanceId: id });
      const next = new Map(players);
      next.delete(id);
      players = next;
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

  function statusLabel(p: PlayerState): string {
    return p.state === "buffering" ? t("player.buffering") : "";
  }

  let unlistenFns: UnlistenFn[] = [];

  onMount(async () => {
    try {
      dbStatus = await invoke<DbStatus>("db_status");
    } catch (e) {
      dbError = asErrorMessage(e);
    }
    await refreshYtDlp();

    unlistenFns.push(
      await listen<PlayerState>("player://state", (ev) => {
        const next = new Map(players);
        next.set(ev.payload.instanceId, ev.payload);
        players = next;
      }),
      await listen<PlayerEnded>("player://ended", (ev) => {
        const next = new Map(players);
        next.delete(ev.payload.instanceId);
        players = next;
        notify(t("player.ended", { reason: ev.payload.reason }));
      }),
    );
  });

  onDestroy(() => {
    for (const u of unlistenFns) u();
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
  </div>
  {#if resumeHint}
    <p class="hint">
      {t("player.history.hint", { position: fmt(resumeHint.positionSec) })}
      {#if resumeHint.title}（{resumeHint.title}）{/if}
    </p>
  {/if}

  {#each [...players.values()] as p (p.instanceId)}
    <section class="player">
      <div class="player-title">
        {p.mediaTitle || p.videoId}
        <span class="badge">#{p.instanceId}</span>
      </div>

      <input
        class="seek"
        type="range"
        min="0"
        max={Math.max(p.duration, 1)}
        step="0.5"
        value={seekPreview.get(p.instanceId) ?? p.position}
        disabled={p.duration <= 0}
        oninput={(e) => {
          const next = new Map(seekPreview);
          next.set(p.instanceId, Number(e.currentTarget.value));
          seekPreview = next;
        }}
        onchange={(e) => {
          const seconds = Number(e.currentTarget.value);
          control(p.instanceId, { type: "seek", seconds });
          const next = new Map(seekPreview);
          next.delete(p.instanceId);
          seekPreview = next;
        }}
      />
      <div class="times">
        <span>{fmt(seekPreview.get(p.instanceId) ?? p.position)} / {p.duration > 0 ? fmt(p.duration) : t("player.noDuration")}</span>
        {#if p.fps > 0}<span class="subtle">{t("player.fps", { fps: p.fps.toFixed(2) })}</span>{/if}
        {#if statusLabel(p)}<span class="buffering">{statusLabel(p)}</span>{/if}
      </div>

      <div class="controls">
        <button onclick={() => control(p.instanceId, { type: "pause", value: !p.pause })}>
          {p.pause ? t("player.resume") : t("player.pause")}
        </button>
        <label>
          {t("player.volume")}
          <input
            type="range"
            min="0"
            max="130"
            step="1"
            value={p.volume}
            onchange={(e) =>
              control(p.instanceId, {
                type: "volume",
                value: Number(e.currentTarget.value),
              })}
          />
          <span class="subtle">{Math.round(p.volume)}%</span>
        </label>
        <label>
          {t("player.speed")}
          <select
            value={p.speed}
            onchange={(e) =>
              control(p.instanceId, {
                type: "speed",
                value: Number(e.currentTarget.value),
              })}
          >
            {#each SPEED_OPTIONS as s}
              <option value={s} selected={s === p.speed}>{s}x</option>
            {/each}
          </select>
        </label>
        <button class="danger" onclick={() => closePlayer(p.instanceId)}>
          {t("player.close")}
        </button>
      </div>
    </section>
  {/each}

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
  :root {
    font-family: Inter, "Hiragino Kaku Gothic ProN", "Noto Sans CJK JP", Meiryo, sans-serif;
    font-size: 16px;
    line-height: 1.6;
    color: #e8e8e8;
    background-color: #1a1b1e;
    font-synthesis: none;
    text-rendering: optimizeLegibility;
    -webkit-font-smoothing: antialiased;
  }

  .container {
    margin: 0 auto;
    max-width: 720px;
    padding: 6vh 24px 48px;
    display: flex;
    flex-direction: column;
  }

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
    padding: 8px 12px;
    border-radius: 8px;
    border: 1px solid #3c4043;
    background: #202124;
    color: #e8e8e8;
  }

  button {
    padding: 8px 16px;
    border-radius: 8px;
    border: 1px solid #3c4043;
    background: #2d2f33;
    color: #e8e8e8;
    cursor: pointer;
  }

  button:hover:not(:disabled) {
    background: #3a3d42;
  }

  button:disabled {
    opacity: 0.4;
    cursor: default;
  }

  button.danger {
    border-color: #7c3a3a;
  }

  button.link {
    padding: 2px 10px;
    font-size: 0.85rem;
  }

  .hint {
    color: #9aa0a6;
    font-size: 0.9rem;
  }

  .player {
    margin-top: 16px;
    padding: 16px;
    border: 1px solid #3c4043;
    border-radius: 12px;
    background: #202124;
  }

  .player-title {
    font-weight: 600;
    margin-bottom: 8px;
    overflow-wrap: anywhere;
  }

  .badge {
    color: #9aa0a6;
    font-weight: 400;
    font-size: 0.85rem;
    margin-left: 8px;
  }

  .seek {
    width: 100%;
  }

  .times {
    display: flex;
    gap: 12px;
    font-family: monospace;
    font-size: 0.95rem;
    align-items: baseline;
  }

  .subtle {
    color: #9aa0a6;
  }

  .buffering {
    color: #f9ab00;
  }

  .controls {
    display: flex;
    gap: 16px;
    align-items: center;
    margin-top: 8px;
    flex-wrap: wrap;
  }

  .controls label {
    display: flex;
    gap: 8px;
    align-items: center;
    font-size: 0.9rem;
    color: #c8c9cc;
  }

  .controls select {
    background: #2d2f33;
    color: #e8e8e8;
    border: 1px solid #3c4043;
    border-radius: 6px;
    padding: 4px;
  }

  .notice {
    margin-top: 12px;
    padding: 8px 12px;
    border-radius: 8px;
    background: #263040;
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
