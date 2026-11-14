// チャットポップアップの表示カスタマイズ設定の解釈（FR-68、仕様決定 CS）。
// Tauri API へ触れない純粋関数だけを置き、vitest から直接検証できるようにする。
// 解釈の正は Rust 側（src-tauri/src/chat/popup.rs の
// `popup_show_author_enabled` / `popup_font_size` / `popup_opacity`）にあり、
// ここにあるのは UI 側の同一解釈（pip.ts と mpv/mod.rs の関係と同じ型）。

/// `chat.popup_show_author` の保存値を表示用の boolean に解釈する
/// （FR-68、仕様決定 CS）。on 系（on/true/1/yes、大小文字・前後空白を
/// 許容）のみ投稿者名を表示し、未設定・その他は従来どおり非表示。
/// 受理集合は `player.close_on_eof`・`feed.show_shorts` と同一で、
/// Rust 側 `popup_show_author_enabled`（src-tauri/src/chat/popup.rs）と同じ。
export function popupShowAuthorEnabled(raw: string | null): boolean {
  return (
    raw !== null && ["on", "true", "1", "yes"].includes(raw.trim().toLowerCase())
  );
}

// Rust の f64::from_str が受理する形式だけを通す数値パース。
// JS の Number() は "0x10" や "0b101" を受理してしまうため、
// 符号・10 進の整数部/小数部・指数部だけの文法に限定する
// （inf/nan 系は Rust 側でも非有限として既定値へ戻るためここでは弾く）
const FLOAT_RE = /^[+-]?(\d+\.?\d*|\.\d+)([eE][+-]?\d+)?$/;
function strictNumber(raw: string | null): number | null {
  if (raw === null || !FLOAT_RE.test(raw.trim())) {
    return null;
  }
  const n = Number(raw.trim());
  return Number.isFinite(n) ? n : null;
}

/// `chat.popup_font_size` の既定値（px、暫定）。既存の行高
/// （`.chat-item` の 0.88rem ≒ 14px）に合わせる。Rust 側の
/// `DEFAULT_POPUP_FONT_SIZE` と同じ値
export const POPUP_FONT_SIZE_DEFAULT = 14;

/// `chat.popup_font_size` の保存値を表示用の px 数値に解釈する
/// （FR-68、仕様決定 CS）。10〜32 の数値だけを受理し、
/// 未設定・空・非数・範囲外は既定値へ戻す。
/// Rust 側 `popup_font_size`（src-tauri/src/chat/popup.rs）と同一解釈。
export function popupFontSize(raw: string | null): number {
  const n = strictNumber(raw);
  if (n === null || n < 10 || n > 32) {
    return POPUP_FONT_SIZE_DEFAULT;
  }
  return n;
}

/// `chat.popup_opacity` の既定値（暫定）。既存は完全不透明なので 1.0。
/// Rust 側の `DEFAULT_POPUP_OPACITY` と同じ値
export const POPUP_OPACITY_DEFAULT = 1.0;

/// `chat.popup_opacity` の保存値を表示用の不透明度に解釈する
/// （FR-68、仕様決定 CS）。0.2〜1.0 の数値だけを受理し、
/// 未設定・空・非数・範囲外は既定値へ戻す。下限は窓を見失わない 0.2。
/// 適用先はポップアップ内容の CSS opacity（暫定解釈。窓枠ごとの
/// OS レベル半透明化は対象外）。
/// Rust 側 `popup_opacity`（src-tauri/src/chat/popup.rs）と同一解釈。
export function popupOpacity(raw: string | null): number {
  const n = strictNumber(raw);
  if (n === null || n < 0.2 || n > 1.0) {
    return POPUP_OPACITY_DEFAULT;
  }
  return n;
}
