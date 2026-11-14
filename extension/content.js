// 再生位置の取得（FR-51、仕様決定 CB）。
// 「再生位置を引き継いで開く」が押されたとき（およびポップアップが開かれた
// ときの事前確認）、ポップアップからの問い合わせに対してページ内の
// 動画要素の現在再生位置（秒）を返すコンテントスクリプト。
// 動画要素が無い・再生位置が取れない場合は null を返す

chrome.runtime.onMessage.addListener((msg, _sender, sendResponse) => {
  if (msg?.type !== "video_position") return;
  const videos = [...document.querySelectorAll("video")];
  // ページ内に複数の video 要素が有り得るため（サムネイルのホバー
  // プレビュー等）、再生中 → 再生済み → 先頭の順で「見ている動画」を選ぶ
  const v =
    videos.find((x) => !x.paused && !x.ended) ??
    videos.find((x) => x.currentTime > 0) ??
    videos[0];
  const pos = v && Number.isFinite(v.currentTime) ? v.currentTime : null;
  sendResponse({ position: pos });
});
