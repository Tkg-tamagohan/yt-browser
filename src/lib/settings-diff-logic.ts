// 設定画面（src/routes/settings/+page.svelte）のフォーム状態スナップショットと
// 変更内容の差分算出・表示整形（FR-90、仕様決定 DS）。
// Tauri IPC に依存しない純粋関数に保ち、vitest から直接検証できるようにする。

import { TONE_MAPPING_MPV, type MessageKey } from "$lib/i18n";
import type { MonitorInfo } from "$lib/monitors";
import { PIP_GEOMETRY_DEFAULT } from "$lib/pip";
import { QUALITY_PRESETS } from "$lib/quality";
import {
  CUSTOM,
  SPONSOR_CATEGORIES,
  type SponsorCategory,
} from "$lib/settings-consts";

/// 設定画面で「保存」ボタンの対象になる全 34 設定（35 フィールド）の状態。
export type SettingsFormState = {
  selected: string;
  customFormat: string;
  sponsorActions: Record<SponsorCategory, string>;
  sponsorShowSegments: boolean;
  wheelDelta: number | undefined;
  pipDefault: boolean;
  pipFitAspect: boolean;
  pipGeometry: string;
  pipQuality: string;
  feedShowShorts: boolean;
  feedNotifyNew: boolean;
  feedAutoReadPlayed: boolean;
  feedAutoReadDays: number | undefined;
  liveWaitAction: string;
  shortcutsOn: boolean;
  chatShowAuthor: boolean;
  chatFontSize: string;
  chatOpacity: string;
  chatCloseOnEnd: boolean;
  initialSpeed: number | undefined;
  initialVolume: number | undefined;
  windowScreen: string;
  closeOnEof: boolean;
  uiTheme: string;
  uiDensity: string;
  uiLanguage: string;
  confirmUnsaved: boolean;
  navigateOnPlay: boolean;
  toneMapping: string;
  computePeak: string;
  mpvExtraArgs: string;
  mpvNetworkTimeout: number | undefined;
  mpvHwdec: string;
  ytdlpPath: string;
  poToken: string;
};

export type TranslateFn = (
  key: MessageKey,
  params?: Record<string, string | number>,
) => string;

export type SettingsDiffContext = {
  t: TranslateFn;
  monitors: readonly MonitorInfo[];
};

export type SettingsChangeItem = {
  id: string;
  sectionLabel: string;
  fieldLabel: string;
  beforeText: string;
  afterText: string;
  note?: string;
};

/// 既定値で初期化した設定フォーム状態を返す。
export function defaultSettingsFormState(): SettingsFormState {
  const sponsorActions = Object.fromEntries(
    SPONSOR_CATEGORIES.map((c) => [c, c === "sponsor" ? "skip" : "off"]),
  ) as Record<SponsorCategory, string>;
  return {
    selected: QUALITY_PRESETS[0].format,
    customFormat: "",
    sponsorActions,
    sponsorShowSegments: true,
    wheelDelta: 2,
    pipDefault: true,
    pipFitAspect: true,
    pipGeometry: PIP_GEOMETRY_DEFAULT,
    pipQuality: "",
    feedShowShorts: false,
    feedNotifyNew: false,
    feedAutoReadPlayed: false,
    feedAutoReadDays: 0,
    liveWaitAction: "play",
    shortcutsOn: false,
    chatShowAuthor: false,
    chatFontSize: "",
    chatOpacity: "",
    chatCloseOnEnd: true,
    initialSpeed: undefined,
    initialVolume: undefined,
    windowScreen: "",
    closeOnEof: false,
    uiTheme: "dark",
    uiDensity: "comfortable",
    uiLanguage: "ja",
    confirmUnsaved: true,
    navigateOnPlay: true,
    toneMapping: "auto",
    computePeak: "auto",
    mpvExtraArgs: "",
    mpvNetworkTimeout: undefined,
    mpvHwdec: "auto-safe",
    ytdlpPath: "",
    poToken: "",
  };
}

