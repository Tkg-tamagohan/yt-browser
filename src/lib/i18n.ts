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
  "nav.feed": "フィード",
  "nav.search": "検索",
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
  "settings.applyPartial":
    "保存しましたが、再生中の {count} 台への即時適用に失敗しました（その台は次回再生時から有効）",
  "settings.failed": "保存に失敗: {message}",
  "settings.savedQualitySkipped":
    "保存しました（画質式が空のため、画質は変更していません）",

  // Phase 4: 購読フィード
  "feed.title": "購読フィード",
  "feed.subscribe.title": "チャンネルを購読",
  "feed.subscribe.placeholder": "チャンネル ID (UC...) / URL / @handle",
  "feed.subscribe.button": "購読",
  "feed.subscribed": "{title} を購読しました",
  "feed.subscribeFailed": "購読に失敗: {message}",
  "feed.unsubscribed": "{title} の購読を解除しました",
  "feed.unsubscribe": "解除",
  "feed.failed": "操作に失敗: {message}",
  "feed.category.label": "カテゴリ",
  "feed.category.none": "未分類",
  "feed.category.placeholder": "新しいカテゴリ名",
  "feed.category.add": "カテゴリ追加",
  "feed.categoryAdded": "カテゴリ「{name}」を追加しました",
  "feed.channels.title": "購読チャンネル",
  "feed.items.title": "フィード",
  "feed.items.empty": "フィード項目がありません",
  "feed.items.markRead": "既読",
  "feed.items.markAllRead": "すべて既読",
  "feed.readMarkFailed": "再生は開始しましたが既読の記録に失敗: {message}",
  "feed.markedAllRead": "すべて既読にしました",
  "feed.filters.unreadOnly": "未読のみ",
  "feed.filters.all": "すべて",
  "feed.filters.category": "カテゴリ",
  "feed.refresh": "今すぐ更新",
  "feed.refreshQueued": "更新を開始しました",
  "feed.newItems": "{count} 件の新着を受信",
  "feed.statusEvent": "フィード状態: {message}",
  "feed.item.block": "チャンネルをブロック",

  // Phase 5: 検索・関連動画・チャンネルブロック
  "search.title": "検索",
  "search.placeholder": "検索キーワード",
  "search.button": "検索",
  "search.searching": "検索中…",
  "search.empty": "結果がありません",
  "search.failed": "検索に失敗: {message}",
  "search.play": "再生",
  "search.subscribe": "購読",
  "search.block": "ブロック",
  "search.views.count": "{count} 回視聴",
  "search.views.man": "{count}万回",
  "search.views.oku": "{count}億回",

  "related.title": "関連動画",
  "related.show": "関連動画を表示",
  "related.hide": "関連動画を閉じる",
  "related.loading": "読み込み中…",
  "related.empty": "関連動画が見つかりません",
  "related.failed": "関連動画の取得に失敗: {message}",

  "blocked.title": "ブロック中のチャンネル",
  "blocked.desc": "ブロックしたチャンネルの動画はフィード・検索・関連動画に表示されません。",
  "blocked.empty": "ブロック中のチャンネルはありません",
  "blocked.added": "{title} をブロックしました",
  "blocked.addFailed": "ブロックに失敗: {message}",
  "blocked.unblock": "解除",
  "blocked.unblocked": "{title} のブロックを解除しました",
  "blocked.unblockFailed": "ブロック解除に失敗: {message}",

  // Phase 3: SponsorBlock のカテゴリ設定
  "settings.sponsor.title": "SponsorBlock",
  "settings.sponsor.desc":
    "カテゴリごとに区間へ入ったときの動作を選ぶ。変更は次回の再生から有効",
  "settings.sponsor.action.skip": "スキップ",
  "settings.sponsor.action.notify": "通知のみ",
  "settings.sponsor.action.off": "無効",
  "sponsor.cat.sponsor": "広告・提供読み",
  "sponsor.cat.selfpromo": "自社宣伝・無料宣伝",
  "sponsor.cat.interaction": "登録・高評価の呼びかけ",
  "sponsor.cat.intro": "イントロ（開始演出）",
  "sponsor.cat.outro": "アウトロ（終了カード）",
  "sponsor.cat.preview": "プレビュー・まとめ・フック",
  "sponsor.cat.poi_highlight": "見どころ（ハイライト）",
  "sponsor.cat.music_offtopic": "音楽動画の本題でない部分",
  "sponsor.cat.filler": "余談・フィラー",

  "sponsor.skipped": "SponsorBlock: {category} 区間をスキップしました",
  "sponsor.notified": "SponsorBlock: {category} 区間を検出しました（通知のみ）",
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
