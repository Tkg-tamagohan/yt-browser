// ページ横断の一時通知ストア。フィード画面で発火した通知が再生画面への
// 遷移で消えないよう、レイアウトで描画する共有キューとする。
// FR-83（仕様決定 DH）では発行した通知を履歴（noticeHistory）としても
// 保持し、設定画面の履歴一覧から振り返れるようにする。

import {
  pushNoticeHistory,
  type NoticeHistoryEntry,
} from "$lib/notice-logic";

export interface AppNotice {
  id: number;
  msg: string;
  /// 任意の追加アクション（スクリーンショットの「フォルダを開く」など）。
  /// 押下で run を実行して通知を閉じる（FR-38、仕様決定 BN）。
  action?: { label: string; run: () => void };
  /// 手動の閉じるボタン（×）を表示するか（FR-95、仕様決定 EA）。
  dismissible?: boolean;
}

export interface NotifyOptions {
  /// 自動消去までの時間（ミリ秒）。未指定時は既定の 6,000 ミリ秒。
  durationMs?: number;
  /// 手動の閉じるボタン（×）を付けるか。
  dismissible?: boolean;
}

export const appNotices = $state<AppNotice[]>([]);

/// 発行済み通知の履歴（FR-83、仕様決定 DH）。メモリ内のみで保持し、
/// 再起動では残らない（暫定解釈: 永続化しない。通知は DB・イベント経路を
/// 持たないフロント側の一時表示機構であり、再起動を跨ぐ記録は要求されていない）。
/// 保持上限は notice-logic の NOTICE_HISTORY_MAX（暫定）。
/// なお各ページのローカル notify（設定画面の保存通知など）はこの履歴に
/// 載らない（暫定解釈: 共有キュー経由の通知のみを対象とする）。
export const noticeHistory = $state<NoticeHistoryEntry[]>([]);

let seq = 0;
const MAX = 5;
const DEFAULT_DURATION_MS = 6000;
const noticeTimers = new Map<number, ReturnType<typeof setTimeout>>();

function clearNoticeTimer(id: number): void {
  const timer = noticeTimers.get(id);
  if (timer !== undefined) {
    clearTimeout(timer);
    noticeTimers.delete(id);
  }
}

/// 画面下のトーストに一時通知を出す。既定は約 6 秒で消える。
/// action 指定時はラベルボタンを併記し、押下で run を実行して閉じる。
/// options で表示時間延長や手動の閉じるボタン（×）を指定できる（FR-95、仕様決定 EA）。
/// 発行した通知は履歴へも記録する（FR-83。履歴にはコールバックを持たせず
/// ラベルのみ残す。設計書 §4.22）
export function notify(
  msg: string,
  action?: AppNotice["action"],
  options?: NotifyOptions,
): void {
  const id = ++seq;
  const durationMs = options?.durationMs ?? DEFAULT_DURATION_MS;
  const dismissible = options?.dismissible ?? false;
  appNotices.push({ id, msg, action, dismissible });
  if (appNotices.length > MAX) {
    const evicted = appNotices.splice(0, appNotices.length - MAX);
    for (const item of evicted) {
      clearNoticeTimer(item.id);
    }
  }
  pushNoticeHistory(noticeHistory, {
    id,
    msg,
    at: Date.now(),
    actionLabel: action?.label,
  });
  const timer = setTimeout(() => {
    noticeTimers.delete(id);
    const i = appNotices.findIndex((n) => n.id === id);
    if (i >= 0) appNotices.splice(i, 1);
  }, durationMs);
  noticeTimers.set(id, timer);
}

/// 指定 ID の通知をトーストから取り除き、自動消去タイマーを解放する（FR-95）。
/// 履歴の記録は残る。
export function dismissNotice(id: number): void {
  clearNoticeTimer(id);
  const i = appNotices.findIndex((n) => n.id === id);
  if (i >= 0) appNotices.splice(i, 1);
}

/// 通知の action を実行して通知を閉じる。レイアウトのボタンから呼ぶ。
/// 履歴の記録は通知消去の影響を受けず残る（FR-83）
export function runNoticeAction(notice: AppNotice): void {
  dismissNotice(notice.id);
  notice.action?.run();
}

/// 通知履歴を消去する（設定画面の履歴一覧から呼ぶ。FR-83）。
/// 表示中のトーストには影響しない
export function clearNoticeHistory(): void {
  noticeHistory.splice(0, noticeHistory.length);
}
