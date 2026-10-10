// PiP 関連の設定値の UI 側ヘルパー（設計書 §4.5）。
// `pip.geometry` の受理形式と既定値、`pip.default` / `pip.fit_aspect` の
// 解釈の正は Rust 側（src-tauri/src/mpv/mod.rs の `is_valid_pip_geometry` /
// `DEFAULT_PIP_GEOMETRY` / `pip_default_enabled` / `pip_fit_aspect_enabled`）
// にある。ここにあるのは UI の事前チェック・表示用の同一解釈で、保存された
// 値は使われるたびに Rust 側でも再検証される（geometry の不正値は既定値へ
// フォールバック）。

/// 設定が無い・不正なときの既定値（Rust 側の既定と同じ値）。右下寄せの 480x270。
export const PIP_GEOMETRY_DEFAULT = "480x270-40-40";

const PIP_GEOMETRY_RE = /^\d{2,5}x\d{2,5}([+-]\d{1,5}[+-]\d{1,5})?$/;

/// 保存前の事前チェック（mpv geometry の `WxH` + 任意の `±x±y`）。
/// 受理の最終判定は Rust 側が行うため、ここでは同一形式だけを弾く。
export function isValidPipGeometry(s: string): boolean {
  return PIP_GEOMETRY_RE.test(s);
}

/// `pip.default` の保存値を表示用の boolean に解釈する（仕様決定 AJ）。
/// off 系（off/false/0/no、大小文字・前後空白を許容）のみ通常窓既定で、
/// 未設定・その他は PiP 既定。Rust 側 `pip_default_enabled`
/// （src-tauri/src/mpv/mod.rs）と同じ受理集合。
export function pipDefaultEnabled(raw: string | null): boolean {
  return (
    raw === null || !["off", "false", "0", "no"].includes(raw.trim().toLowerCase())
  );
}

/// `pip.fit_aspect` の保存値を表示用の boolean に解釈する（仕様決定 AL）。
/// on のとき PiP 小窓のサイズを動画のアスペクト比へ追従させる
/// （pip.geometry の WxH は上限枠として扱う）。受理集合は
/// `pip.default` と同じで、未設定・その他は追従あり。
/// Rust 側 `pip_fit_aspect_enabled`（src-tauri/src/mpv/mod.rs）と同一解釈。
export function pipFitAspectEnabled(raw: string | null): boolean {
  return pipDefaultEnabled(raw);
}

/// `chat.popup_ontop` の保存値を表示用の boolean に解釈する
/// （FR-27、仕様決定 AT）。受理集合は `pip.default` と同じで、
/// 未設定・その他は最前面 ON。Rust 側は chat_popup_open 内で
/// `pip_default_enabled` を共用して同じ解釈をする
export function chatPopupOntopEnabled(raw: string | null): boolean {
  return pipDefaultEnabled(raw);
}
