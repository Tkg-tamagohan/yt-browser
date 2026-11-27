// i18n のロケール解決とリソース網羅のテスト（FR-81、仕様決定 DF）。
// LN-01: en リソースが ja の全キーを網羅する（フォールバック対象なし）
// LN-02: resolveMessage のロケール別解決・ja フォールバック・未知キー
// LN-03: setLocale / currentLocale の反映と t() のプレースホルダー置換
// LN-04: Phase 92 の YouTube Data API キー関連メッセージ解決と protectedNote 5 キー記載
// LN-05: Phase 96 のプレイリスト同期関連メッセージ解決とプレースホルダー整合
import { describe, expect, test } from "vitest";
import { en } from "./i18n-en";
import {
  currentLocale,
  fallbackKeys,
  LOCALES,
  messageKeys,
  resolveMessage,
  setLocale,
  t,
  type MessageKey,
} from "./i18n";

describe("LN-01 en リソースの網羅", () => {
  test("en は ja の全キーを持つ（フォールバック対象なし）", () => {
    expect(fallbackKeys("en")).toEqual([]);
  });

  test("en に ja へ無い余剰キーが存在しない", () => {
    const jaSet = new Set<string>(messageKeys());
    const extra = Object.keys(en).filter((key) => !jaSet.has(key));
    expect(extra).toEqual([]);
  });

  test("全キーが en で文言へ解決される", () => {
    // 解決値が空でなくキー名そのまま（未解決）でないことを確認する
    expect(messageKeys().length).toBeGreaterThan(0);
    for (const key of messageKeys()) {
      const resolved = resolveMessage("en", key);
      expect(resolved, `${key} の en 文言`).not.toBe(key);
      expect(resolved.trim(), `${key} の en 文言`).not.toBe("");
    }
  });

  test("ja と en で埋め込みプレースホルダー名が一致する", () => {
    const extractPlaceholders = (msg: string): string[] => {
      const names = new Set<string>();
      for (const m of msg.matchAll(/\{(\w+)\}/g)) {
        names.add(m[1]);
      }
      return Array.from(names).sort();
    };
    for (const key of messageKeys()) {
      const jaPh = extractPlaceholders(resolveMessage("ja", key));
      const enPh = extractPlaceholders(resolveMessage("en", key));
      expect(enPh, `${key} のプレースホルダーが ja と一致しない`).toEqual(jaPh);
    }
  });
});

describe("LN-02 resolveMessage の解決", () => {
  test("ロケールごとにその言語の文言を返す", () => {
    expect(resolveMessage("ja", "nav.feed")).toBe("フィード");
    expect(resolveMessage("en", "nav.feed")).toBe("Feed");
    expect(resolveMessage("ja", "settings.ui.language")).toBe("UI 言語");
    expect(resolveMessage("en", "settings.ui.language")).toBe("UI language");
  });

  test("どちらの言語にも無いキーはキー名をそのまま返す", () => {
    // MessageKey 型外の未知キーはキャストで注入する
    const bogus = "nonexistent" as MessageKey;
    expect(resolveMessage("en", bogus)).toBe("nonexistent");
    expect(resolveMessage("ja", bogus)).toBe("nonexistent");
  });
});

describe("LN-03 setLocale と t()", () => {
  test("setLocale が現行ロケールと t() の解決を切り替える", () => {
    try {
      expect(currentLocale()).toBe("ja");
      expect(t("nav.feed")).toBe("フィード");
      setLocale("en");
      expect(currentLocale()).toBe("en");
      expect(t("nav.feed")).toBe("Feed");
    } finally {
      setLocale("ja");
    }
  });

  test("プレースホルダーはどちらの言語でも置換する", () => {
    try {
      setLocale("en");
      expect(t("player.ended", { reason: "eof" })).toBe(
        "Playback ended (eof)",
      );
      setLocale("ja");
      expect(t("player.ended", { reason: "eof" })).toBe(
        "再生が終了しました（eof）",
      );
    } finally {
      setLocale("ja");
    }
  });

  test("LOCALES は ja / en を持つ", () => {
    expect(LOCALES).toContain("ja");
    expect(LOCALES).toContain("en");
  });
});

