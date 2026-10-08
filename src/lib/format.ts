// 表示用フォーマッタ（監査「フォーマッタ重複の共通化」）。
// 各ページ・パネルに散在していた同一実装をここに集約する。

import { t } from "$lib/i18n";

/// 再生位置・動画長の "h:mm:ss" / "m:ss" 表示。
/// null/undefined は空文字、0 以下・非有限は "0:00"。
export function fmtDuration(sec: number | null | undefined): string {
  if (sec === null || sec === undefined) return "";
  if (!Number.isFinite(sec) || sec <= 0) return "0:00";
  const s = Math.floor(sec);
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const ss = s % 60;
  const mm = h > 0 ? String(m).padStart(2, "0") : String(m);
  return `${h > 0 ? h + ":" : ""}${mm}:${String(ss).padStart(2, "0")}`;
}

/// 絶対日時の表示（フィードの公開日時・ライブラリの各種日時）。
/// ISO 形式以外に "YYYY-MM-DD HH:MM:SS"（SQLite の datetime 文字列）も
/// T/Z を補完して解釈する。解釈不能なら生文字列をそのまま返す。
export function fmtDateTime(iso: string | null | undefined): string {
  if (!iso) return "";
  const d = new Date(iso.includes("T") ? iso : iso.replace(" ", "T") + "Z");
  return Number.isNaN(d.getTime()) ? iso : d.toLocaleString("ja-JP");
}

/// チャット投稿時刻の時刻表示（チャットパネルの行内タイムスタンプ）。
/// postedAtUsec はマイクロ秒なのでミリ秒に直して解釈する。
export function fmtChatTime(usec: number): string {
  return new Date(usec / 1000).toLocaleTimeString("ja-JP", {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

/// チャット投稿時刻の日時表示（設定画面の履歴検索結果）。
export function fmtChatDateTime(usec: number): string {
  return new Date(usec / 1000).toLocaleString("ja-JP", {
    month: "numeric",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

/// 検索結果の再生数の短縮表示（億/万/件の日本語単位）。
export function fmtViews(n: number | null): string {
  if (n === null) return "";
  if (n >= 100_000_000)
    return t("search.views.oku", { count: (n / 100_000_000).toFixed(1) });
  if (n >= 10_000)
    return t("search.views.man", { count: (n / 10_000).toFixed(1) });
  return t("search.views.count", { count: n.toLocaleString() });
}
