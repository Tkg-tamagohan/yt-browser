// 動画行のコンテキストメニュー（FR-78、仕様決定 DC）の純粋ロジック。
// 表示位置の決定をここに集約し、コンポーネント（VideoContextMenu）は
// 配置と開閉の配線だけを受け持つ。

/// メニューの概算寸法。開く前は未計測のため概算値で収まりを決める。
/// 実メニューはプレイリスト数で伸びるため高めに見積もる
export const CTX_MENU_WIDTH = 260;
export const CTX_MENU_HEIGHT = 320;

/// メニューとビューポート端の間に取る余白（px）
export const CTX_MENU_MARGIN = 4;

/// クリック位置（x, y）を左上とするときのメニュー表示位置を返す。
/// 右端・下端をはみ出す場合は内側へ折り返してビューポート（vw×vh）に
/// 収める。メニューがビューポートより大きい場合は端へ寄せる
/// （左上が負にならないよう 0 で止める）
export function contextMenuPos(
  x: number,
  y: number,
  vw: number,
  vh: number,
  w: number = CTX_MENU_WIDTH,
  h: number = CTX_MENU_HEIGHT,
): { x: number; y: number } {
  return {
    x: clampEdge(x, vw, w),
    y: clampEdge(y, vh, h),
  };
}

/// 1 軸分のクランプ。start に置くと end を超える場合だけ内側へ畳む
function clampEdge(start: number, view: number, size: number): number {
  if (start + size + CTX_MENU_MARGIN <= view) return start;
  return Math.max(0, view - size - CTX_MENU_MARGIN);
}
