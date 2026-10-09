// yt-browser opener — YouTube の動画・プレイリストを yt-browser で開く
// （yt-browser Phase 19、仕様決定 AC）
//
// - アクション（ツールバーボタン）: 動画画面・プレイリスト画面で有効。
//   対象外ページでは無効化して押せないようにする
// - 右クリックメニュー: YouTube の動画・プレイリストへのリンク上でのみ表示
// - 起動はカスタムスキーム yt-browser://open?url=<encoded> へのタブ遷移。
//   Chrome が「外部アプリで開きますか」の確認を出し、承認で yt-browser が
//   起動する（起動中なら起動中インスタンスへ URL が渡る）

const SCHEME_PREFIX = "yt-browser://open?url=";

const VIDEO_ID = /^[A-Za-z0-9_-]{11}$/;

// 対象 URL の判定: 動画（watch / youtu.be / shorts / live）または
// プレイリスト（/playlist?list=）。watch+list 複合は動画として扱う
// （アプリ側の classify と揃える）
function classify(url) {
  let u;
  try {
    u = new URL(url);
  } catch {
    return false;
  }
  if (u.protocol !== "https:" && u.protocol !== "http:") return false;
  const host = u.hostname.replace(/^www\./, "");
  if (host === "youtu.be") {
    return VIDEO_ID.test(u.pathname.split("/")[1] ?? "");
  }
  if (host !== "youtube.com" && host !== "music.youtube.com" && host !== "m.youtube.com") {
    return false;
  }
  const path = u.pathname;
  if (path === "/playlist") return !!u.searchParams.get("list");
  if (path === "/watch") return VIDEO_ID.test(u.searchParams.get("v") ?? "");
  return /^\/(shorts|live)\/[A-Za-z0-9_-]{11}/.test(path);
}

// ページ遷移でスキームを踏ませて OS 経由でアプリを起動する。
// 現タブを書き換えるとページが失われるので専用タブを開き、
// Chrome の確認ダイアログ承認後に自動で閉じる
function openInApp(targetUrl) {
  const scheme = SCHEME_PREFIX + encodeURIComponent(targetUrl);
  chrome.tabs.create({ url: scheme, active: false }, (tab) => {
    // 外部アプリ起動の確認を出すとタブは about:blank 相当に留まる。
    // 少し待って閉じる（承認/拒否どちらでも閉じてよい）
    if (tab.id !== undefined) {
      setTimeout(() => chrome.tabs.remove(tab.id), 5000);
    }
  });
}

// --- アクション（対象ページでのみ有効） ---

async function updateActionState(tabId, url) {
  const ok = typeof url === "string" && classify(url);
  if (ok) {
    await chrome.action.enable(tabId);
  } else {
    await chrome.action.disable(tabId);
  }
}

chrome.action.onClicked.addListener((tab) => {
  if (tab.url) openInApp(tab.url);
});

chrome.tabs.onUpdated.addListener((tabId, _changeInfo, tab) => {
  void updateActionState(tabId, tab.url);
});

chrome.tabs.onActivated.addListener(async ({ tabId }) => {
  const tab = await chrome.tabs.get(tabId);
  void updateActionState(tabId, tab.url);
});

// --- 右クリックメニュー（YouTube リンク上のみ） ---

chrome.runtime.onInstalled.addListener(() => {
  chrome.contextMenus.create({
    id: "open-in-yt-browser",
    title: "yt-browser で開く",
    contexts: ["link"],
    targetUrlPatterns: [
      "*://*.youtube.com/watch*",
      "*://*.youtube.com/playlist*",
      "*://*.youtube.com/shorts/*",
      "*://*.youtube.com/live/*",
      "*://youtu.be/*",
      "*://music.youtube.com/watch*",
      "*://music.youtube.com/playlist*",
    ],
  });
});

chrome.contextMenus.onClicked.addListener((info) => {
  if (info.menuItemId === "open-in-yt-browser" && info.linkUrl) {
    if (classify(info.linkUrl)) openInApp(info.linkUrl);
  }
});
