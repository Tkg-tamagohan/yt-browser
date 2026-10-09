// yt-browser opener — YouTube の動画・プレイリストを yt-browser で開く
// （yt-browser Phase 19・23、仕様決定 AC・AH）
//
// - アクション（ツールバーボタン）: 動画画面・プレイリスト画面で有効。
//   対象外ページでは無効化して押せないようにする
// - 右クリックメニュー: YouTube の動画・プレイリストへのリンク上でのみ表示
// - 起動は Native Messaging で yt-browser 本体のホストモードへ URL を渡す。
//   ホストが OS 経由で yt-browser://open?url=<encoded> を開くので、
//   Chrome の外部アプリ確認も新規タブも出ない（起動中なら起動中
//   インスタンスへ URL が渡る）
// - ホストが使えない（アプリ未起動で未登録など）か失敗を返したときは、
//   従来のスキーム方式でタブをアクティブで開き、バッジ「!」と
//   ツールチップで理由を示す

const HOST_NAME = "io.github.tkg_tamagohan.yt_browser";

const DEFAULT_TITLE = "yt-browser で開く";

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

// Native Messaging で本体のホストへ URL を渡す。応答が ok でなければ
// スキーム方式へフォールバックする
function openInApp(targetUrl) {
  chrome.runtime.sendNativeMessage(HOST_NAME, { url: targetUrl }, (response) => {
    const err = chrome.runtime.lastError;
    if (!err && response && response.ok === true) {
      void setIndicator(null);
      return;
    }
    const reason = err
      ? `アプリに接続できません（${err.message}）。yt-browser を一度起動すると登録されます`
      : `アプリが失敗を返しました（${response?.error ?? "unknown"}）`;
    void setIndicator(reason);
    openViaScheme(targetUrl);
  });
}

// 従来方式: スキームを踏ませて OS 経由でアプリを起動する。
// 現タブを書き換えるとページが失われるので専用タブをアクティブで開き、
// Chrome の確認ダイアログをその場で承認できるようにする。
// 承認後に残るタブは少し待って閉じる（承認/拒否どちらでも閉じてよい）
function openViaScheme(targetUrl) {
  const scheme = SCHEME_PREFIX + encodeURIComponent(targetUrl);
  chrome.tabs.create({ url: scheme, active: true }, (tab) => {
    if (tab.id !== undefined) {
      setTimeout(() => chrome.tabs.remove(tab.id), 5000);
    }
  });
}

// --- 失敗表示（バッジとツールチップ）。成功時は何も表示しない ---

// 応答が重なっても表示が最後の結果に収束するよう、更新を 1 本の
// Promise 連鎖で直列化する（途中で別の更新が割り込まない）
let indicatorChain = Promise.resolve();

// reason が null なら失敗表示を消し、文字列なら失敗表示にする
function setIndicator(reason) {
  indicatorChain = indicatorChain
    .then(() => (reason === null ? clearFailure() : showFailure(reason)))
    .catch((e) => console.warn("yt-browser opener: 表示を更新できません", e));
  return indicatorChain;
}

async function showFailure(reason) {
  await chrome.action.setBadgeText({ text: "!" });
  await chrome.action.setBadgeBackgroundColor({ color: "#d93025" });
  await chrome.action.setTitle({ title: `${DEFAULT_TITLE}\n前回の失敗: ${reason}` });
}

async function clearFailure() {
  await chrome.action.setBadgeText({ text: "" });
  await chrome.action.setTitle({ title: DEFAULT_TITLE });
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
