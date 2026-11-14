// ライブ配信の待機の共有状態（FR-44、仕様決定 BT）。
// 待機リストはバックエンドの live_waits テーブルが正で、ここはその写像。
// 変更は livewait://changed を受けて live_wait_list で取り直す
// （ポーラー側の開始検出・終了・自動解除でも同じ経路で同期される）。
// 状態通知は livewait://status をトーストへ流す。
// レイアウトの初期化（initLiveWaits）で購読と初期一覧を登録し、
// 行アクションの待機中の印・待機タブの双方がこの共有状態を参照する。

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { t } from "$lib/i18n";
import { notify } from "$lib/notices.svelte";
import { asErrorMessage, type VideoRef } from "$lib/players.svelte";
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
async function refresh(): Promise<void> {
  try {
    const list = await invoke<LiveWait[]>("live_wait_list");
    liveWaits.list = list.sort(liveWaitCompare);
  } catch (e) {
    // 取り直しに失敗しても直前の表示を維持する（空に潰さない）
    notify(t("livewait.listFailed", { message: asErrorMessage(e) }));
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
              });
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

let initPromise: Promise<void> | null = null;

/// 待機イベントの購読と初期一覧の取得。レイアウト（非ポップアップ窓）で
/// 一度だけ呼ぶ。複数窓では窓ごとの JS コンテキストで購読されるが、
/// 待機の登録・解除はコマンド経由のため競合しない
export function initLiveWaits(): Promise<void> {
  if (!initPromise) {
    initPromise = (async () => {
      // 待機集合の変化は一覧を取り直して同期する
      await listen<WaitChanged>("livewait://changed", () => {
        void refresh();
      });
      // 開始検出（notify 動作・自動再生の成否）・終了・自動解除・
      // 状態確認の劣化/復帰をトーストへ流す
      await listen<WaitStatusEvent>("livewait://status", (ev) => {
        notify(t("livewait.status", { message: ev.payload.message }));
      });
      await refresh();
    })();
  }
  return initPromise;
}
