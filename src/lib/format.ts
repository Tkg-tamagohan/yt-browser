// 表示用フォーマッタ（監査「フォーマッタ重複の共通化」）。
// 各ページ・パネルに散在していた同一実装をここに集約する。

import { currentLocale, t } from "$lib/i18n";

/// Intl のロケールタグ（FR-81、仕様決定 DF）。ui.language の現行値に従う
function localeTag(): string {
  return currentLocale() === "en" ? "en-US" : "ja-JP";
}

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
  return Number.isNaN(d.getTime()) ? iso : d.toLocaleString(localeTag());
}

/// チャット投稿時刻の時刻表示（チャットパネルの行内タイムスタンプ）。
/// postedAtUsec はマイクロ秒なのでミリ秒に直して解釈する。
export function fmtChatTime(usec: number): string {
  return new Date(usec / 1000).toLocaleTimeString(localeTag(), {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

/// チャット投稿時刻の日時表示（設定画面の履歴検索結果）。
export function fmtChatDateTime(usec: number): string {
  return new Date(usec / 1000).toLocaleString(localeTag(), {
    month: "numeric",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

/// 検索結果の再生数の短縮表示。
/// ja は億/万の日本語単位（億=1e8・万=1e4）、en は B/M/K の
/// 英語単位（B=1e9・M=1e6・K=1e3）で縮める（FR-81、仕様決定 DF）
export function fmtViews(n: number | null): string {
  if (n === null) return "";
  if (currentLocale() === "en") {
    if (n >= 1_000_000_000)
      return t("search.views.oku", { count: (n / 1_000_000_000).toFixed(1) });
    if (n >= 1_000_000)
      return t("search.views.man", { count: (n / 1_000_000).toFixed(1) });
    if (n >= 1_000)
      return t("search.views.count", {
        count: `${(n / 1_000).toFixed(1)}K`,
      });
    return t("search.views.count", { count: n.toLocaleString("en-US") });
  }
  if (n >= 100_000_000)
    return t("search.views.oku", { count: (n / 100_000_000).toFixed(1) });
  if (n >= 10_000)
    return t("search.views.man", { count: (n / 10_000).toFixed(1) });
  return t("search.views.count", { count: n.toLocaleString() });
}
