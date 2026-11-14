/// mpv 窓の前面化ボタンの可否判定（FR-42、仕様決定 BR）。
/// `windowing_system` コマンドが返す表示環境の分類を受け取る。
/// Wayland はプロトコル上窓の前面化手段が無い恒久制限（仕様決定 BU）、
/// その他の不明環境も前面化経路を持たないため無効。
export function focusWindowSupported(windowing: string): boolean {
  return windowing === "windows" || windowing === "x11";
}

/// 無効時のツールチップ用 i18n キー。対応環境（有効）では null。
/// 判定前・取得失敗（null/undefined 扱いの呼び出しはないが空文字）も
/// 無効側には倒さない方針のため、空文字は null を返す。
export function focusDisabledKey(
  windowing: string,
): "player.focus.wayland" | "player.focus.other" | null {
  if (windowing === "") return null;
  if (focusWindowSupported(windowing)) return null;
  return windowing === "wayland" ? "player.focus.wayland" : "player.focus.other";
}
