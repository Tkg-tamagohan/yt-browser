// 設定画面（src/routes/settings/+page.svelte）の定数群。
// check_docs_consistency の DOMAIN_CONSTANTS がこのファイルの
// SPONSOR_CATEGORIES / FILTER_TARGETS / FILTER_KINDS を参照する。
export const CUSTOM = "custom";

// SponsorBlock のカテゴリ一覧（src-tauri/src/sponsor/mod.rs の設定キーに対応）
export const SPONSOR_CATEGORIES = [
  "sponsor",
  "selfpromo",
  "interaction",
  "intro",
  "outro",
  "preview",
  "poi_highlight",
  "music_offtopic",
  "filler",
] as const;
export type SponsorCategory = (typeof SPONSOR_CATEGORIES)[number];

// HDR 関連（仕様決定 Y）。auto は「mpv 既定に任せる」＝未設定。
// 次回の再生開始から有効（起動時引数なので稼働中インスタンスには即時適用しない）
// bt.2390 / bt.2446a のようなドット入り mpv 値は check_docs_consistency の
// i18n 参照検査（ドット区切りリテラルを i18n キーとして拾う）に引っかかる
// ため、選択肢 ID はドット無しにし、mpv 値への対応は i18n.ts 側の
// TONE_MAPPING_MPV（同検査の対象外ファイル）に置く
export const TONE_MAPPINGS = [
  "auto",
  "clip",
  "hable",
  "mobius",
  "reinhard",
  "gamma",
  "linear",
  "spline",
  "bt2390",
  "bt2446a",
];

// NG フィルタ（FR-9）。対象・種別は DDL の CHECK と同じ値集合
export const FILTER_TARGETS = [
  "video_title",
  "video_desc",
  "channel_title",
  "channel_id",
  "chat_text",
  "chat_author",
] as const;
export const FILTER_KINDS = ["literal", "regex"] as const;
