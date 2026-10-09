/// カスタムスキーム `yt-browser://open?url=<encoded>` で受け取った URL の
/// 解析と振り分け（FR-17、仕様決定 AC）。
///
/// 受信は Rust 側（src-tauri/src/deep_link.rs）が `app://open_url`
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

type OpenUrlItem = { seq: number; url: string };

/// 処理済みの配送 seq。保留ドレインとイベントで同一配送が二度届く場合の
/// 重複除去に使う。URL ではなく配送単位で識別するので、同じリンクを
/// 後で再度開く操作は新しい seq を持ち、常に処理される
const handledSeq = new Set<number>();

async function handleItem(item: OpenUrlItem): Promise<void> {
  if (handledSeq.has(item.seq)) return;
  handledSeq.add(item.seq);
  if (handledSeq.size > 200) {
    // Set は挿入順に走査するので先頭を捨てれば古い順に減らせる
    const first = handledSeq.values().next().value;
    if (first !== undefined) handledSeq.delete(first);
  }
  const target = parseOpenUrl(item.url);
  if (target === null) {
    notify(t("deeplink.unsupported"));
    return;
  }
  await dispatch(target);
}

/// deep link 処理を開始する。返した関数は解除用（$effect のクリーンアップ）。
/// 初期化は listen 登録の完了を待ってから take_open_urls を呼ぶ
/// （先に drain すると Rust 側の ready が立ち、リスナー不在のまま
/// emit だけになった URL が失われる）
export function initDeepLinks(): () => void {
  let unlisten: (() => void) | undefined;
  void (async () => {
    try {
      // イベント名はリテラルで書く（docs 整合チェックが emit/listen の
      // イベント名引数を設計書 §3.2 と照合するため定数化しない）
      unlisten = await listen<OpenUrlItem>("app://open_url", (ev) => {
        void handleItem(ev.payload);
      });
    } catch (e) {
      // リスナー登録に失敗しても保留分だけは回収を試みる
      notify(t("deeplink.playFailed", { message: asErrorMessage(e) }));
    }
    try {
      const items = await invoke<OpenUrlItem[]>("take_open_urls");
      for (const item of items) void handleItem(item);
    } catch (e) {
      notify(t("deeplink.playFailed", { message: asErrorMessage(e) }));
    }
  })();
  return () => unlisten?.();
}
