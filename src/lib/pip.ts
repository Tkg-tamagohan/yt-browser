// PiP 小窓の設定値 `pip.geometry`（設計書 §4.5）。
// 受理形式と既定値の正は Rust 側の `is_valid_pip_geometry` /
// `DEFAULT_PIP_GEOMETRY`（src-tauri/src/mpv/mod.rs）にある。
// ここにあるのは UI の事前チェック・表示用の同一形式で、保存された値は
// PiP 化のたびに Rust 側でも再検証される（不正値は既定値へフォールバック）。

/// 設定が無い・不正なときの既定値（Rust 側の既定と同じ値）。右下寄せの 480x270。
export const PIP_GEOMETRY_DEFAULT = "480x270-40-40";

const PIP_GEOMETRY_RE = /^\d{2,5}x\d{2,5}([+-]\d{1,5}[+-]\d{1,5})?$/;

/// 保存前の事前チェック（mpv geometry の `WxH` + 任意の `±x±y`）。
/// 受理の最終判定は Rust 側が行うため、ここでは同一形式だけを弾く。
export function isValidPipGeometry(s: string): boolean {
  return PIP_GEOMETRY_RE.test(s);
}