/// ネストした sponsorActions を含めてスナップショットを複製する。
export function cloneSettingsFormState(
  state: SettingsFormState,
): SettingsFormState {
  return {
    ...state,
    sponsorActions: { ...state.sponsorActions },
  };
}

/// 画質設定の実効式（プリセット時はその式、カスタム時はトリム済み入力式）。
export function effectiveQualityFormat(
  selected: string,
  customFormat: string,
): string {
  return selected === CUSTOM ? customFormat.trim() : selected;
}

// 静的メッセージキー参照を保つため、動的なテンプレート補間ではなく明示表を使う
const SPONSOR_ACTION_KEYS: Record<string, MessageKey> = {
  skip: "settings.sponsor.action.skip",
  notify: "settings.sponsor.action.notify",
  off: "settings.sponsor.action.off",
};

const THEME_KEYS: Record<string, MessageKey> = {
  dark: "settings.ui.theme.dark",
  light: "settings.ui.theme.light",
  system: "settings.ui.theme.system",
};

const DENSITY_KEYS: Record<string, MessageKey> = {
  comfortable: "settings.ui.density.comfortable",
  compact: "settings.ui.density.compact",
};

const LANGUAGE_KEYS: Record<string, MessageKey> = {
  ja: "settings.ui.language.ja",
  en: "settings.ui.language.en",
};

const LIVE_WAIT_KEYS: Record<string, MessageKey> = {
  play: "settings.livewait.action.play",
  notify: "settings.livewait.action.notify",
};

const COMPUTE_PEAK_KEYS: Record<string, MessageKey> = {
  auto: "settings.hdr.auto",
  yes: "settings.hdr.yes",
  no: "settings.hdr.no",
};

function labelFromMap(
  map: Record<string, MessageKey>,
  value: string,
  t: TranslateFn,
): string {
  const key = map[value];
  return key ? t(key) : value;
}

function qualityText(
  selected: string,
  customFormat: string,
  t: TranslateFn,
): string {
  if (selected === CUSTOM) {
    const trimmed = customFormat.trim();
    return trimmed
      ? t("settings.value.customFormat", { format: trimmed })
      : t("settings.value.qualityEmpty");
  }
  const preset = QUALITY_PRESETS.find((p) => p.format === selected);
  return preset ? t(preset.key) : selected;
}

function screenText(
  screen: string,
  monitors: readonly MonitorInfo[],
  t: TranslateFn,
): string {
  if (!screen) return t("settings.window.screen.auto");
  const found = monitors.find((m) => m.name === screen);
  if (!found) return t("settings.window.screen.missing", { name: screen });
  const dims = `${found.width}x${found.height}`;
  return found.primary
    ? t("settings.window.monitorPrimary", { name: screen, dims })
    : t("settings.window.monitor", { name: screen, dims });
}

