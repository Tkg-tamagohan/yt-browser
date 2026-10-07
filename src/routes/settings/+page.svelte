<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { t, type MessageKey } from "$lib/i18n";
  import {
    initPlayerEvents,
    playerStates,
    type BlockedChannel,
    type ChatEvent,
    type Filter,
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

  // SponsorBlock のカテゴリ一覧（src-tauri/src/sponsor/mod.rs の設定キーに対応）
  const SPONSOR_CATEGORIES = [
    "sponsor",
    "selfpromo",
    "interaction",
    "intro",
    "outro",
    "preview",
    "poi_highlight",
    "music_offtopic",
    "filler",
  ] as const;
  type SponsorCategory = (typeof SPONSOR_CATEGORIES)[number];

  let selected = $state<string>(PRESETS[0].format);
  let customFormat = $state("");
  // カテゴリごとの動作（"skip" | "notify" | "off"）。既定は sponsor のみ skip
  let sponsorActions = $state<Record<SponsorCategory, string>>({
    ...(Object.fromEntries(SPONSOR_CATEGORIES.map((c) => [c, "off"])) as Record<
      SponsorCategory,
      string
    >),
    sponsor: "skip",
  });
  let loading = $state(true);
  let saving = $state(false);
  let notices = $state<string[]>([]);
  // DB に最後に書き込んだ画質式（読込時点では読み取った値）。
  // 書込み済みと適用完了を分けて追うため、即時適用の成否とは独立に進める
  let persistedFormat = $state("");
  // ホイール 1 ノッチの音量変化量（mpv script-opts の wheel-volume_delta。
  // 次回再生から有効。既定 2、undefined は「Lua 既定のまま」＝保存スキップ）
  let wheelDelta = $state<number | undefined>(2);
  // PiP 小窓の --geometry 値。mpv 形式（WxH + 任意の +-x+-y）だけ保存する。
  // 空欄での保存は「既定値へ戻す」操作として扱い、UI 表示も既定に戻す
  const PIP_GEOMETRY_DEFAULT = "480x270-40-40";
  let pipGeometry = $state(PIP_GEOMETRY_DEFAULT);
  const PIP_GEOMETRY_RE = /^\d{2,5}x\d{2,5}([+-]\d{1,5}[+-]\d{1,5})?$/;
  // 稼働中インスタンスへの即時適用が残っている画質式。全台に適用できたら null
  let pendingApply = $state<string | null>(null);

  // ブロック中チャンネル（FR-5: 設定画面での解除）
  let blocked = $state<BlockedChannel[]>([]);

  // NG フィルタ（FR-7）。対象・種別は DDL の CHECK と同じ値集合
  const FILTER_TARGETS = [
    "video_title",
    "video_desc",
    "channel_title",
    "channel_id",
    "chat_text",
    "chat_author",
  ] as const;
  const FILTER_KINDS = ["literal", "regex"] as const;
  let filters = $state<Filter[]>([]);
  let fTarget = $state<string>("chat_text");
  let fKind = $state<string>("literal");
  let fPattern = $state("");

  // チャット履歴検索（FR-8）
  let chatQuery = $state("");
  let chatVideoId = $state("");
  let chatResults = $state<ChatEvent[] | null>(null);
  let chatSearching = $state(false);

  async function loadFilters(): Promise<void> {
    try {
      filters = await invoke<Filter[]>("filter_list");
    } catch {
      filters = [];
    }
  }

  async function addFilter(): Promise<void> {
    const pattern = fPattern.trim();
    if (!pattern) return;
    try {
      await invoke("filter_add", {
        target: fTarget,
        kind: fKind,
        pattern,
      });
      fPattern = "";
      await loadFilters();
      notify(t("settings.filters.added"));
    } catch (e) {
      notify(t("settings.filters.addFailed", { message: asErrorMessage(e) }));
    }
  }

  async function removeFilter(f: Filter): Promise<void> {
    try {
      await invoke("filter_remove", { id: f.id });
      filters = filters.filter((x) => x.id !== f.id);
      notify(t("settings.filters.removed"));
    } catch (e) {
      notify(t("settings.filters.removeFailed", { message: asErrorMessage(e) }));
    }
  }

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

  /// フィルタ対象・種別の日本語ラベル。未定義の値は生値をそのまま出す
  /// （メッセージキー欠落時のフォールバック。sponsor カテゴリと同じ方式）。
  function filterTargetLabel(target: string): string {
    const key = `filter.target.${target}` as MessageKey;
    const s = t(key);
    return s === key ? target : s;
  }

  function filterKindLabel(kind: string): string {
    const key = `filter.kind.${kind}` as MessageKey;
    const s = t(key);
    return s === key ? kind : s;
  }

  function fmtChatTime(usec: number): string {
    return new Date(usec / 1000).toLocaleString("ja-JP", {
      month: "numeric",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    });
  }

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
    // 画質式が前回保存値から変わったときだけ適用する（カテゴリだけの保存で
    // 再生中インスタンスが再読込されないようにする。loadfile replace は再生を中断させる）
    const format = effectiveFormat;
    saving = true;
    try {
      // DB の値と違うときだけ書き込む。書いた値は必ず pendingApply にして
      // 適用成功まで追跡する（書き戻し時も適用が残る形になる）
      if (format && format !== persistedFormat) {
        await invoke("settings_set", { key: "quality.format", value: format });
        persistedFormat = format;
        pendingApply = format;
      }
      await invoke("settings_set", {
        key: "sponsor.categories",
        value: JSON.stringify(sponsorActions),
      });
      // 不正値があっても他キーの保存自体は行うが、全体の成功通知は出さない
      let invalid = false;
      // wheel.volume_delta: 有限数値・±100 以内だけ保存する
      // （mpv 側がそのまま script-opts に渡すため、ここで弾く）
      if (wheelDelta !== undefined) {
        if (!Number.isFinite(wheelDelta) || Math.abs(wheelDelta) > 100) {
          invalid = true;
          notify(
            t("settings.failed", {
              message: `wheel.volume_delta: ${wheelDelta}`,
            }),
          );
        } else {
          await invoke("settings_set", {
            key: "wheel.volume_delta",
            value: String(wheelDelta),
          });
        }
      }
      // pip.geometry: mpv の geometry 形式だけ保存する（ここで弾く）。
      // 空欄は既定値へのリセットとして扱い、不正形式は失敗通知のみ
      const geo = pipGeometry.trim();
      if (!geo) {
        // 空欄は既定値へのリセット。保存に成功してから画面値を戻す
        // （失敗時に表示と DB の値がずれないようにする）
        await invoke("settings_set", {
          key: "pip.geometry",
          value: PIP_GEOMETRY_DEFAULT,
        });
        pipGeometry = PIP_GEOMETRY_DEFAULT;
      } else if (PIP_GEOMETRY_RE.test(geo)) {
        await invoke("settings_set", { key: "pip.geometry", value: geo });
      } else {
        invalid = true;
        notify(t("settings.failed", { message: `pip.geometry: ${geo}` }));
      }
      // 画質の即時適用（pendingApply）は不正値があっても最後まで実行する。
      // 書き込み済みの画質式が適用されないまま残るのを防ぐため、成功通知だけ抑える
      if (!format || pendingApply !== format) {
        if (!invalid) {
          notify(
            format
              ? t("settings.saved")
              : t("settings.savedQualitySkipped"),
          );
        }
        return;
      }
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
        } catch (e) {
          // 終了済みインスタンスは失敗に数えない:
          // - mpv_no_instance … エントリが既に消えている
          // - 共有マップに無い … player://ended が届いて終了を反映済み
          // それ以外の IPC エラーは稼働中インスタンスへの適用失敗として記録する
          const code = (e as UiError)?.code;
          if (code !== "mpv_no_instance" && playerStates.list.has(id)) {
            failed.push(id);
          }
        }
      }
      if (failed.length > 0) {
        // 未適用の台が残るため pendingApply を残し、次回保存で適用を再試行できるようにする
        notify(t("settings.applyPartial", { count: failed.length }));
      } else {
        pendingApply = null;
        if (!invalid) {
          notify(
            applied > 0
              ? t("settings.applied", { count: applied })
              : t("settings.saved"),
          );
        }
      }
    } catch (e) {
      notify(t("settings.failed", { message: asErrorMessage(e) }));
    } finally {
      saving = false;
    }
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
      const sponsorRaw = await invoke<string | null>("settings_get", {
        key: "sponsor.categories",
      });
      const wheelRaw = await invoke<string | null>("settings_get", {
        key: "wheel.volume_delta",
      });
      const pipGeoRaw = await invoke<string | null>("settings_get", {
        key: "pip.geometry",
      });
      if (pipGeoRaw !== null && PIP_GEOMETRY_RE.test(pipGeoRaw.trim())) {
        pipGeometry = pipGeoRaw.trim();
      }
      if (wheelRaw !== null) {
        const n = Number(wheelRaw);
        if (Number.isFinite(n)) wheelDelta = n;
      }
      if (sponsorRaw) {
        try {
          const parsed = JSON.parse(sponsorRaw) as Record<string, string>;
          sponsorActions = {
            ...sponsorActions,
            ...Object.fromEntries(
              SPONSOR_CATEGORIES.filter((c) => c in parsed).map((c) => [
                c,
                parsed[c],
              ]),
            ),
          };
        } catch {
          // JSON 壊れは既定のままにする
        }
      }
    } catch {
      // 読み取り失敗時はプリセット既定のままにする
    }
    persistedFormat =
      selected === CUSTOM ? customFormat.trim() : selected;
    loading = false;
    void loadBlocked();
    void loadFilters();
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
    {/if}
  </section>

  <section class="panel">
    <h2>{t("settings.wheel.title")}</h2>
    <p class="subtle desc">{t("settings.wheel.desc")}</p>
    {#if loading}
      <p class="subtle">…</p>
    {:else}
      <label class="wheel-row">
        {t("settings.wheel.volumeDelta")}
        <input
          type="number"
          class="wheel-input"
          bind:value={wheelDelta}
          min="-100"
          max="100"
          step="1"
        />
      </label>
    {/if}
  </section>

  <section class="panel">
    <h2>{t("settings.pip.title")}</h2>
    <p class="subtle desc">{t("settings.pip.desc")}</p>
    {#if loading}
      <p class="subtle">…</p>
    {:else}
      <label class="wheel-row">
        {t("settings.pip.geometry")}
        <input
          type="text"
          class="format-input"
          bind:value={pipGeometry}
          placeholder="480x270-40-40"
        />
      </label>
    {/if}
  </section>

  <section class="panel">
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

  <section class="panel">
    <h2>{t("settings.filters.title")}</h2>
    <p class="subtle desc">{t("settings.filters.desc")}</p>
    <div class="filter-form">
      <select bind:value={fTarget} aria-label={t("settings.filters.target")}>
        {#each FILTER_TARGETS as target}
          <option value={target}>{t(`filter.target.${target}`)}</option>
        {/each}
      </select>
      <select bind:value={fKind} aria-label={t("settings.filters.kind")}>
        {#each FILTER_KINDS as kind}
          <option value={kind}>{t(`filter.kind.${kind}`)}</option>
        {/each}
      </select>
      <input
        type="text"
        class="pattern-input"
        bind:value={fPattern}
        placeholder={t("settings.filters.pattern.placeholder")}
        onkeydown={(e) => e.key === "Enter" && addFilter()}
      />
      <button onclick={addFilter} disabled={!fPattern.trim()}>
        {t("settings.filters.add")}
      </button>
    </div>
    {#if filters.length === 0}
      <p class="subtle">{t("settings.filters.empty")}</p>
    {:else}
      <ul class="filter-list">
        {#each filters as f (f.id)}
          <li>
            <span class="f-target">{filterTargetLabel(f.target)}</span>
            <span class="f-kind">{filterKindLabel(f.kind)}</span>
            <code class="f-pattern">{f.pattern}</code>
            <button class="link" onclick={() => removeFilter(f)}>
              {t("settings.filters.remove")}
            </button>
          </li>
        {/each}
      </ul>
    {/if}
  </section>

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
              <span class="chat-time">{fmtChatTime(e.postedAtUsec)}</span>
              <span class="ch-title">{e.authorName ?? "-"}</span>
              {#if e.kind !== "text"}<span class="f-kind">{e.kind}</span>{/if}
              <span class="hit-msg">{e.message}</span>
            </li>
          {/each}
        </ul>
      {/if}
    {/if}
  </section>

  <button
    class="save-btn"
    onclick={save}
    disabled={loading || saving}
  >
    {saving ? t("settings.saving") : t("settings.save")}
  </button>

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

  .wheel-row {
    display: flex;
    align-items: center;
    gap: 10px;
    margin: 12px 0;
    font-size: 0.95rem;
  }

  .wheel-input {
    width: 80px;
  }

  .panel + .panel {
    margin-top: 16px;
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

  .save-btn {
    align-self: flex-start;
    margin-top: 16px;
  }

  .filter-form {
    display: flex;
    gap: 8px;
    margin: 12px 0;
    flex-wrap: wrap;
  }

  .filter-form select {
    padding: 4px;
    border-radius: 6px;
  }

  .pattern-input {
    flex: 1;
    min-width: 200px;
  }

  .vid-input {
    width: 220px;
    font-family: monospace;
  }

  .filter-list,
  .chat-hits {
    list-style: none;
    padding: 0;
    margin: 8px 0 0;
  }

  .filter-list li,
  .chat-hits li {
    display: flex;
    gap: 10px;
    align-items: baseline;
    padding: 4px 0;
    font-size: 0.9rem;
  }

  .f-target {
    color: #8ab4f8;
    white-space: nowrap;
  }

  .f-kind {
    color: #9aa0a6;
    font-size: 0.8rem;
    white-space: nowrap;
  }

  .f-pattern {
    color: #e8eaed;
    overflow-wrap: anywhere;
    flex: 1;
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
