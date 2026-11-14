// 動画内ブックマーク（FR-49、仕様決定 BZ）の共有状態。
// 一覧はバックエンドの video_bookmarks テーブルが正で、ここはその写像。
// 変更はコマンド呼び出し経由に限られるため専用イベントは持たず、
// bookmarkAdd/Remove/SetLabel の各ラッパが成功後に取り直す
// （バックエンド発の変更経路は存在しない。暫定解釈）。
// レイアウトの初期化（initBookmarks）で初期一覧を取得する。

import { invoke } from "@tauri-apps/api/core";
import { t } from "$lib/i18n";
import { notify } from "$lib/notices.svelte";
import { asErrorMessage } from "$lib/players.svelte";
import {
  bookmarkCompare,
  fmtBookmarkSec,
  type VideoBookmark,
} from "./bookmark-logic";

/// ブックマーク一覧（保存の新しい順）。
export const bookmarks = $state<{ list: VideoBookmark[] }>({ list: [] });

/// 一覧の取り直し。各ミューテーションの成功後に呼ぶ。
async function refresh(): Promise<void> {
  try {
    const list = await invoke<VideoBookmark[]>("bookmark_list");
    bookmarks.list = list.sort(bookmarkCompare);
  } catch (e) {
    // 取り直しに失敗しても直前の表示を維持する（空に潰さない）
    notify(t("bookmark.listFailed", { message: asErrorMessage(e) }));
  }
}

/// プレイヤーカードの「ブックマーク」（FR-49）。現在位置を保存する
/// （バックエンドで整数秒へ切り捨て）。同位置の重複はバックエンドが
/// null を返すのでここで案内する
export async function bookmarkAdd(
  videoId: string,
  positionSec: number,
): Promise<void> {
  try {
    const saved = await invoke<VideoBookmark | null>("bookmark_add", {
      videoId,
      positionSec,
    });
    if (saved === null) {
      notify(
        t("bookmark.duplicate", { position: fmtBookmarkSec(positionSec) }),
      );
      return;
    }
    notify(
      t("bookmark.saved", { position: fmtBookmarkSec(saved.positionSec) }),
    );
    await refresh();
  } catch (e) {
    notify(t("bookmark.addFailed", { message: asErrorMessage(e) }));
  }
}

/// 個別削除（FR-49）。未登録の id でもバックエンドは正常終了する（冪等）
export async function bookmarkRemove(id: number): Promise<void> {
  try {
    await invoke("bookmark_remove", { id });
    notify(t("bookmark.removed"));
    await refresh();
  } catch (e) {
    notify(t("bookmark.removeFailed", { message: asErrorMessage(e) }));
  }
}

/// ラベルの後付け編集（FR-49）。空・空白のみはバックエンド側で
/// 位置表記へ戻される。失敗は通知だけ出し、呼び出し側の編集状態は
/// ここでは巻き戻さない
export async function bookmarkSetLabel(
  id: number,
  label: string,
): Promise<void> {
  try {
    await invoke("bookmark_set_label", { id, label });
    notify(t("bookmark.labelSaved"));
    await refresh();
  } catch (e) {
    notify(t("bookmark.labelFailed", { message: asErrorMessage(e) }));
  }
}

/// ブックマーク位置からの再生（FR-49）。play_video の start_sec が
/// URL の t= や履歴再開より優先される経路（設計書 §3.1）。
/// 失敗は呼び出し側へ投げ、通知は呼び出し側の責務とする
export async function bookmarkPlay(b: VideoBookmark): Promise<void> {
  await invoke("play_video", {
    videoId: b.videoId,
    resume: false,
    startSec: b.positionSec,
  });
}

let initPromise: Promise<void> | null = null;

/// 初期一覧の取得。レイアウト（非ポップアップ窓）で一度だけ呼ぶ
export function initBookmarks(): Promise<void> {
  if (!initPromise) {
    initPromise = refresh();
  }
  return initPromise;
}