describe("LN-04 YouTube Data API キーの i18n キーと保護対象案内", () => {
  test("protectedNote は ja/en ともに 5 つのローカル保護設定を列挙する", () => {
    const expectedKeys = [
      "youtube.api_key",
      "ytdlp.po_token",
      "ytdlp.path",
      "mpv.extra_args",
      "setup.deps_shown",
    ];
    for (const locale of LOCALES) {
      const note = resolveMessage(locale, "settings.ops.protectedNote");
      for (const settingKey of expectedKeys) {
        expect(note, `${locale}: ${settingKey}`).toContain(settingKey);
      }
    }
  });

  test("settings.deps.youtubeApiKey 系キーが ja/en で解決されプレースホルダーを置換できる", () => {
    const keys: MessageKey[] = [
      "settings.deps.youtubeApiKey",
      "settings.deps.youtubeApiKeyPlaceholder",
      "settings.deps.youtubeApiKeyDesc",
      "settings.deps.youtubeApiKeyShow",
      "settings.deps.youtubeApiKeyHide",
      "settings.deps.youtubeApiKeyTest",
      "settings.deps.youtubeApiKeyTesting",
      "settings.deps.youtubeApiKeyTestOk",
      "settings.deps.youtubeApiKeyTestFailed",
      "settings.deps.youtubeApiKeyStatusUnset",
      "settings.deps.youtubeApiKeyStatusActive",
      "settings.deps.youtubeApiKeyStatusKeyInvalid",
      "settings.deps.youtubeApiKeyStatusSharedQuota",
      "settings.deps.youtubeApiKeyStatusSearchQuota",
      "settings.deps.youtubeApiKeyResetAt",
    ];
    for (const locale of LOCALES) {
      for (const key of keys) {
        const msg = resolveMessage(locale, key);
        expect(msg).not.toBe(key);
        expect(msg.trim()).not.toBe("");
      }
    }

    try {
      setLocale("ja");
      expect(
        t("settings.deps.youtubeApiKeyTestFailed", { reason: "keyInvalid" }),
      ).toContain("keyInvalid");
      expect(
        t("settings.deps.youtubeApiKeyResetAt", { time: "11/28 17:01" }),
      ).toContain("11/28 17:01");

      setLocale("en");
      expect(
        t("settings.deps.youtubeApiKeyTestFailed", { reason: "keyInvalid" }),
      ).toContain("keyInvalid");
      expect(
        t("settings.deps.youtubeApiKeyResetAt", { time: "11/28 17:01" }),
      ).toContain("11/28 17:01");
    } finally {
      setLocale("ja");
    }
  });
});

describe("LN-05 Phase 96 プレイリスト同期の i18n キーとプレースホルダー", () => {
  test("library.playlist.sync 系キーが ja/en で解決されプレースホルダーを置換できる", () => {
    const keys: MessageKey[] = [
      "library.playlist.sync",
      "library.playlist.syncing",
      "library.playlist.synced",
      "library.playlist.syncedNoChanges",
      "library.playlist.syncFailed",
      "library.playlist.lastSynced",
    ];
    for (const locale of LOCALES) {
      for (const key of keys) {
        const msg = resolveMessage(locale, key);
        expect(msg).not.toBe(key);
        expect(msg.trim()).not.toBe("");
      }
    }

    try {
      setLocale("ja");
      expect(
        t("library.playlist.synced", { name: "お気に入り音楽", count: 3 }),
      ).toContain("+3");
      expect(
        t("library.playlist.syncedNoChanges", { name: "お気に入り音楽" }),
      ).toContain("お気に入り音楽");
      expect(
        t("library.playlist.syncFailed", { message: "timeout" }),
      ).toContain("timeout");
      expect(
        t("library.playlist.lastSynced", { time: "2026/11/28 00:00" }),
      ).toContain("2026/11/28 00:00");

      setLocale("en");
      expect(
        t("library.playlist.synced", { name: "Mix", count: 3 }),
      ).toContain("+3");
      expect(
        t("library.playlist.syncedNoChanges", { name: "Mix" }),
      ).toContain("Mix");
      expect(
        t("library.playlist.syncFailed", { message: "timeout" }),
      ).toContain("timeout");
      expect(
        t("library.playlist.lastSynced", { time: "11/28/2026" }),
      ).toContain("11/28/2026");
    } finally {
      setLocale("ja");
    }
  });
});
