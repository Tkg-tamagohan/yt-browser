// 再生開始失敗時の原因別エラー通知ロジックの回帰テスト（FR-95、仕様決定 EA）。
// PE-01: 全 7 カテゴリの文言と導線アクションの解決（ja）
// PE-02: video_unavailable の 4 理由と未知理由フォールバック
// PE-03: errorKind 未指定または null の従来文言後方互換
// PE-04: 7 値以外の未知の errorKind の unknown フォールバック
// PE-05: 英語ロケール（en）での全 7 カテゴリの文言とボタンラベル
import { describe, expect, test } from "vitest";
import { resolveMessage, type Locale, type MessageKey } from "./i18n";
import {
  resolvePlaybackErrorNotice,
  type Translator,
} from "./playback-error-logic";

function makeTranslator(locale: Locale): Translator {
  return (key: MessageKey, params?: Record<string, string | number>) => {
    const template = resolveMessage(locale, key);
    if (!params) return template;
    return template.replace(/\{(\w+)\}/g, (_, name: string) =>
      String(params[name] ?? `{${name}}`),
    );
  };
}

describe("resolvePlaybackErrorNotice", () => {
  test("PE-01: 全 7 カテゴリの文言と導線アクションを日本語で解決する", () => {
    const t = makeTranslator("ja");

    const cookie = resolvePlaybackErrorNotice(
      { videoId: "dQw4w9WgXcQ", errorKind: "cookie_failed" },
      t,
    );
    expect(cookie.msg).toContain("ブラウザの Cookie を読み取れなかった可能性");
    expect(cookie.actionKind).toBeNull();
    expect(cookie.actionLabelKey).toBeNull();

    const blocked = resolvePlaybackErrorNotice(
      { videoId: "dQw4w9WgXcQ", errorKind: "blocked" },
      t,
    );
    expect(blocked.msg).toContain("YouTube に再生を拒否された可能性");
    expect(blocked.actionKind).toBe("open_deps");
    expect(blocked.actionLabelKey).toBe("player.failed.action.openDeps");

    const jsc = resolvePlaybackErrorNotice(
      { videoId: "dQw4w9WgXcQ", errorKind: "js_challenge" },
      t,
    );
    expect(jsc.msg).toContain("deno の導入と yt-dlp の更新");
    expect(jsc.actionKind).toBe("open_deps");
    expect(jsc.actionLabelKey).toBe("player.failed.action.openDeps");

    const noFormat = resolvePlaybackErrorNotice(
      { videoId: "dQw4w9WgXcQ", errorKind: "no_format" },
      t,
    );
    expect(noFormat.msg).toContain("再生できる形式を取得できなかった可能性");
    expect(noFormat.actionKind).toBe("open_deps");
    expect(noFormat.actionLabelKey).toBe("player.failed.action.openDeps");

    const network = resolvePlaybackErrorNotice(
      { videoId: "dQw4w9WgXcQ", errorKind: "network" },
      t,
    );
    expect(network.msg).toContain("ネットワークに接続できないか");
    expect(network.actionKind).toBeNull();
    expect(network.actionLabelKey).toBeNull();

    const unknownWithDetail = resolvePlaybackErrorNotice(
      {
        videoId: "dQw4w9WgXcQ",
        errorKind: "unknown",
        errorDetail: "[youtube] unexpected failure",
      },
      t,
    );
    expect(unknownWithDetail.msg).toContain("dQw4w9WgXcQ");
    expect(unknownWithDetail.msg).toContain("\n詳細: [youtube] unexpected failure");
    expect(unknownWithDetail.actionKind).toBe("open_logs");
    expect(unknownWithDetail.actionLabelKey).toBe("player.failed.action.openLogs");

    const unknownNoDetail = resolvePlaybackErrorNotice(
      { videoId: "dQw4w9WgXcQ", errorKind: "unknown", errorDetail: "   " },
      t,
    );
    expect(unknownNoDetail.msg).toBe(t("player.failed", { videoId: "dQw4w9WgXcQ" }));
    expect(unknownNoDetail.actionKind).toBe("open_logs");
  });

  test("PE-02: video_unavailable の 4 理由と未知理由フォールバックを解決する", () => {
    const t = makeTranslator("ja");
    const cases: Array<[string | null | undefined, string]> = [
      ["private", "再生できない動画です（理由：非公開）"],
      ["unavailable", "再生できない動画です（理由：削除済み）"],
      ["members_only", "再生できない動画です（理由：メンバー限定）"],
      ["upcoming", "再生できない動画です（理由：配信開始前）"],
      ["unexpected_reason", "再生できない動画です（理由：再生制限）"],
      ["toString", "再生できない動画です（理由：再生制限）"],
      ["constructor", "再生できない動画です（理由：再生制限）"],
      [null, "再生できない動画です（理由：再生制限）"],
      [undefined, "再生できない動画です（理由：再生制限）"],
    ];

    for (const [detail, expected] of cases) {
      const res = resolvePlaybackErrorNotice(
        { videoId: "00000000000", errorKind: "video_unavailable", errorDetail: detail },
        t,
      );
      expect(res.msg).toBe(expected);
      expect(res.actionKind).toBeNull();
      expect(res.actionLabelKey).toBeNull();
    }
  });

  test("PE-03: errorKind 未指定または null では従来の汎用文言（アクションなし）へ後方互換でフォールバックする", () => {
    const t = makeTranslator("ja");
    for (const payload of [
      { videoId: "abc12345678" },
      { videoId: "abc12345678", errorKind: undefined, errorDetail: "ignored" },
      { videoId: "abc12345678", errorKind: null, errorDetail: "ignored" },
    ]) {
      const res = resolvePlaybackErrorNotice(payload, t);
      expect(res.msg).toBe(t("player.failed", { videoId: "abc12345678" }));
      expect(res.actionKind).toBeNull();
      expect(res.actionLabelKey).toBeNull();
    }
  });

  test("PE-04: 7 値以外の未知の errorKind は unknown として扱いログフォルダ導線を付ける", () => {
    const t = makeTranslator("ja");
    const res = resolvePlaybackErrorNotice(
      {
        videoId: "abc12345678",
        errorKind: "future_category_v2",
        errorDetail: "sanitized reason",
      },
      t,
    );
    expect(res.msg).toContain("abc12345678");
    expect(res.msg).toContain("詳細: sanitized reason");
    expect(res.actionKind).toBe("open_logs");
    expect(res.actionLabelKey).toBe("player.failed.action.openLogs");
  });

  test("PE-05: 英語ロケール（en）で全 7 カテゴリの文言とボタンラベルが生成される", () => {
    const t = makeTranslator("en");

    expect(
      resolvePlaybackErrorNotice(
        { videoId: "00000000000", errorKind: "video_unavailable", errorDetail: "upcoming" },
        t,
      ).msg,
    ).toBe("This video cannot be played (not started yet)");

    expect(
      resolvePlaybackErrorNotice({ videoId: "x", errorKind: "cookie_failed" }, t).msg,
    ).toContain("failed to read browser cookies");

    const blocked = resolvePlaybackErrorNotice(
      { videoId: "x", errorKind: "blocked" },
      t,
    );
    expect(blocked.msg).toContain("blocked playback");
    expect(t(blocked.actionLabelKey!)).toBe("Open dependency settings");

    expect(
      resolvePlaybackErrorNotice({ videoId: "x", errorKind: "js_challenge" }, t).msg,
    ).toContain("Make sure deno is installed");

    expect(
      resolvePlaybackErrorNotice({ videoId: "x", errorKind: "no_format" }, t).msg,
    ).toContain("No playable format");

    expect(
      resolvePlaybackErrorNotice({ videoId: "x", errorKind: "network" }, t).msg,
    ).toContain("network may be unreachable");

    const unknown = resolvePlaybackErrorNotice(
      { videoId: "vid123", errorKind: "unknown", errorDetail: "some err" },
      t,
    );
    expect(unknown.msg).toContain("Could not start playback (vid123)");
    expect(unknown.msg).toContain("\nDetail: some err");
    expect(t(unknown.actionLabelKey!)).toBe("Open log folder");
  });
});
