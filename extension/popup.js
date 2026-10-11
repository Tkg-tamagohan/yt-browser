// yt-browser opener ポップアップ（Phase 38、仕様決定 AX）。
// 現在タブの URL を background.js の classify へ問い合わせ、
// 動画なら両ボタン、プレイリストなら「yt-browser で開く」のみ、
// 対象外では両方を無効にする。起動と失敗表示は background 側に委譲する
// （ボタンを押しても popup は閉じるだけで、結果の表示はバッジが担う）

const openBtn = document.getElementById("open");
const queueBtn = document.getElementById("queue");

function invoke(action) {
  chrome.tabs.query({ active: true, currentWindow: true }, (tabs) => {
    const url = tabs[0]?.url;
    if (typeof url !== "string") {
      window.close();
      return;
    }
    // sendMessage の送出を待たずに popup が閉じるとメッセージが
    // 落ちうるため、コールバックで閉じる
    chrome.runtime.sendMessage({ type: "invoke", url, action }, () =>
      window.close(),
    );
  });
}

chrome.tabs.query({ active: true, currentWindow: true }, (tabs) => {
  const url = tabs[0]?.url;
  if (typeof url !== "string") return;
  chrome.runtime.sendMessage({ type: "classify", url }, (res) => {
    if (chrome.runtime.lastError || !res) return;
    if (res.kind === "video" || res.kind === "playlist") {
      openBtn.disabled = false;
    }
    // キュー追加は動画のみ（プレイリスト単体は対象外）
    if (res.kind === "video") {
      queueBtn.disabled = false;
    }
  });
});

openBtn.addEventListener("click", () => invoke("open"));
queueBtn.addEventListener("click", () => invoke("queue"));
