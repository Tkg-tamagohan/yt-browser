/// 視聴統計タブの表示用の純粋ロジック（FR-65、仕様決定 CP）。
/// `history_stats` の秒数を「N時間M分」「M分S秒」などの和文単位に整形する。

/// 秒数を視聴時間の和文表記にする。
/// - 1 時間以上は「N時間M分」（秒は切り捨て。分が 0 なら「N時間」）
/// - 1 時間未満は「M分S秒」（秒が 0 なら「M分」）
/// - 1 分未満は「S秒」
/// - 0 以下・非有限は「0秒」
export function fmtWatchSec(sec: number | null | undefined): string {
  if (sec === null || sec === undefined || !Number.isFinite(sec) || sec <= 0) {
    return "0秒";
  }
  const s = Math.floor(sec);
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const ss = s % 60;
  if (h > 0) {
    return m > 0 ? `${h}時間${m}分` : `${h}時間`;
  }
  if (m > 0) {
    return ss > 0 ? `${m}分${ss}秒` : `${m}分`;
  }
  return `${ss}秒`;
}
