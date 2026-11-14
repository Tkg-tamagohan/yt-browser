// ページ横断の一時通知ストア。フィード画面で発火した通知が再生画面への
// 遷移で消えないよう、レイアウトで描画する共有キューとする。

export interface AppNotice {
  id: number;
  msg: string;
  /// 任意の追加アクション（スクリーンショットの「フォルダを開く」など）。
  /// 押下で run を実行して通知を閉じる（FR-38、仕様決定 BN）。
  action?: { label: string; run: () => void };
}

export const appNotices = $state<AppNotice[]>([]);

let seq = 0;
const MAX = 5;

/// 画面下のトーストに一時通知を出す。約 6 秒で消える。
/// action 指定時はラベルボタンを併記し、押下で run を実行して閉じる。
export function notify(msg: string, action?: AppNotice["action"]): void {
  const id = ++seq;
  appNotices.push({ id, msg, action });
  if (appNotices.length > MAX) {
    appNotices.splice(0, appNotices.length - MAX);
  }
  setTimeout(() => {
    const i = appNotices.findIndex((n) => n.id === id);
    if (i >= 0) appNotices.splice(i, 1);
  }, 6000);
}

/// 通知の action を実行して通知を閉じる。レイアウトのボタンから呼ぶ。
export function runNoticeAction(notice: AppNotice): void {
  const i = appNotices.findIndex((n) => n.id === notice.id);
  if (i >= 0) appNotices.splice(i, 1);
  notice.action?.run();
}
