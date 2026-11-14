<script lang="ts">
  // 依存コマンド節（FR-72、仕様決定 CW）。
  // `ytdlp.path`（ユーザー指定の yt-dlp 実行ファイル）と
  // `ytdlp.po_token`（PO Token、FR-86・仕様決定 DK）の入力欄を置く。
  // 保存は設定画面の保存ボタン経由（値の解決順は設計書 §5）。
  import { t } from "$lib/i18n";

  let {
    loading,
    ytdlpPath = $bindable(),
    poToken = $bindable(),
  }: {
    loading: boolean;
    ytdlpPath: string;
    poToken: string;
  } = $props();
</script>

  <section class="panel panel--filled">
    <h2>{t("settings.deps.title")}</h2>
    <p class="subtle desc">{t("settings.deps.desc")}</p>
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
    </div>
  </section>

<style>
  .path-input {
    flex: 1;
    font-family: monospace;
  }
</style>
