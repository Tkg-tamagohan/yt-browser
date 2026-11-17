// 設定画面（src/routes/settings/+page.svelte）の差分抽出・表示整形テスト（FR-90、仕様決定 DS）。
// SD-01: 未変更時は空配列、空白トリムのみで実質不変の文字列は変更とみなさない
// SD-02: 真偽値・選択肢・数値・SponsorBlock 個別カテゴリの差分抽出と ja/en ラベル解決
// SD-03: ytdlp.po_token のマスク表示（変更前後の平文非露出）
// SD-04: pip.geometry 空欄リセット、wheelDelta undefined、カスタム画質空欄の境界条件
// SD-05: cloneSettingsFormState による独立複製と部分スナップショット更新による差分解消
import { describe, expect, test } from "vitest";
import { resolveMessage } from "./i18n";
import type { MonitorInfo } from "./monitors";
import { PIP_GEOMETRY_DEFAULT } from "./pip";
import { CUSTOM } from "./settings-consts";
import {
  cloneSettingsFormState,
  defaultSettingsFormState,
  diffSettingsFormState,
  effectiveQualityFormat,
  type TranslateFn,
} from "./settings-diff-logic";

const jaT: TranslateFn = (key, params) => {
  const tmpl = resolveMessage("ja", key);
  if (!params) return tmpl;
  return tmpl.replace(/\{(\w+)\}/g, (_, n: string) =>
    String(params[n] ?? `{${n}}`),
  );
};

const enT: TranslateFn = (key, params) => {
  const tmpl = resolveMessage("en", key);
  if (!params) return tmpl;
  return tmpl.replace(/\{(\w+)\}/g, (_, n: string) =>
    String(params[n] ?? `{${n}}`),
  );
};

const sampleMonitors: MonitorInfo[] = [
  {
    name: "HDMI-1",
    index: 0,
    x: 0,
    y: 0,
    width: 1920,
    height: 1080,
    primary: true,
  },
];

describe("SD-01 未変更と空白トリムの正規化", () => {
  test("同一状態なら差分は空配列になる", () => {
    const saved = defaultSettingsFormState();
    const curr = cloneSettingsFormState(saved);
    expect(
      diffSettingsFormState(saved, curr, { t: jaT, monitors: sampleMonitors }),
    ).toEqual([]);
  });

  test("前後空白だけの変更は実質同一として差分に含めない", () => {
    const saved = defaultSettingsFormState();
    saved.ytdlpPath = "/usr/bin/yt-dlp";
    saved.mpvExtraArgs = "--vo=gpu-next";
    const curr = cloneSettingsFormState(saved);
    curr.ytdlpPath = "  /usr/bin/yt-dlp  ";
    curr.mpvExtraArgs = " --vo=gpu-next ";
    expect(
      diffSettingsFormState(saved, curr, { t: jaT, monitors: sampleMonitors }),
    ).toEqual([]);
  });
});

describe("SD-02 各種設定の差分抽出と日英ラベル解決", () => {
  test("真偽値・選択肢・SponsorBlock 個別カテゴリを ja と en で整形する", () => {
    const saved = defaultSettingsFormState();
    const curr = cloneSettingsFormState(saved);
    curr.uiTheme = "light";
    curr.navigateOnPlay = false;
    curr.confirmUnsaved = false;
    curr.windowScreen = "HDMI-1";
    curr.sponsorActions.intro = "skip";

    const jaItems = diffSettingsFormState(saved, curr, {
      t: jaT,
      monitors: sampleMonitors,
    });
    expect(jaItems.map((i) => i.id)).toEqual([
      "player.navigate_on_play",
      "window.screen",
      "sponsor.categories:intro",
      "ui.theme",
      "ui.confirm_unsaved",
    ]);
    expect(jaItems.find((i) => i.id === "ui.theme")).toMatchObject({
      sectionLabel: "表示",
      fieldLabel: "配色テーマ",
      beforeText: "ダーク",
      afterText: "ライト",
    });
    expect(jaItems.find((i) => i.id === "sponsor.categories:intro")).toMatchObject({
      sectionLabel: "SponsorBlock",
      fieldLabel: "イントロ（開始演出）",
      beforeText: "無効",
      afterText: "スキップ",
    });

    const enItems = diffSettingsFormState(saved, curr, {
      t: enT,
      monitors: sampleMonitors,
    });
    expect(enItems.find((i) => i.id === "player.navigate_on_play")).toMatchObject({
      sectionLabel: "Playback",
      beforeText: "On",
      afterText: "Off",
    });
    expect(enItems.find((i) => i.id === "ui.theme")).toMatchObject({
      sectionLabel: "Appearance",
      fieldLabel: "Color theme",
      beforeText: "Dark",
      afterText: "Light",
    });
  });
});

