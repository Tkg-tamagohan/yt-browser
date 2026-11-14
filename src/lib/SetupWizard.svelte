<script lang="ts">
  // 初回セットアップの依存検出画面（FR-75、仕様決定 CZ）。
  // 設定 `setup.deps_shown` が on 系でない初回起動時だけ表示し、
  // 「はじめる」で既出フラグを立てて閉じる。一覧本体は設定画面の
  // 診断節と同じ DepsDiagList を使う。
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { t } from "$lib/i18n";
  import DepsDiagList from "$lib/DepsDiagList.svelte";

  let visible = $state(false);

  async function dismiss(): Promise<void> {
    visible = false;
    try {
      // 既出フラグ（setup.deps_shown）。次回以降は表示しない
      await invoke("settings_set", { key: "setup.deps_shown", value: "on" });
    } catch {
      // 保存失敗時は次回も表示されるだけなので握り潰す
    }
  }

  onMount(async () => {
    try {
      visible = await invoke<boolean>("setup_deps_pending");
    } catch {
      // 判定失敗時は出さない（起動を妨げない）
      visible = false;
    }
  });
</script>

{#if visible}
  <div class="setup-overlay">
    <div class="setup-dialog" role="dialog" aria-label={t("setup.title")}>
      <h2>{t("setup.title")}</h2>
      <p>{t("setup.desc")}</p>
      <DepsDiagList />
      <div class="setup-actions">
        <button class="primary" onclick={() => void dismiss()}>
          {t("setup.ok")}
        </button>
      </div>
    </div>
  </div>
{/if}

<style>
  .setup-overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.5);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 70;
  }

  .setup-dialog {
    background: #202124;
    border: 1px solid #3c4043;
    border-radius: 8px;
    padding: 20px;
    max-width: 560px;
    width: calc(100% - 48px);
  }

  .setup-dialog h2 {
    margin-top: 0;
  }

  .setup-actions {
    display: flex;
    justify-content: flex-end;
    margin-top: 16px;
  }

  .setup-actions .primary {
    font-weight: 600;
  }
</style>
