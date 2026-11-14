<script lang="ts">
  // 運用セクション（FR-62・FR-66、仕様決定 CM・CQ）。
  // - データのエクスポート/インポート（ファイルダイアログ経由）
  // - 診断情報の zip エクスポート（FR-82、仕様決定 DG）
  // - DB サイズ表示と「VACUUM」「孤児行整理」のメンテナンス
  // いずれも設定画面から即時実行する（保存ボタンは通さない）。
  // 置き換え系の操作（インポート・孤児行整理）は実行前に確認する。
  // VACUUM はデータを壊さない再構築なので確認なしで実行する
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { ask, open, save } from "@tauri-apps/plugin-dialog";
  import { t } from "$lib/i18n";
  import { diagFileName } from "$lib/diag-logic";
  import { asErrorMessage, type DbSize } from "$lib/players.svelte";

  let { notify }: { notify: (msg: string) => void } = $props();

  // DB サイズ（バイト）。取得失敗は null のままにして表示を出さない
  let dbBytes = $state<number | null>(null);
  let busy = $state(false);

  const JSON_FILTER = { name: "JSON", extensions: ["json"] };
  const ZIP_FILTER = { name: "ZIP", extensions: ["zip"] };

  /// バイト数を B/KB/MB 表記にする（運用節の目安表示用）
  function fmtBytes(n: number): string {
    if (n >= 1 << 20) return `${(n / (1 << 20)).toFixed(1)} MB`;
    if (n >= 1 << 10) return `${(n / (1 << 10)).toFixed(1)} KB`;
    return `${n} B`;
  }

  async function loadSize(): Promise<void> {
    try {
      const s = await invoke<DbSize>("db_size");
      dbBytes = s.bytes;
    } catch (e) {
      dbBytes = null;
      notify(t("settings.ops.dbSize.failed", { message: asErrorMessage(e) }));
    }
  }

  async function exportData(): Promise<void> {
    const path = await save({
      defaultPath: "yt-browser-backup.json",
      filters: [JSON_FILTER],
    });
    if (!path) return;
    busy = true;
    try {
      await invoke("data_export", { path });
      notify(t("settings.ops.exported", { path }));
    } catch (e) {
      notify(t("settings.ops.exportFailed", { message: asErrorMessage(e) }));
    } finally {
      busy = false;
    }
  }

  /// 診断情報の zip エクスポート（FR-82、仕様決定 DG、設計書 §4.20）。
  /// 保存ダイアログで選んだパスへ、ログ・設定・環境情報をマスク済みで書き出す
  async function exportDiag(): Promise<void> {
    const path = await save({
      defaultPath: diagFileName(),
      filters: [ZIP_FILTER],
    });
    if (!path) return;
    busy = true;
    try {
      await invoke("diag_export", { path });
      notify(t("settings.ops.diagExported", { path }));
    } catch (e) {
      notify(t("settings.ops.diagFailed", { message: asErrorMessage(e) }));
    } finally {
      busy = false;
    }
  }

  async function importData(): Promise<void> {
    const path = await open({ multiple: false, filters: [JSON_FILTER] });
    if (typeof path !== "string") return;
    const yes = await ask(t("settings.ops.importConfirm"), {
      title: t("settings.ops.import"),
      kind: "warning",
    });
    if (!yes) return;
    busy = true;
    try {
      await invoke("data_import", { path });
      notify(t("settings.ops.imported", { path }));
    } catch (e) {
      notify(t("settings.ops.importFailed", { message: asErrorMessage(e) }));
    } finally {
      busy = false;
    }
  }

  async function vacuum(): Promise<void> {
    busy = true;
    const before = dbBytes;
    try {
      await invoke("db_vacuum");
      await loadSize();
      notify(
        t("settings.ops.vacuumDone", {
          before: before !== null ? fmtBytes(before) : "?",
          after: dbBytes !== null ? fmtBytes(dbBytes) : "?",
        }),
      );
    } catch (e) {
      notify(t("settings.ops.vacuumFailed", { message: asErrorMessage(e) }));
    } finally {
      busy = false;
    }
  }

  async function cleanupOrphans(): Promise<void> {
    const yes = await ask(t("settings.ops.cleanupConfirm"), {
      title: t("settings.ops.cleanup"),
      kind: "warning",
    });
    if (!yes) return;
    busy = true;
    try {
      const removed = await invoke<number>("db_cleanup_orphans");
      notify(t("settings.ops.cleanupDone", { count: removed }));
      await loadSize();
    } catch (e) {
      notify(t("settings.ops.cleanupFailed", { message: asErrorMessage(e) }));
    } finally {
      busy = false;
    }
  }

  onMount(() => {
    void loadSize();
  });
</script>

<section class="panel panel--filled">
  <h2>{t("settings.ops.title")}</h2>
  <p class="subtle desc">{t("settings.ops.desc")}</p>

  <div class="ops-group">
    <div class="ops-row">
      <div class="ops-text">
        <div>{t("settings.ops.export")}</div>
        <div class="subtle">{t("settings.ops.export.desc")}</div>
      </div>
      <button onclick={() => void exportData()} disabled={busy}>
        {t("settings.ops.export")}
      </button>
    </div>
    <div class="ops-row">
      <div class="ops-text">
        <div>{t("settings.ops.import")}</div>
        <div class="subtle">{t("settings.ops.import.desc")}</div>
      </div>
      <button onclick={() => void importData()} disabled={busy}>
        {t("settings.ops.import")}
      </button>
    </div>
    <div class="ops-row">
      <div class="ops-text">
        <div>{t("settings.ops.diag")}</div>
        <div class="subtle">{t("settings.ops.diag.desc")}</div>
      </div>
      <button onclick={() => void exportDiag()} disabled={busy}>
        {t("settings.ops.diag")}
      </button>
    </div>
  </div>

  <div class="ops-group">
    <div class="ops-row">
      <div class="ops-text">
        <div>{t("settings.ops.dbSize")}</div>
        <div class="subtle">{t("settings.ops.dbSize.desc")}</div>
      </div>
      <span class="size">
        {dbBytes !== null ? fmtBytes(dbBytes) : "-"}
      </span>
    </div>
    <div class="ops-row">
      <div class="ops-text">
        <div>{t("settings.ops.vacuum")}</div>
        <div class="subtle">{t("settings.ops.vacuum.desc")}</div>
      </div>
      <button onclick={() => void vacuum()} disabled={busy}>
        {busy ? t("settings.ops.busy") : t("settings.ops.vacuum")}
      </button>
    </div>
    <div class="ops-row">
      <div class="ops-text">
        <div>{t("settings.ops.cleanup")}</div>
        <div class="subtle">{t("settings.ops.cleanup.desc")}</div>
      </div>
      <button class="danger" onclick={() => void cleanupOrphans()} disabled={busy}>
        {busy ? t("settings.ops.busy") : t("settings.ops.cleanup")}
      </button>
    </div>
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

  .ops-group {
    margin-top: 14px;
  }

  .ops-row {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 8px 0;
    border-top: 1px solid var(--border-row);
  }

  .ops-text {
    flex: 1;
    min-width: 0;
  }

  .ops-text .subtle {
    font-size: 0.8rem;
  }

  .size {
    color: var(--accent);
    font-variant-numeric: tabular-nums;
  }

  button.danger {
    border-color: var(--danger-soft-border);
    color: var(--danger-soft);
  }
</style>
