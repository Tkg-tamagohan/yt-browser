// ページ横断の一時通知ストア。フィード画面で発火した通知が再生画面への
// 遷移で消えないよう、レイアウトで描画する共有キューとする。

export interface AppNotice {
  id: number;
  msg: string;
}

export const appNotices = $state<AppNotice[]>([]);

let seq = 0;
const MAX = 5;

/// 画面下のトーストに一時通知を出す。約 6 秒で消える。
export function notify(msg: string): void {
  const id = ++seq;
  appNotices.push({ id, msg });
  if (appNotices.length > MAX) {
    appNotices.splice(0, appNotices.length - MAX);
  }
  setTimeout(() => {
    const i = appNotices.findIndex((n) => n.id === id);
    if (i >= 0) appNotices.splice(i, 1);
  }, 6000);
}
