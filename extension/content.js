// 再生位置の取得（FR-51、仕様決定 CB・DN）。
// 「再生位置を引き継いで開く」が押されたとき（およびポップアップが開かれた
// ときの事前確認）、ポップアップからの問い合わせに対してページ内の
// 動画要素の現在再生位置（秒）を返すコンテントスクリプト。
// 広告再生中や動画要素が無い・再生位置が取れない場合は null を返す

chrome.runtime.onMessage.addListener((msg, _sender, sendResponse) => {
  if (msg?.type !== "video_position") return;
  // インストリーム広告の再生中は広告の再生位置を本編として拾わない
  if (document.querySelector("#movie_player.ad-showing")) {
    sendResponse({ position: null });
    return;
  }
  const primary =
    document.querySelector("#movie_player video.html5-main-video") ??
    document.querySelector("ytd-reel-video-renderer[is-active] video") ??
    document.querySelector("#shorts-player video");
  if (primary && Number.isFinite(primary.currentTime)) {
    sendResponse({ position: primary.currentTime });
    return;
  }
  const videos = [...document.querySelectorAll("video")];
  // 優先要素が無い場合（YouTube Music 等）は再生中 → 再生済み → 先頭の順で選ぶ
  const v =
    videos.find((x) => !x.paused && !x.ended) ??
    videos.find((x) => x.currentTime > 0) ??
    videos[0];
  const pos = v && Number.isFinite(v.currentTime) ? v.currentTime : null;
  sendResponse({ position: pos });
});
