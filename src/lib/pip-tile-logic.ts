// PiP 窓の一括整列（FR-41、仕様決定 BQ）の整列先モニター解決と格子計算。
// 純粋関数のみ置き、呼び出し側（PlayerCards）が monitor_list・
// settings `window.screen`・PiP インスタンス一覧を集めて渡す。

import type { MonitorInfo } from "./monitors";

/// 整列先モニターの解決（FR-41、仕様決定 BQ）。`window.screen` の
/// 保存名に一致するモニターがあればそれを選び、未設定（空）・保存名が
/// 現在の列挙に無いときはプライマリへ退く。プライマリも無い・名前が
/// 取れない環境では列挙先頭を使う。モニターが 1 台も取れないときは
/// null（整列不可）を返す。
export function pipTileMonitor(
  monitors: MonitorInfo[],
  saved: string | null,
): MonitorInfo | null {
  if (monitors.length === 0) return null;
  const name = saved?.trim() ?? "";
  if (name !== "") {
    const hit = monitors.find((m) => m.name === name);
    if (hit) return hit;
  }
  return monitors.find((m) => m.primary) ?? monitors[0];
}

/// PiP 窓 count 台を並べる格子の列数と行数。
/// 列数は ceil(sqrt(n))、行数は ceil(n/cols)（暫定仕様。横長の
/// ディスプレイで正方形に近い並びになる。例: 1 台→1x1、2 台→2x1、
/// 3・4 台→2x2、5・6 台→3x2、7〜9 台→3x3）。
export function pipTileGrid(count: number): { cols: number; rows: number } {
  const n = Math.max(1, Math.floor(count));
  const cols = Math.ceil(Math.sqrt(n));
  const rows = Math.ceil(n / cols);
  return { cols, rows };
}

/// index 番目の窓に割り当てるマス目の geometry 文字列（`WxH+x+y`）。
/// 座標は対象ディスプレイ左上を原点とする相対値で、mpv が窓のある
/// ディスプレイ上の相対位置として解釈する（仕様決定 AZ(4)）。
/// マス目境界は floor(i*screen/cols) で切るため、割り切れない寸法でも
/// 隙間・重なりなく敷き詰められる（余りピクセルは末尾側の列・行へ散る）。
/// 並び順は行優先（左上→右→次の行）。
export function pipTileGeometry(
  index: number,
  cols: number,
  rows: number,
  screenW: number,
  screenH: number,
): string {
  const col = index % cols;
  const row = Math.floor(index / cols);
  const x1 = Math.floor((col * screenW) / cols);
  const x2 = Math.floor(((col + 1) * screenW) / cols);
  const y1 = Math.floor((row * screenH) / rows);
  const y2 = Math.floor(((row + 1) * screenH) / rows);
  return `${x2 - x1}x${y2 - y1}+${x1}+${y1}`;
}
