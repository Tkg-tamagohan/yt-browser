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

/// 画面下のトーストに一時通知を出す。約 6 秒で消える。
/// action 指定時はラベルボタンを併記し、押下で run を実行して閉じる。
/// 発行した通知は履歴へも記録する（FR-83。履歴にはコールバックを持たせず
/// ラベルのみ残す。設計書 §4.22）
export function notify(msg: string, action?: AppNotice["action"]): void {
  const id = ++seq;
  appNotices.push({ id, msg, action });
  if (appNotices.length > MAX) {
    appNotices.splice(0, appNotices.length - MAX);
  }
  pushNoticeHistory(noticeHistory, {
    id,
    msg,
    at: Date.now(),
    actionLabel: action?.label,
  });
  setTimeout(() => {
    const i = appNotices.findIndex((n) => n.id === id);
    if (i >= 0) appNotices.splice(i, 1);
  }, 6000);
}

/// 通知の action を実行して通知を閉じる。レイアウトのボタンから呼ぶ。
/// 履歴の記録は通知消去の影響を受けず残る（FR-83）
export function runNoticeAction(notice: AppNotice): void {
  const i = appNotices.findIndex((n) => n.id === notice.id);
  if (i >= 0) appNotices.splice(i, 1);
  notice.action?.run();
}

/// 通知履歴を消去する（設定画面の履歴一覧から呼ぶ。FR-83）。
/// 表示中のトーストには影響しない
export function clearNoticeHistory(): void {
  noticeHistory.splice(0, noticeHistory.length);
}
