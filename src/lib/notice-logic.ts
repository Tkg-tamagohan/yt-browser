// 通知履歴の保持ロジック（FR-83、仕様決定 DH、設計書 §4.22）。
// 画面下のトースト（最大 5 件・約 6 秒で消える）とは別に、発行した通知を
// 履歴としてセッション内に保持し、設定画面の履歴一覧から振り返れるようにする。

/// 履歴 1 件。実行可能なコールバックは持たず、アクションはラベル表示に
/// 留める（履歴から操作を再実行する要件は無い。暫定解釈）
export interface NoticeHistoryEntry {
  /// 通知と共通の連番
  id: number;
  /// 表示したメッセージ本文
  msg: string;
  /// 発行時刻（Date.now() のエポックミリ秒）
  at: number;
  /// 通知に付いていたアクションのラベル（無ければ undefined）
  actionLabel?: string;
}

/// 履歴の保持上限（暫定解釈: 暫定 200 件。メモリ内のみで、
/// 超過分は古い方から落とす）
export const NOTICE_HISTORY_MAX = 200;

/// 履歴へ 1 件追加し、上限を超えた分は古い方から落とす。
/// 引数の配列をそのまま更新して返す（$state 配列へ直接使える形）。
export function pushNoticeHistory(
  history: NoticeHistoryEntry[],
  entry: NoticeHistoryEntry,
): NoticeHistoryEntry[] {
  history.push(entry);
  if (history.length > NOTICE_HISTORY_MAX) {
    history.splice(0, history.length - NOTICE_HISTORY_MAX);
  }
  return history;
}

/// 一覧表示用の並び（新しいものを先頭にする。暫定解釈）。
/// 元配列は変更しない。
export function historyDesc(
  history: readonly NoticeHistoryEntry[],
): NoticeHistoryEntry[] {
  return [...history].reverse();
}

/// 履歴行の時刻表示（HH:MM:SS、ローカル時刻）。
export function formatNoticeAt(at: number): string {
  const d = new Date(at);
  const p = (n: number) => String(n).padStart(2, "0");
  return `${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`;
}
