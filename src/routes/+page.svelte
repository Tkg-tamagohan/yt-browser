<script lang="ts">
  import { onDestroy, onMount, tick } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { t, type MessageKey } from "$lib/i18n";
  import { loadLibrary } from "$lib/library";
  import VideoActions from "$lib/VideoActions.svelte";
  import {
    initPlayerEvents,
    playerStates,
    type ChatEvent,
    type ChatStatus,
    type DbStatus,
    type PlayerAction,
    type PlayerEnded,
    type PlayerState,
    type Playlist,
    type SearchResult,
    type SponsorSkipped,
    type UiError,
    type VideoRef,
    type WatchHistory,
    type YtDlpStatus,
  } from "$lib/players.svelte";

  let dbStatus = $state<DbStatus | null>(null);
  let dbError = $state("");

  let input = $state("");
  let resumeHint = $state<WatchHistory | null>(null);
  // 再生中インスタンスの状態は共有ストア（ページ遷移で消えないようコンポーネント外に置く）
  const players = $derived(playerStates.list);
  // シークバーはドラッグ中に state 更新で暴れないよう、操作中の値を別で持つ
  let seekPreview = $state<Map<number, number>>(new Map());
  let notices = $state<string[]>([]);
  // 関連動画パネルの開閉と内容（インスタンス ID ごと）
  let related = $state<
    Map<
      number,
      { open: boolean; loading: boolean; items: SearchResult[]; req: number }
    >
  >(new Map());
  // チャットパネルの開閉と表示済みメッセージ（インスタンス ID ごと）。
  // deleted フラグは削除アクションが届いた表示行を消さず打消し表示にするためのもの
  type ChatItem = ChatEvent & { deleted?: boolean };
  let chatPanels = $state<
    Map<number, { open: boolean; items: ChatItem[]; status: string | null }>
  >(new Map());
  let ytdlp = $state<YtDlpStatus | null>(null);
  let ytdlpChecking = $state(true);
  let ytdlpUpdating = $state(false);

  // お気に入り・プレイリスト行アクション用（FR-7）
  let favIds = $state<Set<string>>(new Set());
  let playlists = $state<Playlist[]>([]);

  function videoRefOf(r: SearchResult): VideoRef {
    return {
      videoId: r.videoId,
      title: r.title,
      channelId: r.channelId,
      channelTitle: r.channelTitle,
      thumbnailUrl: r.thumbnailUrl,
    };
  }

  function onFavChange(videoId: string, faved: boolean): void {
    const next = new Set(favIds);
    if (faved) next.add(videoId);
    else next.delete(videoId);
    favIds = next;
  }

  function onPlaylistCreated(pl: Playlist): void {
    playlists = [...playlists, pl];
  }

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
      const videoId = playerStates.list.get(id)?.videoId;
      await invoke("player_close", { instanceId: id });
      // プレイヤー終了に合わせてチャット取得も止める（同動画の利用者が残る場合は維持）
      if (videoId) cleanupChatPanel(id, videoId);
      // 手動 close では player://ended が来ないため、共有マップをここで外す
      const next = new Map(playerStates.list);
      next.delete(id);
      playerStates.list = next;
      const rel = new Map(related);
      rel.delete(id);
      related = rel;
      // 閉じた時点の位置で履歴が更新されているのでヒントを取り直す
      void refreshResumeHint();
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

  function relatedPanel(id: number) {
    return related.get(id);
  }

  function chatPanel(id: number) {
    return chatPanels.get(id);
  }

  /// 同一動画の chat_start/chat_stop を直列化するキュー。
  /// invoke は到着順を保証しないため、「閉じる→すぐ開く」で
  /// stop が start の後に処理されてポーラーが死ぬ競合を防ぐ。
  const chatOpQueues = new Map<string, Promise<void>>();
  function enqueueChatOp(videoId: string, op: () => Promise<void>): void {
    const prev = chatOpQueues.get(videoId) ?? Promise.resolve();
    const next = prev.then(() => op().catch(() => {}));
    chatOpQueues.set(videoId, next);
    void next.finally(() => {
      if (chatOpQueues.get(videoId) === next) chatOpQueues.delete(videoId);
    });
  }

  /// チャットパネルの開閉。開くと chat_start、閉じると chat_stop を呼ぶ。
  /// メッセージの表示件数は直近 500 件に絞る（設計書 §6.2 の表示間引き）。
  const CHAT_CAP = 500;
  async function toggleChat(id: number, videoId: string): Promise<void> {
    const cur = chatPanels.get(id);
    const next = new Map(chatPanels);
    if (cur?.open) {
      next.set(id, { ...cur, open: false, status: null });
      chatPanels = next;
      // 同じ動画を見ている他パネルがあれば共有ポーラーは維持する
      maybeStopChat(videoId);
      return;
    }
    next.set(id, { open: true, items: cur?.items ?? [], status: null });
    chatPanels = next;
    enqueueChatOp(videoId, async () => {
      try {
        await invoke("chat_start", { videoId });
      } catch (e) {
        notify(t("player.error", { message: asErrorMessage(e) }));
      }
    });
  }

  /// `chat://message` の受信処理。videoId で対応プレイヤーへ振り分け、
  /// 削除イベントは既表示行の打消しに使う。ng / other は表示しない。
  function onChatMessages(events: ChatEvent[]): void {
    const byVideo = new Map<string, ChatEvent[]>();
    for (const e of events) {
      const arr = byVideo.get(e.videoId) ?? [];
      arr.push(e);
      byVideo.set(e.videoId, arr);
    }
    // 同一動画を複数ウィンドウで再生している場合、開いている全パネルへ配送する
    // （ポーラーは動画 ID ごとに 1 本。設計書 §6.2）
    const vidToInstances = new Map<string, number[]>();
    for (const p of playerStates.list.values()) {
      const arr = vidToInstances.get(p.videoId) ?? [];
      arr.push(p.instanceId);
      vidToInstances.set(p.videoId, arr);
    }
    const next = new Map(chatPanels);
    let changed = false;
    for (const [vid, evs] of byVideo) {
      for (const inst of vidToInstances.get(vid) ?? []) {
        const cur = next.get(inst);
        if (!cur) continue;
        const items = [...cur.items];
        const known = new Set(items.map((i) => i.itemId));
        for (const e of evs) {
          if (e.kind === "deleted") {
            const idx = items.findIndex((i) => i.itemId === e.message);
            if (idx >= 0) items[idx] = { ...items[idx], deleted: true };
            continue;
          }
          if (e.ng || e.kind === "other") continue;
          // 保存失敗後の再送などで同一 item_id が二度届きうるため表示側でも dedup
          if (e.itemId && known.has(e.itemId)) continue;
          if (e.itemId) known.add(e.itemId);
          items.push(e);
        }
        next.set(inst, { ...cur, items: items.slice(-CHAT_CAP) });
        changed = true;
      }
    }
    if (changed) {
      chatPanels = next;
      scrollChatBottom();
    }
  }

  /// 同じ動画の開いたパネルが残っていなければ共有ポーラーを止める。
  /// パネルの状態変更（閉じる・削除）を反映した後に呼ぶこと。
  function maybeStopChat(videoId: string): void {
    const stillOpen = [...chatPanels.entries()].some(
      ([inst, cp]) =>
        cp.open && playerStates.list.get(inst)?.videoId === videoId,
    );
    if (!stillOpen) {
      enqueueChatOp(videoId, async () => {
        await invoke("chat_stop", { videoId }).catch(() => {});
      });
    }
  }

  /// インスタンスのチャットパネルを閉じ、最後の利用者なら取得も止める
  function cleanupChatPanel(instanceId: number, videoId: string): void {
    if (chatPanels.delete(instanceId)) {
      chatPanels = new Map(chatPanels);
    }
    maybeStopChat(videoId);
  }

  function scrollChatBottom(): void {
    void tick().then(() => {
      for (const el of document.querySelectorAll(".chat-list")) {
        el.scrollTop = el.scrollHeight;
      }
    });
  }

  /// `chat://status` の受信処理。対象動画を開いている全パネルに状態行を出し、
  /// warn 以上は通知にも出す。
  function onChatStatus(s: ChatStatus): void {
    if (s.videoId) {
      const next = new Map(chatPanels);
      let changed = false;
      for (const [inst, cp] of next) {
        if (cp.open && playerStates.list.get(inst)?.videoId === s.videoId) {
          next.set(inst, { ...cp, status: s.message });
          changed = true;
        }
      }
      if (changed) chatPanels = next;
    }
    if (s.level !== "info") {
      notify(t("chat.status", { message: s.message }));
    }
  }

  function fmtChatTime(usec: number): string {
    const d = new Date(usec / 1000);
    return d.toLocaleTimeString("ja-JP", {
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
    });
  }

  async function toggleRelated(id: number, videoId: string): Promise<void> {
    const cur = related.get(id);
    if (cur?.open) {
      const next = new Map(related);
      next.set(id, { ...cur, open: false });
      related = next;
      return;
    }
    // 開くたびに必ず再取得する（ブロック状態の変化と前回失敗の再試行に対応するため
    // キャッシュでショートカットしない）。req は開き直しごとに増やす世代番号で、
    // 古いリクエストの応答が新しい開き直しの結果を上書きしないよう照合する。
    const req = (cur?.req ?? 0) + 1;
    const next = new Map(related);
    next.set(id, { open: true, loading: true, items: [], req });
    related = next;
    try {
      const items = await invoke<SearchResult[]>("get_related", { videoId });
      // 応答までに閉じられた・別の開き直しが始まった場合は結果を捨てる
      const now = related.get(id);
      if (!now?.open || now.req !== req) return;
      const m = new Map(related);
      m.set(id, { ...now, loading: false, items });
      related = m;
    } catch (e) {
      const now = related.get(id);
      if (!now?.open || now.req !== req) return;
      const m = new Map(related);
      m.set(id, { ...now, loading: false, items: [] });
      related = m;
      notify(t("related.failed", { message: asErrorMessage(e) }));
    }
  }

  async function playRelated(r: SearchResult): Promise<void> {
    try {
      await invoke("play_video", { videoId: r.videoId, resume: true });
    } catch (e) {
      notify(t("player.error", { message: asErrorMessage(e) }));
    }
  }

  async function blockRelated(id: number, r: SearchResult): Promise<void> {
    if (!r.channelId) return;
    try {
      await invoke("block_channel", {
        channelId: r.channelId,
        title: r.channelTitle ?? r.channelId,
      });
      const cur = related.get(id);
      if (cur) {
        const next = new Map(related);
        next.set(id, {
          ...cur,
          items: cur.items.filter((x) => x.channelId !== r.channelId),
        });
        related = next;
      }
      notify(t("blocked.added", { title: r.channelTitle ?? r.channelId }));
    } catch (e) {
      notify(t("blocked.addFailed", { message: asErrorMessage(e) }));
    }
  }

  let unlistenFns: UnlistenFn[] = [];

  onMount(async () => {
    try {
      dbStatus = await invoke<DbStatus>("db_status");
    } catch (e) {
      dbError = asErrorMessage(e);
    }
    await refreshYtDlp();
    await initPlayerEvents();
    try {
      const lib = await loadLibrary();
      favIds = lib.favIds;
      playlists = lib.playlists;
    } catch {
      // 行アクションが出せなくても再生は使えるため静かに握る
    }

    // 状態マップの更新は共有ストア側。ここでは通知とヒント更新だけを購読する
    unlistenFns.push(
      await listen<PlayerEnded>("player://ended", (ev) => {
        notify(t("player.ended", { reason: ev.payload.reason }));
        // 再生終了したインスタンスのチャットパネルも片付け、ポーラーを解放する
        cleanupChatPanel(ev.payload.instanceId, ev.payload.videoId);
        // 終了時の位置（または完了リセット）が履歴へ保存済みなのでヒントを取り直す
        void refreshResumeHint();
      }),
      await listen<ChatEvent[]>("chat://message", (ev) =>
        onChatMessages(ev.payload),
      ),
      await listen<ChatStatus>("chat://status", (ev) =>
        onChatStatus(ev.payload),
      ),
      await listen<SponsorSkipped>("sponsor://skipped", (ev) => {
        const key =
          ev.payload.action === "skip" ? "sponsor.skipped" : "sponsor.notified";
        // 設定画面と同じ日本語ラベルに揃える。未定義カテゴリは API の値のまま
        const catKey = `sponsor.cat.${ev.payload.category}` as MessageKey;
        const localized = t(catKey);
        const category =
          localized === catKey ? ev.payload.category : localized;
        notify(t(key, { category }));
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
        <button
          title={t("player.frameBackStep")}
          onclick={() => control(p.instanceId, { type: "frame_back_step" })}
        >
          ◀ 1f
        </button>
        <button
          title={t("player.frameStep")}
          onclick={() => control(p.instanceId, { type: "frame_step" })}
        >
          1f ▶
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
        <button class="link" onclick={() => toggleRelated(p.instanceId, p.videoId)}>
          {relatedPanel(p.instanceId)?.open ? t("related.hide") : t("related.show")}
        </button>
        <button class="link" onclick={() => toggleChat(p.instanceId, p.videoId)}>
          {chatPanel(p.instanceId)?.open ? t("chat.hide") : t("chat.show")}
        </button>
      </div>

      {#if chatPanel(p.instanceId)?.open}
        <div class="chat-panel">
          <h3>{t("chat.title")}</h3>
          {#if chatPanel(p.instanceId)?.status}
            <p class="chat-status">{chatPanel(p.instanceId)?.status}</p>
          {/if}
          <div class="chat-list">
            {#each chatPanel(p.instanceId)?.items ?? [] as item (item)}
              {#if item.deleted}
                <p class="chat-item deleted">
                  <span class="subtle">{t("chat.deleted")}</span>
                </p>
              {:else}
                <p class="chat-item" class:superchat={item.kind === "superchat"}>
                  <span class="chat-time">{fmtChatTime(item.postedAtUsec)}</span>
                  <span class="chat-author">
                    {item.authorName ?? t("chat.anonymous")}
                  </span>
                  {#if item.kind === "superchat" && item.amountDisplay}
                    <span class="chat-badge superchat-badge">{item.amountDisplay}</span>
                  {:else if item.kind === "membership"}
                    <span class="chat-badge">{t("chat.membership")}</span>
                  {/if}
                  <span class="chat-msg">{item.message}</span>
                </p>
              {/if}
            {/each}
            {#if (chatPanel(p.instanceId)?.items.length ?? 0) === 0}
              <p class="subtle">{t("chat.empty")}</p>
            {/if}
          </div>
        </div>
      {/if}

      {#if relatedPanel(p.instanceId)?.open}
        <div class="related">
          <h3>{t("related.title")}</h3>
          {#if relatedPanel(p.instanceId)?.loading}
            <p class="subtle">{t("related.loading")}</p>
          {:else if (relatedPanel(p.instanceId)?.items.length ?? 0) === 0}
            <p class="subtle">{t("related.empty")}</p>
          {:else}
            <ul class="related-list">
              {#each relatedPanel(p.instanceId)?.items ?? [] as r (r.videoId)}
                <li class="related-item">
                  {#if r.thumbnailUrl}
                    <img class="thumb" src={r.thumbnailUrl} alt="" />
                  {/if}
                  <div class="meta">
                    <div class="title">{r.title}</div>
                    <div class="sub">{r.channelTitle ?? ""}</div>
                    <div class="actions">
                      <button onclick={() => playRelated(r)}>{t("search.play")}</button>
                      {#if r.channelId}
                        <button
                          class="danger"
                          onclick={() => blockRelated(p.instanceId, r)}
                        >
                          {t("search.block")}
                        </button>
                      {/if}
                      <VideoActions
                        video={videoRefOf(r)}
                        faved={favIds.has(r.videoId)}
                        {playlists}
                        onfavchange={onFavChange}
                        onplaylistcreated={onPlaylistCreated}
                      />
                    </div>
                  </div>
                </li>
              {/each}
            </ul>
          {/if}
        </div>
      {/if}
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

  .buffering {
    color: #f9ab00;
  }

  .controls {
    display: flex;
    gap: 12px;
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
    padding: 4px;
    border-radius: 6px;
  }

  .related,
  .chat-panel {
    margin-top: 12px;
    border-top: 1px solid #3c4043;
    padding-top: 8px;
  }

  .chat-panel h3 {
    font-size: 1rem;
    margin: 0 0 8px;
  }

  .chat-status {
    color: #f9ab00;
    font-size: 0.85rem;
    margin: 4px 0;
  }

  .chat-list {
    max-height: 320px;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .chat-item {
    margin: 0;
    padding: 2px 0;
    font-size: 0.88rem;
    display: flex;
    gap: 8px;
    align-items: baseline;
    flex-wrap: wrap;
  }

  .chat-item.deleted {
    opacity: 0.6;
  }

  .chat-item.superchat {
    background: #3d2b1f;
    border-radius: 6px;
    padding: 2px 8px;
  }

  .chat-time {
    color: #9aa0a6;
    font-family: monospace;
    font-size: 0.78rem;
    flex-shrink: 0;
  }

  .chat-author {
    color: #8ab4f8;
    font-size: 0.8rem;
    flex-shrink: 0;
    max-width: 14em;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .chat-badge {
    background: #3c4043;
    border-radius: 4px;
    padding: 0 6px;
    font-size: 0.75rem;
    flex-shrink: 0;
  }

  .chat-badge.superchat-badge {
    background: #f9ab00;
    color: #202124;
    font-weight: 600;
  }

  .chat-msg {
    overflow-wrap: anywhere;
    min-width: 0;
  }

  .related h3 {
    font-size: 1rem;
    margin: 0 0 8px;
  }

  .related-list {
    list-style: none;
    padding: 0;
    margin: 0;
    max-height: 320px;
    overflow-y: auto;
  }

  .related-item {
    display: flex;
    gap: 10px;
    padding: 6px 0;
    align-items: flex-start;
  }

  .related-item .thumb {
    width: 120px;
    aspect-ratio: 16 / 9;
    object-fit: cover;
    border-radius: 6px;
    background: #26282c;
  }

  .related-item .meta {
    flex: 1;
    min-width: 0;
  }

  .related-item .title {
    font-size: 0.9rem;
    overflow-wrap: anywhere;
  }

  .related-item .sub {
    color: #9aa0a6;
    font-size: 0.8rem;
    margin: 2px 0 6px;
  }

  .related-item .actions {
    display: flex;
    gap: 8px;
    font-size: 0.85rem;
  }

  .related-item .actions .danger {
    color: #ff7b72;
  }

  .subtle {
    color: #9aa0a6;
    font-size: 0.85rem;
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
