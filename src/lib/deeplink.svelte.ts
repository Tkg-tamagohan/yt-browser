/// カスタムスキーム `yt-browser://open?url=<encoded>` で受け取った URL の
/// 解析と振り分け（FR-17、仕様決定 AC）。
///
/// 受信は Rust 側（src-tauri/src/deep_link.rs）が `app://open-url`
/// イベントと `take_open_urls` の保留バッファで担う。ここではアプリ起動中の
/// 受信（listen）と起動直前に溜まった分のドレインを両方処理する。
/// 振り分けは: 動画 URL → その場で再生、プレイリスト URL → ローカル取り込み、
/// `watch`+`list` 複合は動画として扱う。対象外の URL は通知だけ出す。

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { goto } from "$app/navigation";
import { t } from "$lib/i18n";
import { notify } from "$lib/notices.svelte";
import { asErrorMessage } from "$lib/players.svelte";

const VIDEO_ID = /^[A-Za-z0-9_-]{11}$/;

export type OpenTarget =
  | { kind: "video"; videoId: string }
  | { kind: "playlist"; url: string };

/// `yt-browser:` スキーム URL を受け取り対象へ分解する。対象外なら null。
/// 受理する形は `yt-browser://open?url=...`（拡張が発行する形）と、
/// argv 経由で来る `yt-browser:open?url=...` / `yt-browser:///open?url=...`
export function parseOpenUrl(raw: string): OpenTarget | null {
  let parsed: URL;
  try {
    parsed = new URL(raw);
  } catch {
    return null;
  }
  if (parsed.protocol !== "yt-browser:") return null;
  // オーソリティ部（//open）でもパス部（:open / ///open）でも open だけを受理する
  const host = parsed.hostname;
  const path = parsed.pathname.replace(/^\/+/, "");
  const action = host === "" ? path.split("/")[0] : host;
  if (action !== "open") return null;
  const target = parsed.searchParams.get("url");
  if (!target) return null;
  return classifyTarget(target);
}

/// YouTube URL の振り分け。`watch`+`list` 複合は動画として扱う
/// （仕様決定 AC）。対象外の YouTube URL・非 YouTube URL は null。
export function classifyTarget(url: string): OpenTarget | null {
  let parsed: URL;
  try {
    parsed = new URL(url);
  } catch {
    return null;
  }
  if (parsed.protocol !== "https:" && parsed.protocol !== "http:") return null;
  const host = parsed.hostname.replace(/^www\./, "");
  const isYouTube =
    host === "youtube.com" || host === "music.youtube.com" || host === "m.youtube.com";
  if (host === "youtu.be") {
    const id = parsed.pathname.split("/")[1] ?? "";
    return VIDEO_ID.test(id) ? { kind: "video", videoId: id } : null;
  }
  if (!isYouTube) return null;
  const path = parsed.pathname;
  const list = parsed.searchParams.get("list");
  if (path === "/playlist" && list) return { kind: "playlist", url };
  if (path === "/watch") {
    const v = parsed.searchParams.get("v") ?? "";
    // watch+list 複合は動画として扱う（playlist 判定より後に置かないよう注意）
    return VIDEO_ID.test(v) ? { kind: "video", videoId: v } : null;
  }
  const short = path.match(/^\/(shorts|live)\/([A-Za-z0-9_-]{11})/);
  if (short) return { kind: "video", videoId: short[2] };
  return null;
}

/// 対象を実行する。動画はその場で再生、プレイリストは取り込み。
async function dispatch(target: OpenTarget): Promise<void> {
  if (target.kind === "video") {
    try {
      await invoke("play_video", { videoId: target.videoId, resume: false });
      goto("/");
    } catch (e) {
      notify(t("deeplink.playFailed", { message: asErrorMessage(e) }));
    }
    return;
  }
  try {
    const pl = await invoke<{ name: string; itemCount: number }>(
      "playlist_import",
      { url: target.url, name: null },
    );
    notify(
      t("library.playlist.imported", { name: pl.name, count: pl.itemCount }),
    );
    goto("/library");
  } catch (e) {
    notify(t("deeplink.importFailed", { message: asErrorMessage(e) }));
  }
}

/// 直近に処理した URL。初期ドレインが emit で処理済みの URL を
/// 再度拾う稀な競合への重複除去（小さいリングで十分）
const handled = new Set<string>();

async function handleRaw(raw: string): Promise<void> {
  if (handled.has(raw)) return;
  handled.add(raw);
  if (handled.size > 50) {
    // Set は挿入順に走査するので先頭を捨てれば古い順に減らせる
    const first = handled.values().next().value;
    if (first !== undefined) handled.delete(first);
  }
  const target = parseOpenUrl(raw);
  if (target === null) {
    notify(t("deeplink.unsupported"));
    return;
  }
  await dispatch(target);
}

/// deep link 処理を開始する。返した関数は解除用（$effect のクリーンアップ）。
export function initDeepLinks(): () => void {
  // イベント名はリテラルで書く（docs 整合チェックが emit/listen の
  // イベント名引数を設計書 §3.2 と照合するため定数化しない）
  const unlisten = listen<string>("app://open_url", (ev) => {
    void handleRaw(ev.payload);
  });
  // リスナー登録前に溜まった分を回収する（cold start や登録前受信）
  void invoke<string[]>("take_open_urls").then((urls) => {
    for (const url of urls) void handleRaw(url);
  });
  return () => {
    void unlisten.then((f) => f());
  };
}
