// 再生完了時の自動クローズの判定ロジック（FR-43、仕様決定 BS）。
// 解釈の正は Rust 側（src-tauri/src/mpv/mod.rs の close_on_eof_enabled /
// close_instance_on_end）にある。ここにあるのは UI 表示用の同一解釈と
// イベント側の畳み判定で、実際の窓・カードの存否はバックエンドが送る
// `player://ended` の `closed` が決める。

/// `player.close_on_eof` の保存値を表示用の boolean に解釈する（仕様決定 BS）。
/// on 系（on/true/1/yes、大小文字・前後空白を許容）のみ有効で、
/// 未設定・その他の値は既定の off（残す）。`feed.show_shorts` と同じ受理集合
export function closeOnEofEnabled(raw: string | null): boolean {
  return (
    raw !== null &&
    ["on", "true", "1", "yes"].includes(raw.trim().toLowerCase())
  );
}

/// `player://ended` の終端でインスタンスを畳むかの判定（FR-43、仕様決定 BS）。
/// Rust 側 `close_instance_on_end` の同一解釈: 継続時は残し、
/// 非継続の reason=eof だけは設定値に従う（off なら残す）。
/// eof 以外の終端は設定に関わらず畳む。
export function closeInstanceOnEnd(
  reason: string,
  continued: boolean,
  closeOnEof: boolean,
): boolean {
  if (continued) return false;
  return !(reason === "eof" && !closeOnEof);
}
