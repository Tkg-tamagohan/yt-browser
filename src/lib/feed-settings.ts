// フィード関連の設定値の UI 側ヘルパー（FR-23、仕様決定 AP）。
// `feed.show_shorts` の解釈の正は Rust 側（src-tauri/src/feed/mod.rs の
// `feed_show_shorts_enabled`）にある。ここにあるのは UI の表示用の同一解釈で、
// 実際の除外は `list_feed` がバックエンド側で行う。

/// `feed.show_shorts` の保存値を表示用の boolean に解釈する（仕様決定 AP）。
/// on 系（on/true/1/yes、大小文字・前後空白を許容）のみ表示で、
/// 未設定・その他の値は既定の非表示（false）。
export function feedShowShortsEnabled(raw: string | null): boolean {
  return (
    raw !== null &&
    ["on", "true", "1", "yes"].includes(raw.trim().toLowerCase())
  );
}
