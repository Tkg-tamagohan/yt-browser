// yt-browser opener — YouTube の動画・プレイリストを yt-browser で開く／
// 動画を一時キューへ追加する（yt-browser Phase 19・23・38、
// 仕様決定 AC・AH・AX）
//
// - アクション（ツールバーボタン）: 動画画面・プレイリスト画面で有効。
//   ポップアップで「yt-browser で開く」と「キューに追加」を選ぶ。
//   対象外ページでは無効化して押せないようにする
// - 右クリックメニュー: 「yt-browser で開く」は動画・プレイリストへの
//   リンク上で、「yt-browser のキューに追加」は動画リンク上でのみ表示
// - 起動は Native Messaging で yt-browser 本体のホストモードへ URL と
//   action（open / queue）を渡す。ホストが OS 経由で
//   yt-browser://<action>?url=<encoded> を開くので、Chrome の外部アプリ
//   確認も新規タブも出ない（起動中なら起動中インスタンスへ URL が渡る）
// - ホストが使えない（アプリ未起動で未登録など）か失敗を返したときは、
//   同じ action のスキーム方式でタブをアクティブで開き、バッジ「!」と
//   ツールチップで理由を示す

const HOST_NAME = "io.github.tkg_tamagohan.yt_browser";

const DEFAULT_TITLE = "yt-browser で開く";

// アクション別のスキームプレフィックス（native_host.rs の
// scheme_url_for と揃える）
function schemePrefix(action) {
  return `yt-browser://${action}?url=`;
}

const VIDEO_ID = /^[A-Za-z0-9_-]{11}$/;

// 対象 URL の種別: 動画（watch / youtu.be / shorts / live）は "video"、
// プレイリスト（/playlist?list=）は "playlist"、対象外は null。
// watch+list 複合は動画として扱う（アプリ側の classify と揃える）
function classifyKind(url) {
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
  return /^\/(shorts|live)\/[A-Za-z0-9_-]{11}/.test(path) ? "video" : null;
}

// 「yt-browser で開く」の対象は動画・プレイリスト両方
function classify(url) {
  return classifyKind(url) !== null;
}

// Native Messaging で本体のホストへ URL と action を渡す。応答が ok
// でなければ同じ action のスキーム方式へフォールバックする
function sendToApp(targetUrl, action) {
  chrome.runtime.sendNativeMessage(
    HOST_NAME,
    { url: targetUrl, action },
    (response) => {
      const err = chrome.runtime.lastError;
      if (!err && response && response.ok === true) {
        void setIndicator(null);
        return;
      }
      const reason = err
        ? `アプリに接続できません（${err.message}）。yt-browser を一度起動すると登録されます`
        : `アプリが失敗を返しました（${response?.error ?? "unknown"}）`;
      void setIndicator(reason);
      openViaScheme(targetUrl, action);
    },
  );
}

// 従来方式: スキームを踏ませて OS 経由でアプリを起動する。フォールバック
// でも要求された action をそのまま使う（キュー追加が開く動作に化けない）。
// 現タブを書き換えるとページが失われるので専用タブをアクティブで開き、
// Chrome の確認ダイアログをその場で承認できるようにする。
// 承認後に残るタブは少し待って閉じる（承認/拒否どちらでも閉じてよい）
function openViaScheme(targetUrl, action) {
  const scheme = schemePrefix(action) + encodeURIComponent(targetUrl);
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

// --- アクション（対象ページでのみ有効。本体の起動はポップアップが担う） ---

async function updateActionState(tabId, url) {
  const ok = typeof url === "string" && classify(url);
  if (ok) {
    await chrome.action.enable(tabId);
  } else {
    await chrome.action.disable(tabId);
  }
}

chrome.tabs.onUpdated.addListener((tabId, _changeInfo, tab) => {
  void updateActionState(tabId, tab.url);
});

chrome.tabs.onActivated.addListener(async ({ tabId }) => {
  const tab = await chrome.tabs.get(tabId);
  void updateActionState(tabId, tab.url);
});

// --- ポップアップからの要求。種別判定を返し、起動はここへ委譲させる ---
// 判定ロジックを popup.js へ複製しないよう、分類はすべてこのリスナが担う
chrome.runtime.onMessage.addListener((msg, _sender, sendResponse) => {
  if (msg?.type === "classify" && typeof msg.url === "string") {
    sendResponse({ kind: classifyKind(msg.url) });
    return;
  }
  if (msg?.type === "invoke" && typeof msg.url === "string") {
    if (msg.action === "open" && classify(msg.url)) {
      sendToApp(msg.url, "open");
    } else if (msg.action === "queue" && classifyKind(msg.url) === "video") {
      sendToApp(msg.url, "queue");
    }
  }
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
  // キュー追加は動画リンクのみ（プレイリスト単体は対象外、仕様決定 AX）
  chrome.contextMenus.create({
    id: "queue-in-yt-browser",
    title: "yt-browser のキューに追加",
    contexts: ["link"],
    targetUrlPatterns: [
      "*://*.youtube.com/watch*",
      "*://*.youtube.com/shorts/*",
      "*://*.youtube.com/live/*",
      "*://youtu.be/*",
      "*://music.youtube.com/watch*",
    ],
  });
});

chrome.contextMenus.onClicked.addListener((info) => {
  if (info.menuItemId === "open-in-yt-browser" && info.linkUrl) {
    if (classify(info.linkUrl)) sendToApp(info.linkUrl, "open");
  } else if (info.menuItemId === "queue-in-yt-browser" && info.linkUrl) {
    if (classifyKind(info.linkUrl) === "video") sendToApp(info.linkUrl, "queue");
  }
});
