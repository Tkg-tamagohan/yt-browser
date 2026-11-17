// ライブ配信の待機の共有状態（FR-44、仕様決定 BT）。
// 待機リストはバックエンドの live_waits テーブルが正で、ここはその写像。
// 変更は livewait://changed を受けて live_wait_list で取り直す
// （ポーラー側の開始検出・終了・自動解除でも同じ経路で同期される）。
// 状態通知は livewait://status をトーストへ流す。
// レイアウトの初期化（initLiveWaits）で購読と初期一覧を登録し、
// 行アクションの待機中の印・待機タブの双方がこの共有状態を参照する。

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { t } from "$lib/i18n";
import { notify } from "$lib/notices.svelte";
import { asErrorMessage, type VideoRef } from "$lib/players.svelte";
import { navigateOnPlay } from "$lib/ui-prefs";
import {
  isWaiting as isWaitingInList,
  liveWaitCompare,
  waitAddOutcomeKey,
  type LiveWait,
  type WaitAddOutcome,
  type WaitChanged,
  type WaitStatusEvent,
} from "./livewait-logic";

/// 待機中の配信一覧（予定時刻昇順・未定は末尾）。
export const liveWaits = $state<{ list: LiveWait[] }>({ list: [] });

/// 行への待機中の印（FR-44）。登録済みかを video_id で引く。
export function isWaiting(videoId: string): boolean {
  return isWaitingInList(liveWaits.list, videoId);
}

/// 一覧の取り直し。`livewait://changed` を受けて呼ぶ。
async function refresh(): Promise<boolean> {
  try {
    const list = await invoke<LiveWait[]>("live_wait_list");
    liveWaits.list = list.sort(liveWaitCompare);
    return true;
  } catch (e) {
    // 取り直しに失敗しても直前の表示を維持する（空に潰さない）
    notify(t("livewait.listFailed", { message: asErrorMessage(e) }));
    return false;
  }
}

/// 「配信を待機」行アクション（仕様決定 BT(1)）。
/// バックエンドが InnerTube で開始前かを確認してから登録するため、
/// ここでは結果区分ごとの通知だけを行う。放送中は待機登録せず
/// 通常再生を案内する（再生アクション付きの通知）。
export async function liveWaitAdd(video: VideoRef): Promise<void> {
  try {
    const out = await invoke<WaitAddOutcome>("live_wait_add", { video });
    const action =
      out.result === "live"
        ? {
            label: t("livewait.playNow"),
            run: () => {
              void invoke("play_video", {
                videoId: video.videoId,
                resume: false,
              })
                .then(() => navigateOnPlay())
                .catch((e) =>
                  notify(t("player.error", { message: asErrorMessage(e) })),
                );
            },
          }
        : undefined;
    notify(t(waitAddOutcomeKey(out.result), { title: video.title }), action);
    // 登録後の一覧反映は livewait://changed が担う
  } catch (e) {
    notify(t("livewait.addFailed", { message: asErrorMessage(e) }));
  }
}

/// 待機の個別解除（仕様決定 BT(2)）。
export async function liveWaitRemove(videoId: string): Promise<void> {
  try {
    await invoke("live_wait_remove", { videoId });
    notify(t("livewait.removed"));
  } catch (e) {
    notify(t("livewait.removeFailed", { message: asErrorMessage(e) }));
  }
}

let listenPromise: Promise<void> | null = null;
let loaded = false;
let refreshPromise: Promise<void> | null = null;

/// 待機イベントの購読だけを登録する。購読途中で失敗したときは途中登録分を解除し、
/// 次回呼び出しで再試行できるようにする（初期一覧の成否とは分離する）。
function ensureListeners(): Promise<void> {
  if (!listenPromise) {
    const unlistens: UnlistenFn[] = [];
    let pending: Promise<void> | undefined;
    pending = (async () => {
      try {
        // 待機集合の変化は一覧を取り直して同期する
        unlistens.push(
          await listen<WaitChanged>("livewait://changed", () => {
            void refresh();
          }),
        );
        // 開始検出（notify 動作・自動再生の成否）・終了・自動解除・
        // 状態確認の劣化/復帰をトーストへ流す
        unlistens.push(
          await listen<WaitStatusEvent>("livewait://status", (ev) => {
            notify(t("livewait.status", { message: ev.payload.message }));
          }),
        );
      } catch (err) {
        for (const u of unlistens) u();
        if (listenPromise === pending) listenPromise = null;
        throw err;
      }
    })();
    listenPromise = pending;
  }
  return listenPromise;
}

/// 待機イベントの購読と初期一覧の取得。レイアウト（非ポップアップ窓）で
/// 一度だけ呼ぶ。複数窓では窓ごとの JS コンテキストで購読されるが、
/// 待機の登録・解除はコマンド経由のため競合しない。
/// 購読成功後に初期一覧の取得だけが一時失敗しても購読自体は維持し、
/// 後続の `livewait://changed` / `livewait://status` と再試行の双方を受けられるようにする
export async function initLiveWaits(): Promise<void> {
  await ensureListeners();
  if (loaded) return;
  if (!refreshPromise) {
    let pending: Promise<void> | undefined;
    pending = (async () => {
      try {
        if (await refresh()) {
          loaded = true;
        }
      } finally {
        if (refreshPromise === pending) refreshPromise = null;
      }
    })();
    refreshPromise = pending;
  }
  await refreshPromise;
}
