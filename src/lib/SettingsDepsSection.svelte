<script module lang="ts">
  export type YtApiAvailability =
    | "unset"
    | "active"
    | "keyInvalid"
    | "sharedQuota"
    | "searchQuota";

  export interface YtApiStatus {
    state: YtApiAvailability;
    configured: boolean;
    keyInvalid: boolean;
    sharedQuotaExhausted: boolean;
    searchQuotaExhausted: boolean;
    sharedResetMs: number | null;
    searchResetMs: number | null;
  }

  export interface YtApiTestOutcome {
    ok: boolean;
    reason: string | null;
  }
</script>

<script lang="ts">
  // 依存コマンド節（FR-72、仕様決定 CW）。
  // `ytdlp.path`（ユーザー指定の yt-dlp 実行ファイル）、
  // `ytdlp.po_token`（PO Token、FR-86・仕様決定 DK）、
  // `youtube.api_key`（YouTube Data API v3 キー、FR-97・仕様決定 EC）の入力欄を置く。
  // 保存は設定画面の保存ボタン経由（値の解決順は設計書 §5）。
  import { fmtBroadcastDateTime } from "$lib/format";
  import { t } from "$lib/i18n";

  let {
    loading,
    ytdlpPath = $bindable(),
    poToken = $bindable(),
    youtubeApiKey = $bindable(),
    ytApiStatus,
    ytApiTesting,
    ytApiTestResult,
    onTestYtApi,
    onYoutubeApiKeyInput,
  }: {
    loading: boolean;
    ytdlpPath: string;
    poToken: string;
    youtubeApiKey: string;
    ytApiStatus: YtApiStatus | null;
    ytApiTesting: boolean;
    ytApiTestResult: YtApiTestOutcome | null;
    onTestYtApi: () => void | Promise<void>;
    onYoutubeApiKeyInput?: () => void;
  } = $props();

  let showApiKey = $state(false);

  function statusLabel(status: YtApiStatus | null): string {
    switch (status?.state) {
      case "active":
        return t("settings.deps.youtubeApiKeyStatusActive");
      case "keyInvalid":
        return t("settings.deps.youtubeApiKeyStatusKeyInvalid");
      case "sharedQuota":
        return t("settings.deps.youtubeApiKeyStatusSharedQuota");
      case "searchQuota":
        return t("settings.deps.youtubeApiKeyStatusSearchQuota");
      default:
        return t("settings.deps.youtubeApiKeyStatusUnset");
    }
  }

  const resetMs = $derived.by(() => {
    if (!ytApiStatus) return null;
    if (ytApiStatus.state === "sharedQuota") return ytApiStatus.sharedResetMs;
    if (ytApiStatus.state === "searchQuota") return ytApiStatus.searchResetMs;
    return ytApiStatus.sharedResetMs ?? ytApiStatus.searchResetMs;
  });

  const resetTimeText = $derived(
    resetMs !== null ? fmtBroadcastDateTime(resetMs) : "",
  );
</script>

  <section class="panel panel--filled">
    <h2>{t("settings.deps.title")}</h2>
    <!-- 読み込み中と設定内容の出し分けはリマウントを避けるため
         hidden 切替（ヒットずれ対策、FR-80） -->
    <p class="subtle" class:hidden={!loading}>…</p>
    <div class:hidden={loading}>
      <label class="setting-row">
        {t("settings.deps.ytdlpPath")}
        <input
          type="text"
          class="path-input"
          bind:value={ytdlpPath}
          placeholder={t("settings.deps.ytdlpPath.placeholder")}
        />
      </label>
      <p class="subtle desc">{t("settings.deps.ytdlpPath.desc")}</p>
      <label class="setting-row">
        {t("settings.deps.poToken")}
        <input
          type="text"
          class="path-input"
          bind:value={poToken}
          placeholder={t("settings.deps.poToken.placeholder")}
        />
      </label>
      <p class="subtle desc">{t("settings.deps.poToken.desc")}</p>

      <div class="setting-row">
        <label for="youtube-api-key">{t("settings.deps.youtubeApiKey")}</label>
        <input
          id="youtube-api-key"
          type={showApiKey ? "text" : "password"}
          class="path-input"
          bind:value={youtubeApiKey}
          oninput={() => onYoutubeApiKeyInput?.()}
          placeholder={t("settings.deps.youtubeApiKeyPlaceholder")}
          aria-describedby="youtube-api-key-desc"
          autocomplete="off"
          autocorrect="off"
          autocapitalize="none"
          spellcheck="false"
        />
        <button
          type="button"
          class="link"
          aria-pressed={showApiKey}
          onclick={() => (showApiKey = !showApiKey)}
        >
          {showApiKey
            ? t("settings.deps.youtubeApiKeyHide")
            : t("settings.deps.youtubeApiKeyShow")}
        </button>
      </div>
      <div class="ytapi-controls">
        <span
          class="ytapi-badge"
          class:hidden={!ytApiStatus}
          data-state={ytApiStatus?.state ?? "unset"}
        >
          {statusLabel(ytApiStatus)}
        </span>
        <span class="subtle ytapi-reset" class:hidden={!resetTimeText}>
          {t("settings.deps.youtubeApiKeyResetAt", { time: resetTimeText })}
        </span>
        <button
          type="button"
          class="link"
          disabled={ytApiTesting || !youtubeApiKey.trim()}
          onclick={() => void onTestYtApi()}
        >
          {ytApiTesting
            ? t("settings.deps.youtubeApiKeyTesting")
            : t("settings.deps.youtubeApiKeyTest")}
        </button>
      </div>
      <p
        class="ytapi-test-result"
        class:hidden={!ytApiTestResult}
        class:ok={ytApiTestResult?.ok}
        class:ng={ytApiTestResult !== null && !ytApiTestResult.ok}
        role="status"
        aria-live="polite"
      >
        {#if ytApiTestResult}
          {ytApiTestResult.ok
            ? t("settings.deps.youtubeApiKeyTestOk")
            : t("settings.deps.youtubeApiKeyTestFailed", {
                reason: ytApiTestResult.reason ?? "unknown",
              })}
        {/if}
      </p>
      <p id="youtube-api-key-desc" class="subtle desc">
        {t("settings.deps.youtubeApiKeyDesc")}
      </p>
    </div>
  </section>

<style>
  .path-input {
    flex: 1;
    font-family: monospace;
    padding: 8px 12px;
    border-radius: 8px;
    border: 1px solid var(--border);
    background: var(--bg-panel);
    color: var(--fg);
  }

  .ytapi-controls {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    margin: 4px 0 6px;
  }

  .ytapi-badge {
    display: inline-block;
    padding: 2px 8px;
    border-radius: 999px;
    border: 1px solid var(--border);
    background: var(--bg-subtle);
    color: var(--fg-muted);
    font-size: 0.85rem;
  }

  .ytapi-badge[data-state="active"] {
    color: var(--success);
    border-color: var(--success);
  }

  .ytapi-badge[data-state="keyInvalid"] {
    color: var(--danger);
    border-color: var(--danger-border);
  }

  .ytapi-badge[data-state="sharedQuota"],
  .ytapi-badge[data-state="searchQuota"] {
    color: var(--warn);
    border-color: var(--warn);
  }

  .ytapi-reset {
    font-size: 0.85rem;
  }

  .ytapi-test-result {
    margin: 4px 0 6px;
    font-size: 0.85rem;
  }

  .ytapi-test-result.ok {
    color: var(--success);
  }

  .ytapi-test-result.ng {
    color: var(--danger);
  }
</style>
