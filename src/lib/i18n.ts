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
  "nav.library": "ライブラリ",
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
  "player.quality": "画質",
  "player.quality.hint":
    "このインスタンスだけの画質式を変更（セッション内のみ有効。次回再生は既定画質に戻る）",
  "player.ended": "再生が終了しました（{reason}）",
  "player.error": "エラー: {message}",
  "player.frameStep": "1 コマ進み",
  "player.frameBackStep": "1 コマ戻り",

  // Phase 8: PiP（設計書 §4.5: 最前面・枠なしの小窓）
  "player.playPip": "PiP で再生",
  "player.pip": "PiP",
  "player.unpip": "PiP 解除",
  "player.subscribe": "チャンネル購読",
  "player.subscribed": "購読済み",
  "player.pip.hint":
    "最前面・枠なしの小窓で再生。mpv のショートカット（q で終了、m でミュート）は小窓上でも使えます",

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
  "feed.channels.filter": "表示するカテゴリ",
  "feed.channels.empty": "このカテゴリのチャンネルはありません",
  "feed.items.title": "フィード",
  "feed.items.empty": "フィード項目がありません",
  "feed.items.markRead": "既読",
  "feed.items.markAllRead": "すべて既読",
  "feed.readMarkFailed": "再生は開始しましたが既読の記録に失敗: {message}",
  "feed.markedAllRead": "すべて既読にしました",
  "feed.filters.unreadOnly": "未読のみ",
  "feed.filters.all": "すべて",
  "feed.filters.category": "カテゴリ",
  "feed.filters.kind": "種別",
  "feed.filters.kindVideo": "動画",
  "feed.filters.kindShort": "Shorts",
  "feed.filters.kindLive": "ライブ",
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

  // Phase 15: HDR・mpv 引数（仕様決定 Y、INV-1 調査結果の設定化）
  "settings.hdr.title": "HDR・mpv 引数",
  "settings.hdr.desc": "HDR 動画の変換方法と mpv への追加引数。次回の再生開始から反映されます（SDR 画面では HDR→SDR 変換に効きます）",
  "settings.hdr.toneMapping": "トーンマッピング方式",
  "settings.hdr.computePeak": "ピーク輝度の計測",
  "settings.hdr.auto": "自動（mpv 既定）",
  "settings.hdr.yes": "計測する",
  "settings.hdr.no": "計測しない",
  "settings.hdr.extraArgs": "mpv 追加引数",
  "settings.hdr.extraArgs.desc": "空白区切りで起動引数の末尾に追加します（例: --target-colorspace-hint=yes --gpu-api=d3d11）。無効な引数は mpv 起動失敗になります",

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

  // Phase 6: ライブチャット・NG フィルタ・履歴検索
  "chat.show": "チャットを表示",
  "chat.hide": "チャットを閉じる",
  "chat.title": "ライブチャット",
  "chat.empty": "メッセージがありません",
  "chat.deleted": "このメッセージは削除されました",
  "chat.anonymous": "匿名",
  "chat.membership": "メンバー",
  "chat.status": "チャット: {message}",

  "settings.filters.title": "NG フィルタ",
  "settings.filters.desc":
    "チャット本文・投稿者のほか、動画タイトル・チャンネル名・チャンネル ID も対象にできる（フィード・検索・関連動画の一覧から除外）。登録・削除は即時に反映される。",
  "settings.filters.target": "対象",
  "settings.filters.kind": "種別",
  "settings.filters.pattern.placeholder": "パターン（NG ワード / 正規表現）",
  "settings.filters.add": "追加",
  "settings.filters.added": "フィルタを追加しました",
  "settings.filters.addFailed": "フィルタの追加に失敗: {message}",
  "settings.filters.remove": "削除",
  "settings.filters.removed": "フィルタを削除しました",
  "settings.filters.removeFailed": "フィルタの削除に失敗: {message}",
  "settings.filters.empty": "フィルタはありません",
  "filter.target.video_title": "動画タイトル",
  "filter.target.video_desc": "動画説明文",
  "filter.target.channel_title": "チャンネル名",
  "filter.target.channel_id": "チャンネル ID",
  "filter.target.chat_text": "チャット本文",
  "filter.target.chat_author": "チャット投稿者",
  "filter.kind.literal": "部分一致",
  "filter.kind.regex": "正規表現",

  "settings.chatSearch.title": "チャット履歴検索",
  "settings.chatSearch.desc":
    "保存済みチャットを本文・投稿者名で全文検索する（検索語は 3 文字以上を推奨）。",
  "settings.chatSearch.videoId": "動画 ID / URL（省略可）",
  "settings.chatSearch.placeholder": "検索キーワード",
  "settings.chatSearch.button": "検索",
  "settings.chatSearch.searching": "検索中…",
  "settings.chatSearch.empty": "結果がありません",
  "settings.chatSearch.failed": "検索に失敗: {message}",
  "settings.chatSearch.count": "{count} 件",

  // Phase 7: ローカルデータ（履歴・お気に入り・プレイリスト）
  "library.title": "ライブラリ",
  "library.tab.history": "履歴",
  "library.tab.favorites": "お気に入り",
  "library.tab.playlists": "プレイリスト",
  "library.play": "再生",
  "library.playFromStart": "先頭から再生",
  "library.remove": "削除",
  "library.failed": "操作に失敗: {message}",
  "library.history.empty": "視聴履歴はありません",
  "library.history.progress": "{percent}% 視聴済み",
  "library.history.completed": "視聴済み",
  "library.history.lastWatched": "最終視聴 {at}",
  "library.favorite.add": "お気に入りに追加",
  "library.favorite.remove": "お気に入りを解除",
  "library.favorite.added": "お気に入りに追加しました",
  "library.favorite.removed": "お気に入りを解除しました",
  "library.favorite.failed": "お気に入り操作に失敗: {message}",
  "library.favorite.empty": "お気に入りはありません",
  "library.playlist.addTo": "プレイリストに追加",
  "library.playlist.added": "「{name}」に追加しました",
  "library.playlist.addFailed": "プレイリストへの追加に失敗: {message}",
  "library.playlist.none": "プレイリストがありません",
  "library.playlist.newPlaceholder": "新しいプレイリスト名",
  "library.playlist.createAdd": "作成して追加",
  "library.playlist.create": "作成",
  "library.playlist.created": "プレイリスト「{name}」を作成しました",
  "library.playlist.createFailed": "プレイリストの作成に失敗: {message}",
  "library.playlist.rename": "名前を変更",
  "library.playlist.renamed": "名前を変更しました",
  "library.playlist.delete": "削除",
  "library.playlist.deleted": "プレイリストを削除しました",
  "library.playlist.deleteFailed": "削除に失敗: {message}",
  "library.playlist.items": "{count} 件",
  "library.playlist.empty": "動画が登録されていません",
  "library.playlist.selectHint": "左の一覧からプレイリストを選んでください",
  "library.playlists.empty": "プレイリストはありません",
  "library.playlist.importUrl": "YouTube プレイリストの URL",
  "library.playlist.importName": "プレイリスト名（省略可）",
  "library.playlist.import": "取り込み",
  "library.playlist.importing": "取り込み中…",
  "library.playlist.imported": "「{name}」を取り込みました（{count} 件）",
  "library.playlist.importFailed": "取り込みに失敗: {message}",
  "library.playlist.sort": "投稿日時で並べ替え",
  "library.playlist.sorted": "投稿日時順に並べ替えました",
  "library.playlist.moveUp": "上へ",
  "library.playlist.moveDown": "下へ",
  "library.playlist.reorderFailed": "並べ替えに失敗: {message}",
  "library.playlist.dragHint": "ドラッグで並べ替え",
  "library.playlist.queue": "ここから連続再生",
  "library.playlist.queueFailed": "連続再生の開始に失敗: {message}",
  "library.playlist.queueActive": "連続再生中: {name}（{index}/{count}）",
  "library.playlist.queueStop": "連続再生を止める",
  "library.removed": "削除しました",
  "library.removeFailed": "削除に失敗: {message}",

  // Phase 7: ホイール割り当て（決定記録: script-opts 注入のため次回再生から有効）
  "settings.wheel.title": "ホイール割り当て",
  "settings.wheel.desc":
    "mpv のホイール操作は「一時停止中＝コマ送り、再生中＝音量」。ここでは再生中の 1 ノッチあたりの音量変化量を設定する。mpv 起動オプションで渡すため、変更は次回の再生から有効",
  "settings.wheel.volumeDelta": "音量変化量（ノッチあたり）",

  // Phase 8: PiP 小窓の位置（次回の PiP 化から有効）
  "settings.pip.title": "PiP",
  "settings.pip.desc":
    "PiP（最前面・枠なしの小窓）の位置とサイズ。mpv の --geometry 形式（例: 480x270-40-40 は幅480・高さ270・右下から 40px 内側）。次回の PiP 化から有効",
  "settings.pip.geometry": "小窓の位置とサイズ（WxH+±x±y）",
  "settings.pip.quality": "PiP の既定画質（ytdl-format 式）",
  "settings.pip.quality.desc":
    "空欄なら全体の画質設定に従う。小窓向けに低めの画質を指定しておくと起動が軽くなる",
  "settings.pip.unsaved":
    "保存中に PiP の値が変更されました。DB には既定値が保存されたため、もう一度保存してください",
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
