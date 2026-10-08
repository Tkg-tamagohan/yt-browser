// 画質式（ytdl-format）のプリセット表（設計書 §4.3）。
// 設定画面の全体既定とプレイヤーカードのインスタンス別変更で共有する。
import { t, type MessageKey } from "$lib/i18n";

export type QualityPreset = {
  key: MessageKey;
  format: string;
};

export const QUALITY_PRESETS: QualityPreset[] = [
  {
    key: "settings.quality.preset.1080",
    format: "bv*[height<=1080]+ba/b[height<=1080]",
  },
  {
    key: "settings.quality.preset.1080p60",
    format:
      "bv*[height<=1080][fps>30]+ba/bv*[height<=1080]+ba/b[height<=1080]",
  },
  {
    key: "settings.quality.preset.av1",
    format:
      "bv*[vcodec^=av01][height<=1080]+ba/bv*[vcodec^=vp9][height<=1080]+ba/b[height<=1080]",
  },
  { key: "settings.quality.preset.best", format: "bv*+ba/b" },
];

/// プリセットに一致しない画質式の表示名（プレイヤーカード用）。
export function qualityLabel(format: string): string {
  const p = QUALITY_PRESETS.find((x) => x.format === format);
  return p ? t(p.key) : format;
}
