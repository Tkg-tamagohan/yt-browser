// FR-17・FR-29・FR-50・FR-51（仕様決定 AC・AX・CA・CB・DN）の拡張側ロジック。
// background.js（type="module"）・popup.js（type="module"）・
// Vitest（extension/shared.test.js）のすべてから import できるよう、
// DOM・chrome API に触れない純粋関数だけを置く

const VIDEO_ID = /^[A-Za-z0-9_-]{11}$/;

/**
 * アクション別のスキームプレフィックス（native_host.rs の scheme_prefix と揃える）
 */
export function schemePrefix(action) {
  return `yt-browser://${action}?url=`;
}

/**
 * 対象 URL の種別: 動画（watch / youtu.be / shorts / live）は "video"、
 * プレイリスト（/playlist?list=）は "playlist"、対象外は null。
 * watch+list 複合は動画として扱う（アプリ側の classifyTarget と揃える）
 */
export function classifyKind(url) {
  let u;
  try {
    u = new URL(url);
  } catch {
    return null;
  }
  if (u.protocol !== "https:" && u.protocol !== "http:") return null;
  const host = u.hostname.replace(/^www\./, "");
  if (host === "youtu.be") {
    return VIDEO_ID.test(u.pathname.split("/")[1] ?? "") ? "video" : null;
  }
  if (
    host !== "youtube.com" &&
    host !== "music.youtube.com" &&
    host !== "m.youtube.com"
  ) {
    return null;
  }
  const path = u.pathname;
  if (path === "/playlist") return u.searchParams.get("list") ? "playlist" : null;
  if (path === "/watch") {
    return VIDEO_ID.test(u.searchParams.get("v") ?? "") ? "video" : null;
  }
  return /^\/(shorts|live)\/[A-Za-z0-9_-]{11}(?:\/|$)/.test(path)
    ? "video"
    : null;
}

/**
 * 「yt-browser で開く」の対象は動画・プレイリスト両方
 */
export function classify(url) {
  return classifyKind(url) !== null;
}

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
