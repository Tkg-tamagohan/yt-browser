// 動画内ブックマーク（FR-49、仕様決定 BZ）の純粋ロジック。
// 位置表記・ラベル整形・一覧の並びはバックエンド
// （src-tauri/src/commands/bookmark.rs・db/bookmark.rs）と同一ルールの
// TS 実装で、ブックマークの表示側ルールを Vitest で検証するためここに置く。

import { fmtDuration } from "./format";

/// `video_bookmarks` の 1 行（`bookmark_list` / `bookmark_add` の戻り値要素）。
export type VideoBookmark = {
  /// 行 ID。個別削除・ラベル編集の対象指定に使う
  id: number;
  videoId: string;
  /// 保存位置（秒、整数）。再生位置の小数以下は切り捨てて保存する
  positionSec: number;
  /// 表示ラベル。保存時は位置表記（"h:mm:ss" / "m:ss"）が既定値
  label: string;
  /// 保存時刻（SQLite の datetime 文字列）
  createdAt: string;
  /// 一覧表示用に解決した動画タイトル（videos 台帳 → watch_history の順）。
  /// 未解決は null
  title: string | null;
};

/// 位置表記（"h:mm:ss" / "m:ss"）。一覧の位置表示・既定ラベル・
/// 空ラベルの復帰値に使う。Rust 側の fmt_position_label と同じ規則
/// （fmtDuration と同形式。0 以下・非有限は "0:00"）
export function fmtBookmarkSec(sec: number): string {
  if (!Number.isFinite(sec) || sec <= 0) return "0:00";
  return fmtDuration(sec);
}

/// ラベル入力の整形。前後の空白を除き、空・空白のみは位置表記へ戻す
/// （bookmark_set_label のバックエンド側ルールと同じ。暫定解釈:
/// 空ラベルの行は残さず既定表示に復帰させる）
export function normalizeBookmarkLabel(
  input: string,
  positionSec: number,
): string {
  const s = input.trim();
  return s === "" ? fmtBookmarkSec(positionSec) : s;
}

/// ブックマーク一覧の表示順。保存の新しい順（createdAt 降順、
/// 同時刻は id 降順）。`bookmark_list` の DB 順と同じ規則（暫定解釈）
export function bookmarkCompare(a: VideoBookmark, b: VideoBookmark): number {
  return b.createdAt.localeCompare(a.createdAt) || b.id - a.id;
}

/// 一覧のタイトル表示。解決できていなければ動画 ID に潰す
/// （空白表示にしないフォールバック。queueMetaOf と同じ考え方）
export function bookmarkTitle(b: VideoBookmark): string {
  return b.title || b.videoId;
}
