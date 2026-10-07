// i18n 基盤（要件定義 §5）: UI 文字列をメッセージキーで管理し、言語リソースを差し替え可能にする。
// 現行リソースは日本語のみ。英語リソースは後フェーズで追加する。

export const LOCALES = ["ja"] as const;
export type Locale = (typeof LOCALES)[number];

const ja = {
  "home.lead":
    "mpv による軽量な再生とローカル完結のデータ管理を一体化した YouTube 専用ブラウザ。",
  "home.db.checking": "DB 接続を確認中…",
  "home.db.ok": "DB 接続: OK（スキーマ v{version}）",
  "home.db.ng": "DB 接続: 失敗 — {error}",

  "nav.player": "再生",
  "nav.settings": "設定",

  // Phase 1: mpv 再生の最小 UI
  "player.input.placeholder": "YouTube の URL または動画 ID",
  "player.play": "再生",
  "player.playResume": "続きから再生",
  "player.history.hint": "前回 {position} まで視聴",
  "player.resumeApplied": "前回位置（{position}）から再開しました",
  "player.pause": "一時停止",
  "player.resume": "再開",
  "player.close": "終了",
  "player.buffering": "バッファ中…",
  "player.noDuration": "—:—",
  "player.fps": "{fps} fps",
  "player.volume": "音量",
  "player.speed": "速度",
  "player.ended": "再生が終了しました（{reason}）",
  "player.error": "エラー: {message}",
  "player.frameStep": "1 コマ進み",
  "player.frameBackStep": "1 コマ戻り",

  "ytdlp.checking": "yt-dlp を確認中…",
  "ytdlp.ok": "yt-dlp {version}（{path}）",
  "ytdlp.missing":
    "yt-dlp が見つかりません。インストールするか設定 ytdlp.path でパスを指定してください",
  "ytdlp.update": "yt-dlp 更新",
  "ytdlp.updating": "更新中…",
  "ytdlp.updated": "yt-dlp 更新: {output}",
  "ytdlp.updateFailed": "yt-dlp 更新失敗: {message}",

  // Phase 2: 設定画面（画質プリセット）
  "settings.title": "設定",
  "settings.quality.title": "画質（ytdl-format）",
  "settings.quality.desc":
    "mpv に渡す yt-dlp のフォーマット式をプリセットまたは自由記述で選ぶ。保存すると再生中の動画へも即時適用される。",
  "settings.quality.preset.1080": "1080p 上限",
  "settings.quality.preset.1080p60": "1080p60 優先",
  "settings.quality.preset.av1": "AV1 優先",
  "settings.quality.preset.best": "最高画質",
  "settings.quality.custom": "カスタム",
  "settings.quality.format.label": "フォーマット式",
  "settings.save": "保存",
  "settings.saving": "保存中…",
  "settings.saved": "保存しました",
  "settings.applied": "保存し、再生中の {count} 台へ適用しました",
  "settings.failed": "保存に失敗: {message}",
} as const;

export type MessageKey = keyof typeof ja;

const MESSAGES: Record<Locale, Record<MessageKey, string>> = { ja };

let currentLocale: Locale = "ja";

export function setLocale(locale: Locale): void {
  currentLocale = locale;
}

type Params = Record<string, string | number>;

/// メッセージキーから表示文字列を得る。`{name}` 形式のプレースホルダーを params で埋める。
export function t(key: MessageKey, params?: Params): string {
  const template = MESSAGES[currentLocale][key] ?? MESSAGES.ja[key] ?? key;
  if (!params) return template;
  return template.replace(/\{(\w+)\}/g, (_, name: string) =>
    String(params[name] ?? `{${name}}`),
  );
}
