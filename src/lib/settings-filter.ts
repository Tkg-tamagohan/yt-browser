// 設定画面の節フィルタ（FR-73、仕様決定 CX）。
// 画面上部の絞り込み入力に対して、節のラベル文字列（見出しと項目名）への
// 部分一致で表示する節を絞る。判定はこのファイルの純粋関数に集約する。

import type { MessageKey } from "./i18n";

/// 絞り込み対象の 1 節。labels には表示中のラベル文字列（解決済み）を渡す。
export interface SettingsSectionDef {
  id: string;
  labels: string[];
}

/// 節の定義。labelKeys は i18n キーで保持し、表示時に t() で解決する。
/// extraLabels はコマンド名など i18n キーを持たない検索語用。
interface SettingsSectionKeyDef {
  id: string;
  labelKeys: MessageKey[];
  extraLabels?: string[];
}

/// 設定画面の節と絞り込み対象ラベルの対応表（FR-73、仕様決定 CX）。
/// 節を追加したらここへも登録する。
export const SETTINGS_SECTION_DEFS: readonly SettingsSectionKeyDef[] = [
  {
    id: "quality",
    labelKeys: [
      "settings.quality.title",
      "settings.quality.format.label",
      "settings.quality.custom",
    ],
  },
  {
    id: "wheel",
    labelKeys: ["settings.wheel.title", "settings.wheel.volumeDelta"],
  },
  {
    id: "player",
    labelKeys: [
      "settings.player.title",
      "settings.player.initialSpeed",
      "settings.player.initialVolume",
    ],
  },
  {
    id: "pip",
    labelKeys: [
      "settings.pip.title",
      "settings.pip.default",
      "settings.pip.fitAspect",
      "settings.pip.geometry",
      "settings.pip.quality",
    ],
  },
  {
    id: "window",
    labelKeys: [
      "settings.window.title",
      "settings.window.screen",
      "settings.window.closeOnEof",
    ],
  },
  {
    id: "hdr",
    labelKeys: [
      "settings.hdr.title",
      "settings.hdr.toneMapping",
      "settings.hdr.computePeak",
      "settings.hdr.extraArgs",
      "settings.hdr.networkTimeout",
      "settings.hdr.hwdec",
    ],
  },
  {
    id: "sponsor",
    labelKeys: ["settings.sponsor.title", "settings.sponsor.showSegments"],
  },
  {
    id: "feed",
    labelKeys: [
      "settings.feed.title",
      "settings.feed.showShorts",
      "settings.feed.notifyNew",
      "settings.feed.autoReadPlayed",
      "settings.feed.autoReadDays",
    ],
  },
  { id: "categories", labelKeys: ["settings.categories.title"] },
  {
    id: "livewait",
    labelKeys: ["settings.livewait.title", "settings.livewait.action"],
  },
  {
    id: "chatpopup",
    labelKeys: [
      "settings.chatpopup.title",
      "settings.chatpopup.showAuthor",
      "settings.chatpopup.fontSize",
      "settings.chatpopup.opacity",
    ],
  },
  { id: "blocked", labelKeys: ["blocked.title"] },
  {
    id: "filters",
    labelKeys: [
      "settings.filters.title",
      "settings.filters.target",
      "settings.filters.kind",
    ],
  },
  { id: "ngusers", labelKeys: ["settings.ngusers.title"] },
  {
    id: "deps",
    labelKeys: ["settings.deps.title", "settings.deps.ytdlpPath"],
    extraLabels: ["yt-dlp", "ytdlp.path"],
  },
  {
    id: "diag",
    labelKeys: ["settings.diag.title"],
    extraLabels: ["mpv", "yt-dlp", "deno"],
  },
  {
    id: "ops",
    labelKeys: [
      "settings.ops.title",
      "settings.ops.export",
      "settings.ops.import",
      "settings.ops.dbSize",
      "settings.ops.vacuum",
      "settings.ops.cleanup",
    ],
  },
  { id: "update", labelKeys: ["settings.update.title", "settings.update.check"] },
];

/// 定義の i18n キーを解決して絞り込み用の節列を作る。
export function resolveSectionDefs(
  t: (key: MessageKey) => string,
): SettingsSectionDef[] {
  return SETTINGS_SECTION_DEFS.map((d) => ({
    id: d.id,
    labels: [...d.labelKeys.map((k) => t(k)), ...(d.extraLabels ?? [])],
  }));
}

/// 1 節が絞り込み文字列に一致するか。
/// 空・空白のみのクエリは常に一致（全節を表示）。一致はラベル文字列への
/// 大小文字を区別しない部分一致で、いずれかのラベルに合えばよい。
export function sectionMatches(query: string, labels: string[]): boolean {
  const q = query.trim().toLowerCase();
  if (q === "") return true;
  return labels.some((l) => l.toLowerCase().includes(q));
}

/// 絞り込み後に表示する節 ID の集合を返す。
/// 空・空白のみのクエリでは全節を返す。
export function filterSectionIds(
  query: string,
  sections: readonly SettingsSectionDef[],
): Set<string> {
  return new Set(
    sections.filter((s) => sectionMatches(query, s.labels)).map((s) => s.id),
  );
}
