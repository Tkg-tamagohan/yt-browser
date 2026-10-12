// 「リンクをコピー」の文面組み立て（FR-34、仕様決定 BC）。
// mpv はタイトル未取得時に URL 片（`watch?v=...` や `https://...`）を
// media-title に置くため、URL 形式の値はタイトル無しとして扱う。
// その判定は履歴保存側の sanitize_media_title
// （src-tauri/src/mpv/manager.rs）と同じ手順を renderer 側へ写したもの。

/// コピー用に media-title を検証する。URL 形式の値はタイトル未取得の
/// フォールバックなので空文字へ潰す（manager.rs の sanitize_media_title と同じ）
export function sanitizeMediaTitle(title: string): string {
  const t = title.trim();
  if (t.startsWith("watch?") || t.startsWith("http://") || t.startsWith("https://")) {
    return "";
  }
  return t;
}

/// コピー文面を組み立てる。タイトルが取れている動画は
/// 「タイトル＋改行＋視聴ページ URL」の 2 行、タイトル無しは URL のみを返す。
/// 再生位置 `t=` などのパラメータは含めない
export function linkCopyText(videoId: string, mediaTitle: string): string {
  const url = `https://www.youtube.com/watch?v=${videoId}`;
  const title = sanitizeMediaTitle(mediaTitle);
  return title !== "" ? `${title}\n${url}` : url;
}
