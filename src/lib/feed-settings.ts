// フィード関連の設定値の UI 側ヘルパー（FR-23・FR-58・FR-64、
// 仕様決定 AP・CI・CO）。
// 各設定値の解釈の正は Rust 側（src-tauri/src/feed/mod.rs）にある。
// ここにあるのは UI の表示用の同一解釈で、実際の除外・通知・既読化は
// バックエンド側が行う。

/// on/off 系設定値の共有解釈。
/// on 系（on/true/1/yes、大小文字・前後空白を許容）のみ true で、
/// 未設定・その他の値は既定の false。
function onOffEnabled(raw: string | null): boolean {
  return (
    raw !== null &&
    ["on", "true", "1", "yes"].includes(raw.trim().toLowerCase())
  );
}

/// `feed.show_shorts` の保存値を表示用の boolean に解釈する（仕様決定 AP）。
/// on 系のみ表示で、未設定・その他の値は既定の非表示（false）。
export function feedShowShortsEnabled(raw: string | null): boolean {
  return onOffEnabled(raw);
}

/// `feed.notify_new` の保存値を表示用の boolean に解釈する（FR-58、
/// 仕様決定 CI）。on 系のみデスクトップ通知で、未設定・その他の値は
/// 既定の off（false）。
export function feedNotifyNewEnabled(raw: string | null): boolean {
  return onOffEnabled(raw);
}

/// `feed.auto_read_played` の保存値を表示用の boolean に解釈する（FR-64、
/// 仕様決定 CO）。on 系のみ再生済みの自動既読で、未設定・その他の値は
/// 既定の off（false）。
export function feedAutoReadPlayedEnabled(raw: string | null): boolean {
  return onOffEnabled(raw);
}

/// `feed.auto_read_days` の保存値を表示用の日数に解釈する（FR-64、
/// 仕様決定 CO）。1 以上の整数だけを経過日数として受理し、
/// 未設定・空・非数・0 以下・小数は既定の 0（無効）へ正規化する。
/// Rust 側（i64 パース）と受理形を揃えるため、符号付き 10 進整数の
/// 文字列に限定してから数値化する。
export function feedAutoReadDays(raw: string | null): number {
  if (raw === null) return 0;
  const s = raw.trim();
  if (!/^[+-]?\d+$/.test(s)) return 0;
  const n = Number(s);
  if (!Number.isSafeInteger(n) || n <= 0) return 0;
  return n;
}
