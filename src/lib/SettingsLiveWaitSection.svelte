<script lang="ts">
  // ライブ待機セクション（FR-44、仕様決定 BT(3)）。live_wait.action で
  // 配信開始の検出時に自動再生するかトースト通知のみにするかを選ぶ。
  // 既定は play（未設定・その他の値は play として扱う）
  import { t } from "$lib/i18n";

  let {
    loading,
    liveWaitAction = $bindable(),
  }: {
    loading: boolean;
    liveWaitAction: string;
  } = $props();
</script>

<section class="panel panel--filled">
  <h2>{t("settings.livewait.title")}</h2>
  <p class="subtle desc">{t("settings.livewait.desc")}</p>
  <!-- 読み込み中と設定内容の出し分けはリマウントを避けるため
       hidden 切替（ヒットずれ対策、FR-80） -->
  <p class="subtle" class:hidden={!loading}>…</p>
  <div class:hidden={loading}>
    <label class="row">
      {t("settings.livewait.action")}
      <select bind:value={liveWaitAction}>
        <option value="play">{t("settings.livewait.action.play")}</option>
        <option value="notify">{t("settings.livewait.action.notify")}</option>
      </select>
    </label>
  </div>
</section>

<style>
  h2 {
    font-size: 1.1rem;
    margin: 0 0 4px;
  }

  .desc {
    font-size: 0.9rem;
    margin-top: 0;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 10px;
    margin: 12px 0;
    font-size: 0.95rem;
  }
</style>
