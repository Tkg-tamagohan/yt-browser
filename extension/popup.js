// yt-browser opener ポップアップ（Phase 38・60、仕様決定 AX・CA・CB）。
// 現在タブの URL を background.js の classify へ問い合わせ、
// 動画なら全ボタン、プレイリストなら「yt-browser で開く」のみ、
// 対象外ではすべて無効にする。「再生位置を引き継いで開く」は
// コンテントスクリプト（content.js）へ問い合わせた再生位置が
// 使えるときだけ有効にする（FR-51）。
// プレイリストの選択肢は background 経由でホストの list_playlists から
// 取り、選んだプレイリストへ現在の動画を追加する（FR-50）。
// 起動と失敗表示は background 側に委譲する
// （ボタンを押しても popup は閉じるだけで、結果の表示はバッジが担う）

import { hasUsablePosition, withTimeParam } from "./shared.js";

const openBtn = document.getElementById("open");
const openTimeBtn = document.getElementById("openWithTime");
const queueBtn = document.getElementById("queue");
const plSelect = document.getElementById("plSelect");
const plAddBtn = document.getElementById("plAdd");

// 位置引き継ぎで開く URL。「再生位置を引き継いで開く」が有効な間だけ値を持つ
let timedUrl = null;

function invoke(action, extra = {}) {
  chrome.tabs.query({ active: true, currentWindow: true }, (tabs) => {
    // 位置引き継ぎでは t= 付きの URL を使い、それ以外は現在タブの URL
    const url = extra.url ?? tabs[0]?.url;
    if (typeof url !== "string") {
      window.close();
      return;
    }
    // sendMessage の送出を待たずに popup が閉じるとメッセージが
    // 落ちうるため、コールバックで閉じる
    chrome.runtime.sendMessage({ type: "invoke", url, action, ...extra }, () =>
      window.close(),
    );
  });
}

chrome.tabs.query({ active: true, currentWindow: true }, (tabs) => {
  const url = tabs[0]?.url;
  const tabId = tabs[0]?.id;
  if (typeof url !== "string") return;
  chrome.runtime.sendMessage({ type: "classify", url }, (res) => {
    if (chrome.runtime.lastError || !res) return;
    if (res.kind === "video" || res.kind === "playlist") {
      openBtn.disabled = false;
    }
    // キュー追加・プレイリスト追加は動画のみ（プレイリスト単体は対象外）
    if (res.kind !== "video") return;
    queueBtn.disabled = false;
    // 再生位置を引き継いで開く: 動画要素の現在位置が取れたときだけ有効。
    // コンテントスクリプトが無い・位置が取れない場合は無効のまま
    if (tabId !== undefined) {
      chrome.tabs.sendMessage(tabId, { type: "video_position" }, (pos) => {
        if (chrome.runtime.lastError || !pos) return;
        if (hasUsablePosition(pos.position)) {
          timedUrl = withTimeParam(url, pos.position);
          openTimeBtn.disabled = false;
        }
      });
    }
    // プレイリスト一覧の取得。取れない・0 件のときは追加不可のまま
    chrome.runtime.sendMessage({ type: "list_playlists" }, (pl) => {
      if (chrome.runtime.lastError || !pl) return;
      plSelect.replaceChildren();
      const list = pl.playlists;
      if (!Array.isArray(list) || list.length === 0) {
        const opt = document.createElement("option");
        opt.value = "";
        opt.textContent =
          list === null
            ? "プレイリストを取得できません"
            : "プレイリストがありません";
        plSelect.append(opt);
        return;
      }
      for (const p of list) {
        const opt = document.createElement("option");
        opt.value = String(p.id);
        opt.textContent = p.name;
        plSelect.append(opt);
      }
      plSelect.disabled = false;
      plAddBtn.disabled = false;
    });
  });
});

openBtn.addEventListener("click", () => invoke("open"));
openTimeBtn.addEventListener("click", () => {
  if (timedUrl === null) return;
  invoke("open", { url: timedUrl });
});
queueBtn.addEventListener("click", () => invoke("queue"));
plAddBtn.addEventListener("click", () => {
  const id = Number(plSelect.value);
  if (Number.isInteger(id) && id > 0) {
    invoke("playlist_add", { playlistId: id });
  }
});
