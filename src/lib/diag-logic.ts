// 診断情報エクスポートの UI 側ロジック（FR-82、仕様決定 DG、設計書 §4.20）。

/// 保存ダイアログへ渡す既定ファイル名
/// （`yt-browser-diag-<yyyymmdd>-<hhmmss>.zip`、ローカル時刻）。
export function diagFileName(d: Date = new Date()): string {
  const p = (n: number) => String(n).padStart(2, "0");
  const ymd = `${d.getFullYear()}${p(d.getMonth() + 1)}${p(d.getDate())}`;
  const hms = `${p(d.getHours())}${p(d.getMinutes())}${p(d.getSeconds())}`;
  return `yt-browser-diag-${ymd}-${hms}.zip`;
}