/// 保存済みスナップショットと現在の編集値を比較し、変更内容一覧を返す。
/// セクションの画面表示順で並べ、言語やモニター一覧に合わせて文言を解決する。
export function diffSettingsFormState(
  saved: SettingsFormState,
  current: SettingsFormState,
  ctx: SettingsDiffContext,
): SettingsChangeItem[] {
  const { t, monitors } = ctx;
  const items: SettingsChangeItem[] = [];

  const boolText = (v: boolean) =>
    v ? t("settings.value.on") : t("settings.value.off");
  const orEmpty = (v: string) => (v ? v : t("settings.value.empty"));
  const optionalResetText = (v: string, isAfter: boolean, defVal: string) => {
    if (v) return v;
    return isAfter
      ? t("settings.value.resetDefault", { value: defVal })
      : t("settings.value.empty");
  };

  const pushToggle = (
    id: string,
    sectionKey: MessageKey,
    fieldKey: MessageKey,
    prev: boolean,
    curr: boolean,
  ) => {
    if (prev === curr) return;
    items.push({
      id,
      sectionLabel: t(sectionKey),
      fieldLabel: t(fieldKey),
      beforeText: boolText(prev),
      afterText: boolText(curr),
    });
  };

  const pushSelect = (
    id: string,
    sectionKey: MessageKey,
    fieldKey: MessageKey,
    prev: string,
    curr: string,
    format: (v: string) => string,
  ) => {
    if (prev === curr) return;
    items.push({
      id,
      sectionLabel: t(sectionKey),
      fieldLabel: t(fieldKey),
      beforeText: format(prev),
      afterText: format(curr),
    });
  };

  const coerceTrimmed = (v: unknown): string => {
    if (typeof v === "number") {
      return Number.isFinite(v) ? String(v) : "";
    }
    return typeof v === "string" ? v.trim() : "";
  };

  const pushTrimmedString = (
    id: string,
    sectionKey: MessageKey,
    fieldKey: MessageKey,
    prev: unknown,
    curr: unknown,
    format: (trimmed: string, isAfter: boolean) => string,
  ) => {
    const p = coerceTrimmed(prev);
    const c = coerceTrimmed(curr);
    if (p === c) return;
    items.push({
      id,
      sectionLabel: t(sectionKey),
      fieldLabel: t(fieldKey),
      beforeText: format(p, false),
      afterText: format(c, true),
    });
  };

  // 1. 画質（quality.format）
  const savedQuality = effectiveQualityFormat(saved.selected, saved.customFormat);
  const currQuality = effectiveQualityFormat(
    current.selected,
    current.customFormat,
  );
  const emptyCustom = current.selected === CUSTOM && currQuality === "";
  if (emptyCustom || savedQuality !== currQuality) {
    items.push({
      id: "quality.format",
      sectionLabel: t("settings.quality.title"),
      fieldLabel: t("settings.quality.format.label"),
      beforeText: qualityText(saved.selected, saved.customFormat, t),
      afterText: qualityText(current.selected, current.customFormat, t),
      note: emptyCustom ? t("settings.value.skippedNote") : undefined,
    });
  }

  // 2. ホイール割り当て（wheel.volume_delta）
  if (saved.wheelDelta !== current.wheelDelta) {
    items.push({
      id: "wheel.volume_delta",
      sectionLabel: t("settings.wheel.title"),
      fieldLabel: t("settings.wheel.volumeDelta"),
      beforeText:
        saved.wheelDelta === undefined
          ? t("settings.value.empty")
          : String(saved.wheelDelta),
      afterText:
        current.wheelDelta === undefined
          ? t("settings.value.empty")
          : String(current.wheelDelta),
      note:
        current.wheelDelta === undefined
          ? t("settings.value.skippedNote")
          : undefined,
    });
  }

  // 3. 再生（player.*）
  pushTrimmedString(
    "player.initial_speed",
    "settings.player.title",
    "settings.player.initialSpeed",
    saved.initialSpeed,
    current.initialSpeed,
    (v, isAfter) => optionalResetText(v, isAfter, "1.0"),
  );
  pushTrimmedString(
    "player.initial_volume",
    "settings.player.title",
    "settings.player.initialVolume",
    saved.initialVolume,
    current.initialVolume,
    (v, isAfter) => optionalResetText(v, isAfter, "100"),
  );
  pushToggle(
    "player.navigate_on_play",
    "settings.player.title",
    "settings.player.navigateOnPlay",
    saved.navigateOnPlay,
    current.navigateOnPlay,
  );

  // 4. PiP（pip.*）
  pushToggle(
    "pip.default",
    "settings.pip.title",
    "settings.pip.default",
    saved.pipDefault,
    current.pipDefault,
  );
  pushToggle(
    "pip.fit_aspect",
    "settings.pip.title",
    "settings.pip.fitAspect",
    saved.pipFitAspect,
    current.pipFitAspect,
  );
  pushTrimmedString(
    "pip.geometry",
    "settings.pip.title",
    "settings.pip.geometry",
    saved.pipGeometry,
    current.pipGeometry,
    (v, isAfter) => optionalResetText(v, isAfter, PIP_GEOMETRY_DEFAULT),
  );
  pushTrimmedString(
    "pip.quality.format",
    "settings.pip.title",
    "settings.pip.quality",
    saved.pipQuality,
    current.pipQuality,
    (v) => orEmpty(v),
  );

  // 5. 再生窓（window.screen / player.close_on_eof）
  pushSelect(
    "window.screen",
    "settings.window.title",
    "settings.window.screen",
    saved.windowScreen,
    current.windowScreen,
    (v) => screenText(v, monitors, t),
  );
  pushToggle(
    "player.close_on_eof",
    "settings.window.title",
    "settings.window.closeOnEof",
    saved.closeOnEof,
    current.closeOnEof,
  );

  // 6. HDR・mpv 引数
  pushSelect(
    "hdr.tone_mapping",
    "settings.hdr.title",
    "settings.hdr.toneMapping",
    saved.toneMapping,
    current.toneMapping,
    (v) => (v === "auto" ? t("settings.hdr.auto") : (TONE_MAPPING_MPV[v] ?? v)),
  );
  pushSelect(
    "hdr.compute_peak",
    "settings.hdr.title",
    "settings.hdr.computePeak",
    saved.computePeak,
    current.computePeak,
    (v) => labelFromMap(COMPUTE_PEAK_KEYS, v, t),
  );
  pushTrimmedString(
    "mpv.extra_args",
    "settings.hdr.title",
    "settings.hdr.extraArgs",
    saved.mpvExtraArgs,
    current.mpvExtraArgs,
    (v) => orEmpty(v),
  );
  pushTrimmedString(
    "mpv.network_timeout_ms",
    "settings.hdr.title",
    "settings.hdr.networkTimeout",
    saved.mpvNetworkTimeout,
    current.mpvNetworkTimeout,
    (v, isAfter) => optionalResetText(v, isAfter, "30000"),
  );
  pushSelect(
    "mpv.hwdec",
    "settings.hdr.title",
    "settings.hdr.hwdec",
    saved.mpvHwdec,
    current.mpvHwdec,
    (v) => v,
  );

  // 7. SponsorBlock
  pushToggle(
    "sponsor.show_segments",
    "settings.sponsor.title",
    "settings.sponsor.showSegments",
    saved.sponsorShowSegments,
    current.sponsorShowSegments,
  );
  for (const cat of SPONSOR_CATEGORIES) {
    const prev = saved.sponsorActions[cat];
    const curr = current.sponsorActions[cat];
    if (prev !== curr) {
      items.push({
        id: `sponsor.categories:${cat}`,
        sectionLabel: t("settings.sponsor.title"),
        fieldLabel: t(`sponsor.cat.${cat}`),
        beforeText: labelFromMap(SPONSOR_ACTION_KEYS, prev, t),
        afterText: labelFromMap(SPONSOR_ACTION_KEYS, curr, t),
      });
    }
  }

  // 8. フィード
  pushToggle(
    "feed.show_shorts",
    "settings.feed.title",
    "settings.feed.showShorts",
    saved.feedShowShorts,
    current.feedShowShorts,
  );
  pushToggle(
    "feed.notify_new",
    "settings.feed.title",
    "settings.feed.notifyNew",
    saved.feedNotifyNew,
    current.feedNotifyNew,
  );
  pushToggle(
    "feed.auto_read_played",
    "settings.feed.title",
    "settings.feed.autoReadPlayed",
    saved.feedAutoReadPlayed,
    current.feedAutoReadPlayed,
  );
  if (saved.feedAutoReadDays !== current.feedAutoReadDays) {
    items.push({
      id: "feed.auto_read_days",
      sectionLabel: t("settings.feed.title"),
      fieldLabel: t("settings.feed.autoReadDays"),
      beforeText:
        saved.feedAutoReadDays === undefined
          ? t("settings.value.empty")
          : String(saved.feedAutoReadDays),
      afterText:
        current.feedAutoReadDays === undefined
          ? t("settings.value.resetZero")
          : String(current.feedAutoReadDays),
    });
  }

  // 9. ライブ待機
  pushSelect(
    "live_wait.action",
    "settings.livewait.title",
    "settings.livewait.action",
    saved.liveWaitAction,
    current.liveWaitAction,
    (v) => labelFromMap(LIVE_WAIT_KEYS, v, t),
  );

  // 10. ショートカット
  pushToggle(
    "ui.shortcuts",
    "settings.shortcuts.title",
    "settings.shortcuts.enabled",
    saved.shortcutsOn,
    current.shortcutsOn,
  );

  // 11. チャットポップアップ
  pushToggle(
    "chat.popup_show_author",
    "settings.chatpopup.title",
    "settings.chatpopup.showAuthor",
    saved.chatShowAuthor,
    current.chatShowAuthor,
  );
  pushTrimmedString(
    "chat.popup_font_size",
    "settings.chatpopup.title",
    "settings.chatpopup.fontSize",
    saved.chatFontSize,
    current.chatFontSize,
    (v, isAfter) => optionalResetText(v, isAfter, "14"),
  );
  pushTrimmedString(
    "chat.popup_opacity",
    "settings.chatpopup.title",
    "settings.chatpopup.opacity",
    saved.chatOpacity,
    current.chatOpacity,
    (v, isAfter) => optionalResetText(v, isAfter, "1.0"),
  );
  pushToggle(
    "chat.popup_close_on_end",
    "settings.chatpopup.title",
    "settings.chatpopup.closeOnEnd",
    saved.chatCloseOnEnd,
    current.chatCloseOnEnd,
  );

  // 12. 表示
  pushSelect(
    "ui.theme",
    "settings.ui.title",
    "settings.ui.theme",
    saved.uiTheme,
    current.uiTheme,
    (v) => labelFromMap(THEME_KEYS, v, t),
  );
  pushSelect(
    "ui.density",
    "settings.ui.title",
    "settings.ui.density",
    saved.uiDensity,
    current.uiDensity,
    (v) => labelFromMap(DENSITY_KEYS, v, t),
  );
  pushSelect(
    "ui.language",
    "settings.ui.title",
    "settings.ui.language",
    saved.uiLanguage,
    current.uiLanguage,
    (v) => labelFromMap(LANGUAGE_KEYS, v, t),
  );
  pushToggle(
    "ui.confirm_unsaved",
    "settings.ui.title",
    "settings.ui.confirmUnsaved",
    saved.confirmUnsaved,
    current.confirmUnsaved,
  );

  // 13. 依存コマンド
  pushTrimmedString(
    "ytdlp.path",
    "settings.deps.title",
    "settings.deps.ytdlpPath",
    saved.ytdlpPath,
    current.ytdlpPath,
    (v) => orEmpty(v),
  );
  // ytdlp.po_token は秘匿値のため変更前後をマスク表示し、平文を出さない
  const savedToken = saved.poToken.trim();
  const currToken = current.poToken.trim();
  if (savedToken !== currToken) {
    items.push({
      id: "ytdlp.po_token",
      sectionLabel: t("settings.deps.title"),
      fieldLabel: t("settings.deps.poToken"),
      beforeText: savedToken
        ? t("settings.value.masked")
        : t("settings.value.empty"),
      afterText: !currToken
        ? t("settings.value.empty")
        : savedToken
          ? t("settings.value.maskedChanged")
          : t("settings.value.masked"),
    });
  }

  return items;
}
