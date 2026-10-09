//! アプリの自動更新（FR-15、仕様決定 AB）。
//! tauri-plugin-updater で `latest.json` を確認し、更新があれば確認
//! ダイアログ→ダウンロード→インストール→再起動。起動時は自動確認し、
//! 設定画面から手動確認もできる。対象は AppImage と NSIS。

import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { t } from "$lib/i18n";
import { notify } from "$lib/notices.svelte";
import { asErrorMessage } from "$lib/players.svelte";

/// 更新確認の共有状態。検出した更新は確認ダイアログと設定画面で共有する。
/// `dismissed` は「後で」を押したセッション内抑制で、起動ごとに再度出る。
export const appUpdate = $state<{
  pending: Update | null;
  checking: boolean;
  installing: boolean;
  dismissed: boolean;
}>({ pending: null, checking: false, installing: false, dismissed: false });

let startupChecked = false;

/// 更新を確認する。手動確認は結果をトースト通知し、起動時の自動確認は
/// 更新検出時のみ確認ダイアログを出す（失敗は通知しない）。
export async function checkForUpdate(manual = false): Promise<void> {
  if (appUpdate.checking) return;
  appUpdate.checking = true;
  try {
    const update = await check();
    if (update) {
      // 前回の確認結果はネイティブリソースを保持するため差し替え前に閉じる
      await appUpdate.pending?.close();
      appUpdate.pending = update;
      appUpdate.dismissed = false;
      if (manual) notify(t("update.found", { version: update.version }));
    } else if (manual) {
      notify(t("update.upToDate"));
    }
  } catch (e) {
    if (manual) notify(t("update.failed", { message: asErrorMessage(e) }));
  } finally {
    appUpdate.checking = false;
  }
}

/// 検出済みの更新を閉じて状態を畳む（「後で」押下）。
export async function dismissUpdate(): Promise<void> {
  appUpdate.dismissed = true;
  await appUpdate.pending?.close();
  appUpdate.pending = null;
}

/// 起動時の自動確認（1 セッション 1 回）。開発ビルドでは走らない
/// （latest.json が無い・署名検証が成り立たない環境で無駄な要求を出さない）。
export function checkForUpdateAtStartup(): void {
  if (startupChecked || import.meta.env.DEV) return;
  startupChecked = true;
  void checkForUpdate(false);
}

/// 検出済みの更新をダウンロード・インストールして再起動する。
export async function applyUpdate(): Promise<void> {
  const update = appUpdate.pending;
  if (!update || appUpdate.installing) return;
  appUpdate.installing = true;
  try {
    await update.downloadAndInstall();
    await relaunch();
  } catch (e) {
    appUpdate.installing = false;
    notify(t("update.installFailed", { message: asErrorMessage(e) }));
  }
}