describe("SD-03 秘匿設定（ytdlp.po_token）のマスク表示", () => {
  test("設定前後のトークン生文字列を出力に含めない", () => {
    const saved = defaultSettingsFormState();
    const curr = cloneSettingsFormState(saved);
    curr.poToken = "mweb.gvs+SECRET_ABC";

    const added = diffSettingsFormState(saved, curr, {
      t: jaT,
      monitors: [],
    });
    expect(added).toEqual([
      {
        id: "ytdlp.po_token",
        sectionLabel: "依存コマンド",
        fieldLabel: "PO Token",
        beforeText: "未設定",
        afterText: "***",
      },
    ]);
    expect(JSON.stringify(added)).not.toContain("SECRET_ABC");

    const updatedPrev = cloneSettingsFormState(curr);
    curr.poToken = "mweb.gvs+SECRET_XYZ";
    const modified = diffSettingsFormState(updatedPrev, curr, {
      t: jaT,
      monitors: [],
    });
    expect(modified[0]).toMatchObject({
      beforeText: "***",
      afterText: "***（変更あり）",
    });
    expect(JSON.stringify(modified)).not.toContain("SECRET_ABC");
    expect(JSON.stringify(modified)).not.toContain("SECRET_XYZ");

    curr.poToken = "  ";
    const cleared = diffSettingsFormState(updatedPrev, curr, {
      t: jaT,
      monitors: [],
    });
    expect(cleared[0]).toMatchObject({
      beforeText: "***",
      afterText: "未設定",
    });
  });
});

describe("SD-04 空欄リセットと保存スキップの境界条件", () => {
  test("pip.geometry の空欄入力は既定リセット表示になり、既定値でも入力が空なら変化とみなす", () => {
    const saved = defaultSettingsFormState();
    const curr = cloneSettingsFormState(saved);
    curr.pipGeometry = "";
    const items = diffSettingsFormState(saved, curr, { t: jaT, monitors: [] });
    expect(items).toHaveLength(1);
    expect(items[0]).toMatchObject({
      id: "pip.geometry",
      beforeText: PIP_GEOMETRY_DEFAULT,
      afterText: `空欄（既定 ${PIP_GEOMETRY_DEFAULT} に戻す）`,
    });
  });

  test("wheelDelta undefined とカスタム画質空欄にはスキップ注記が付く", () => {
    const saved = defaultSettingsFormState();
    const curr = cloneSettingsFormState(saved);
    curr.selected = CUSTOM;
    curr.customFormat = "  ";
    curr.wheelDelta = undefined;
    curr.feedAutoReadDays = undefined;

    expect(effectiveQualityFormat(curr.selected, curr.customFormat)).toBe("");
    const items = diffSettingsFormState(saved, curr, { t: jaT, monitors: [] });
    const q = items.find((i) => i.id === "quality.format");
    const w = items.find((i) => i.id === "wheel.volume_delta");
    const f = items.find((i) => i.id === "feed.auto_read_days");
    expect(q?.note).toBe("空欄のため保存時は変更されません");
    expect(w?.note).toBe("空欄のため保存時は変更されません");
    expect(f?.afterText).toBe("空欄（0 = 無効として保存）");
    expect(f?.note).toBeUndefined();
  });
});

describe("SD-05 複製分離とスナップショット更新", () => {
  test("cloneSettingsFormState の変更は元オブジェクトへ波及せず、部分保存後も残り差分だけが残る", () => {
    const saved = defaultSettingsFormState();
    const curr = cloneSettingsFormState(saved);
    curr.sponsorActions.sponsor = "notify";
    curr.initialSpeed = "1.5";
    curr.initialVolume = "999";

    expect(saved.sponsorActions.sponsor).toBe("skip");
    // 正常項目（sponsor と initialSpeed）だけが保存された状態を再現
    saved.sponsorActions.sponsor = "notify";
    saved.initialSpeed = "1.5";

    const remaining = diffSettingsFormState(saved, curr, {
      t: jaT,
      monitors: [],
    });
    expect(remaining.map((i) => i.id)).toEqual(["player.initial_volume"]);
  });
});
