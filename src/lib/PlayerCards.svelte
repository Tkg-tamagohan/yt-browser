<script lang="ts">
  // プレイヤーカード群の描画。
  // webkit2gtk の GPU なし環境では、カードをリマウントすると入力ヒット領域が
  // 描画位置からずれる（実機計測で確認。DOM リレイアウト系のハックでは再同期しない）
  // ため、このコンポーネントは +layout で常時マウントし、表示だけパスで切り替える。
  // チャットと関連動画のパネルは ChatPanel / RelatedPanel に切り出し、
  // チャットの状態とイベント購読は chat.svelte.ts の共有ストアが持つ。
  import { page } from "$app/state";
  import { onDestroy, onMount, untrack } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { t, type MessageKey } from "$lib/i18n";
  import { fmtDuration } from "$lib/format";
  import { QUALITY_PRESETS } from "$lib/quality";
  import { notify } from "$lib/notices.svelte";
  import ChatPanel from "$lib/ChatPanel.svelte";
  import RelatedPanel from "$lib/RelatedPanel.svelte";
  import {
    chatPanels,
    cleanupChatPanel,
    initChatEvents,
    toggleChat,
  } from "$lib/chat.svelte";
  import {
    asErrorMessage,
    formatOverrides,
    initPlayerEvents,
    playbackHooks,
    playerStates,
    type PlayerAction,
    type PlayerEnded,
    type PlayerState,
    type SponsorSkipped,
  } from "$lib/players.svelte";
  import { initQueueEvents } from "$lib/queue.svelte";

  // 再生中インスタンスの状態は共有ストア（ページ遷移で消えないようコンポーネント外に置く）
  const players = $derived(playerStates.list);
  // シークバーはドラッグ中に state 更新で暴れないよう、操作中の値を別で持つ
  let seekPreview = $state<Map<number, number>>(new Map());
  // 関連動画パネルの開閉（インスタンス ID ごと）。パネルの内容は
  // RelatedPanel が開くたびのマウントで取り直す
  let relatedOpen = $state<Set<number>>(new Set());

  // 再生中チャンネルの購読導線（FR-12）。解決結果は videoId ごとに 1 回だけ取る。
  // null は「解決不能」（yt-dlp 失敗・メタ無し）で、ボタンは無効表示のままにする
  type PlayingChannel = {
    input: string | null;
    title: string | null;
    subscribed: boolean;
  };
  let channelInfos = $state<Map<string, PlayingChannel | null>>(new Map());
  let subscribing = $state<Set<string>>(new Set());
  // fetch 要求済み videoId（channelInfos とは別管理: 同じキーの二重起動を防ぐ）。
  // プレイ中は失敗しても再要求しないが、カードが閉じられて同じ動画が
  // 再度再生されたときはエントリを外れているので再解決できる
  const channelReq = new Set<string>();

  $effect(() => {
    const active = new Set([...players.values()].map((p) => p.videoId));
    for (const id of channelReq) {
      if (!active.has(id)) channelReq.delete(id);
    }
    for (const p of players.values()) {
      if (p.videoId && !channelReq.has(p.videoId)) {
        channelReq.add(p.videoId);
        void fetchChannel(p.videoId);
      }
    }
  });

  // 「/」へ戻ったとき購読状態を取り直す（feed 画面等での購読・解除を
  // カード側にも反映させる。players の定期更新に乗らないよう
  // pathname だけを追う別エフェクトに分ける。対象は再生中のカードだけで、
  // 閉じたカードのキャッシュは再解決しない）
  $effect(() => {
    if (page.url.pathname === "/") {
      const active = new Set(
        [...untrack(() => players).values()].map((p) => p.videoId),
      );
      for (const id of active) {
        if (untrack(() => channelInfos).has(id)) void refreshChannel(id);
      }
    }
  });

  async function fetchChannel(videoId: string): Promise<void> {
    let info: PlayingChannel | null = null;
    try {
      info = await invoke<PlayingChannel>("playing_channel", { videoId });
    } catch {
      // 解決失敗は無効ボタンとして表す（トーストは出さない）
    }
    const next = new Map(channelInfos);
    next.set(videoId, info);
    channelInfos = next;
  }

  // 購読状態だけの取り直し。input は既に解決済みの値を優先して残す
  // （再取得で yt-dlp が失敗しても購読ボタンを後退させない）
  async function refreshChannel(videoId: string): Promise<void> {
    try {
      const fresh = await invoke<PlayingChannel>("playing_channel", {
        videoId,
      });
      const prev = channelInfos.get(videoId);
      const next = new Map(channelInfos);
      next.set(videoId, {
        input: fresh.input ?? prev?.input ?? null,
        title: fresh.title ?? prev?.title ?? null,
        subscribed: fresh.subscribed,
      });
      channelInfos = next;
    } catch {
      // 取り直し失敗は既存表示のまま（feed 側の操作には影響しない）
    }
  }

  async function subscribePlaying(videoId: string): Promise<void> {
    const info = channelInfos.get(videoId);
    if (!info?.input || subscribing.has(videoId)) return;
    subscribing = new Set(subscribing).add(videoId);
    try {
      const ch = await invoke<{ channel_id?: string; channelId?: string; title: string }>(
        "subscribe_channel",
        { input: info.input, categoryId: null },
      );
      notify(t("feed.subscribed", { title: ch.title }));
      // 同じ UC を入力に持つ他の再生中カードも購読済みへ
      // （@handle 形で解決中のカードは subscribe_channel 側の解決結果と
      // 突き合わせできないため対象外）
      const uc = ch.channelId ?? ch.channel_id;
      const next = new Map(channelInfos);
      for (const [vid, e] of next) {
        if (e && (vid === videoId || (uc != null && e.input === uc))) {
          next.set(vid, { ...e, subscribed: true });
        }
      }
      channelInfos = next;
    } catch (e) {
      notify(t("feed.subscribeFailed", { message: asErrorMessage(e) }));
    } finally {
      const s = new Set(subscribing);
      s.delete(videoId);
      subscribing = s;
    }
  }

  const SPEED_OPTIONS = [0.5, 0.75, 1, 1.25, 1.5, 2];

  function runPlaybackHooks(): void {
    for (const f of playbackHooks) f();
  }

  async function control(id: number, action: PlayerAction): Promise<void> {
    try {
      await invoke("player_control", { instanceId: id, action });
    } catch (e) {
      notify(t("player.error", { message: asErrorMessage(e) }));
    }
  }

  // PiP 切り替え中のインスタンス。p.pip は 300ms 間隔の状態イベントでしか
  // 更新されないため、連打されると古い状態から同じ値を二度送ってしまう。
  // 応答後も次の状態イベントが届く猶予を置いてから再有効化する
  let pipBusy = $state<Set<number>>(new Set());

  // インスタンス別画質の変更中セット。応答までの連続操作を防ぎ、
  // 失敗時は select の表示を現在の適用値へ戻す
  let qualityBusy = $state<Set<number>>(new Set());

  async function setQuality(id: number, format: string, el: HTMLSelectElement): Promise<void> {
    if (qualityBusy.has(id)) return;
    qualityBusy = new Set(qualityBusy).add(id);
    try {
      await control(id, { type: "quality", format });
      // 適用成功: このインスタンスの個別指定を記録する（設定画面の全体適用から外す）
      formatOverrides.add(id);
    } catch {
      // control がエラー通知を出す。欄の表示だけ実態へ戻す
      el.value = playerStates.list.get(id)?.format ?? "";
    } finally {
      const s = new Set(qualityBusy);
      s.delete(id);
      qualityBusy = s;
    }
  }

  async function togglePip(id: number, next: boolean): Promise<void> {
    if (pipBusy.has(id)) return;
    pipBusy = new Set(pipBusy).add(id);
    try {
      await control(id, { type: "pip", enabled: next });
    } finally {
      setTimeout(() => {
        const s = new Set(pipBusy);
        s.delete(id);
        pipBusy = s;
      }, 400);
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
      const rel = new Set(relatedOpen);
      rel.delete(id);
      relatedOpen = rel;
      formatOverrides.delete(id);
      // 閉じた時点の位置で履歴が更新されているのでページ側のヒントを取り直させる
      runPlaybackHooks();
    } catch (e) {
      notify(t("player.error", { message: asErrorMessage(e) }));
    }
  }

  function statusLabel(p: PlayerState): string {
    return p.state === "buffering" ? t("player.buffering") : "";
  }

  function chatPanel(id: number) {
    return chatPanels.list.get(id);
  }

  function toggleRelated(id: number): void {
    const next = new Set(relatedOpen);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    relatedOpen = next;
  }

  let unlistenFns: UnlistenFn[] = [];

  onMount(async () => {
    await initPlayerEvents();
    await initQueueEvents();
    await initChatEvents();

    // 状態マップの更新は共有ストア側。ここでは通知とパネルの片付けを購読する
    unlistenFns.push(
      await listen<PlayerEnded>("player://ended", (ev) => {
        notify(t("player.ended", { reason: ev.payload.reason }));
        // 再生終了したインスタンスのチャットパネルも片付け、ポーラーを解放する
        cleanupChatPanel(ev.payload.instanceId, ev.payload.videoId);
        // 関連パネルの開閉エントリも除去する（手動 close と同じ片付け。
        // ended ではカード自体はマップから消えるが、ここに残ると残存になる）
        const rel = new Set(relatedOpen);
        rel.delete(ev.payload.instanceId);
        relatedOpen = rel;
        // 終了時の位置（または完了リセット）が履歴へ保存済みなのでヒントを取り直させる
        runPlaybackHooks();
      }),
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

<!-- リマウントを避けるため、非「/」では hidden で隠すだけ（アンマウントしない） -->
<div
  class="player-cards container"
  class:hidden={page.url.pathname !== "/"}
>
  {#each [...players.values()] as p (p.instanceId)}
    {@const info = channelInfos.get(p.videoId)}
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
        <span>{fmtDuration(seekPreview.get(p.instanceId) ?? p.position)} / {p.duration > 0 ? fmtDuration(p.duration) : t("player.noDuration")}</span>
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
        <label>
          {t("player.quality")}
          <select
            value={p.format}
            title={t("player.quality.hint")}
            disabled={qualityBusy.has(p.instanceId)}
            onchange={(e) =>
              setQuality(p.instanceId, e.currentTarget.value, e.currentTarget)}
          >
            {#each QUALITY_PRESETS as q}
              <option value={q.format} selected={q.format === p.format}>
                {t(q.key)}
              </option>
            {/each}
            {#if !QUALITY_PRESETS.some((q) => q.format === p.format)}
              <option value={p.format} selected>{p.format}</option>
            {/if}
          </select>
        </label>
        <button
          title={t("player.pip.hint")}
          disabled={pipBusy.has(p.instanceId)}
          onclick={() => togglePip(p.instanceId, !p.pip)}
        >
          {p.pip ? t("player.unpip") : t("player.pip")}
        </button>
        <button class="danger" onclick={() => closePlayer(p.instanceId)}>
          {t("player.close")}
        </button>
        <button class="link" onclick={() => toggleRelated(p.instanceId)}>
          {relatedOpen.has(p.instanceId) ? t("related.hide") : t("related.show")}
        </button>
        <button class="link" onclick={() => toggleChat(p.instanceId, p.videoId)}>
          {chatPanel(p.instanceId)?.open ? t("chat.hide") : t("chat.show")}
        </button>
        {#if info?.subscribed}
          <button class="link" disabled>{t("player.subscribed")}</button>
        {:else}
          <button
            class="link"
            title={info?.title ?? undefined}
            disabled={!info?.input || subscribing.has(p.videoId)}
            onclick={() => subscribePlaying(p.videoId)}
          >
            {t("player.subscribe")}
          </button>
        {/if}
      </div>

      {#if chatPanel(p.instanceId)?.open}
        <ChatPanel instanceId={p.instanceId} />
      {/if}

      {#if relatedOpen.has(p.instanceId)}
        <RelatedPanel instanceId={p.instanceId} videoId={p.videoId} />
      {/if}
    </section>
  {/each}
</div>

<style>
  .player-cards.hidden {
    display: none;
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

  .subtle {
    color: #9aa0a6;
    font-size: 0.85rem;
  }
</style>
