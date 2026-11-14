// FR-51（仕様決定 CB）の拡張側ロジック。popup.js（type="module"）と
// Vitest（extension/shared.test.js）の両方から import できるよう、
// DOM・chrome API に触れない純粋関数だけを置く

/**
 * 動画の再生位置（秒）として URL へ `t=` を付ける意味があるか。
 * 冒頭（1 秒未満）・非数は通常の「開く」と同じ挙動にするため付けない
 */
export function hasUsablePosition(pos) {
  return typeof pos === "number" && Number.isFinite(pos) && pos >= 1;
}

/**
 * URL の `t=` を秒へ付け替える。既存の `t=` は上書きし、
 * 他のクエリは温存する（アプリ側の t= 解釈は deeplink.svelte.ts の
 * parseTimeParam が担うので、ここでは整数秒を書くだけにする）
 */
export function withTimeParam(url, seconds) {
  const u = new URL(url);
  u.searchParams.set("t", String(Math.floor(seconds)));
  return u.toString();
}
