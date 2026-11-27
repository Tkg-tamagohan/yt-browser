// 再生開始失敗イベント（player://ended の reason="error"）から
// 原因別の表示文言と導線アクションを決定する純粋ロジック（FR-95、仕様決定 EA）。

import type { MessageKey } from "./i18n";

export type Translator = (
  key: MessageKey,
  params?: Record<string, string | number>,
) => string;

export type PlaybackErrorActionKind = "open_deps" | "open_logs";

export type PlaybackErrorNotice = {
  msg: string;
  actionKind: PlaybackErrorActionKind | null;
  actionLabelKey: MessageKey | null;
};

export type PlaybackErrorPayload = {
  videoId: string;
  errorKind?: string | null;
  errorDetail?: string | null;
};

const UNAVAILABLE_REASON_KEYS: Record<string, MessageKey> = {
  private: "player.failed.reason.private",
  unavailable: "player.failed.reason.unavailable",
  members_only: "player.failed.reason.membersOnly",
  upcoming: "player.failed.reason.upcoming",
};

/// `player://ended`（`reason="error"`）からトースト表示用の文言とボタン種別を解決する。
/// - `errorKind` が未指定または `null` の場合は従来互換の汎用文言（アクションなし）を返す。
/// - 既知の 7 カテゴリは仕様決定 EA の案内文言と導線ボタンへ変換する。
/// - 7 値以外の未知の `errorKind` は `unknown` として扱い、ログフォルダ導線を付ける。
export function resolvePlaybackErrorNotice(
  ev: PlaybackErrorPayload,
  t: Translator,
): PlaybackErrorNotice {
  if (ev.errorKind == null) {
    return {
      msg: t("player.failed", { videoId: ev.videoId }),
      actionKind: null,
      actionLabelKey: null,
    };
  }

  switch (ev.errorKind) {
    case "video_unavailable": {
      const reasonKey =
        (ev.errorDetail ? UNAVAILABLE_REASON_KEYS[ev.errorDetail] : undefined) ??
        "player.failed.reason.other";
      return {
        msg: t("player.failed.videoUnavailable", { reason: t(reasonKey) }),
        actionKind: null,
        actionLabelKey: null,
      };
    }
    case "cookie_failed":
      return {
        msg: t("player.failed.cookieFailed"),
        actionKind: null,
        actionLabelKey: null,
      };
    case "blocked":
      return {
        msg: t("player.failed.blocked"),
        actionKind: "open_deps",
        actionLabelKey: "player.failed.action.openDeps",
      };
    case "js_challenge":
      return {
        msg: t("player.failed.jsChallenge"),
        actionKind: "open_deps",
        actionLabelKey: "player.failed.action.openDeps",
      };
    case "no_format":
      return {
        msg: t("player.failed.noFormat"),
        actionKind: "open_deps",
        actionLabelKey: "player.failed.action.openDeps",
      };
    case "network":
      return {
        msg: t("player.failed.network"),
        actionKind: null,
        actionLabelKey: null,
      };
    case "unknown":
    default: {
      const base = t("player.failed", { videoId: ev.videoId });
      const detail = ev.errorDetail?.trim();
      const msg = detail
        ? t("player.failed.detailLine", { base, detail })
        : base;
      return {
        msg,
        actionKind: "open_logs",
        actionLabelKey: "player.failed.action.openLogs",
      };
    }
  }
}
