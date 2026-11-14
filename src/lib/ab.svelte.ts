// A-B リピートのインスタンス別セッション状態（FR-37、仕様決定 BM）。
// 遷移ルールは ab-logic.ts の純粋関数、mpv 側への反映は player_control の
// ab_loop_* アクション経由。状態は players.svelte.ts の abLoops マップに置き、
// プレイヤーの終了・mpv 再起動・動画切替時はそちらが掃除する。

import { invoke } from "@tauri-apps/api/core";
import { t } from "$lib/i18n";
import { notify } from "$lib/notices.svelte";
import { abAdvance, abStateFor, type AbState } from "./ab-logic";
import { abLoops, asErrorMessage } from "./players.svelte";

/// インスタンスの現在の表示状態。再生中動画の videoId を渡して解決する。
export function abLoopState(instanceId: number, videoId: string): AbState {
  return abStateFor(abLoops.list.get(instanceId), videoId);
}

/// AB ボタンの 1 押下。遷移に対応する mpv 操作を送り、成功したときだけ
/// 表示状態を更新する（失敗時は現状態を維持してエラー通知）。
export async function cycleAbLoop(
  instanceId: number,
  videoId: string,
): Promise<void> {
  const cur = abLoopState(instanceId, videoId);
  const { next, action } = abAdvance(cur);
  try {
    await invoke("player_control", { instanceId, action });
    const m = new Map(abLoops.list);
    if (next === "off") m.delete(instanceId);
    else m.set(instanceId, { videoId, state: next });
    abLoops.list = m;
  } catch (e) {
    notify(t("player.error", { message: asErrorMessage(e) }));
  }
}
