// i18n 基盤（要件定義 §5、FR-81、仕様決定 DF）: UI 文字列をメッセージキーで
// 管理し、言語リソースを差し替え可能にする。
// リソースは ja（正・全キー網羅）と en（英語）。en に無いキーは ja へ
// フォールバックする。現行ロケールは i18n-locale.svelte.ts の $state が
// 保持し、ui-prefs.ts が設定 ui.language（ja/en、既定 ja）を反映する。

import { i18nLocale } from "./i18n-locale.svelte";

export const LOCALES = ["ja", "en"] as const;
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
  "nav.queue": "キュー",

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
  // Phase 47: シークバー自前化＋SponsorBlock 区間表示（FR-36、仕様決定 BI）
  "player.seekbar": "シーク位置（左右キーで 5 秒ずつ、Home/End で先頭・末尾）",
  "player.quality": "画質",
  "player.quality.hint":
    "このインスタンスだけの画質式を変更（セッション内のみ有効。次回再生は既定画質に戻る）",
  "player.ended": "再生が終了しました（{reason}）",
  "player.failed":
    "再生を開始できませんでした（{videoId}）。yt-dlp によるストリーム解決に失敗した可能性があります（YouTube 側のブロック・フォーマット不一致・yt-dlp の古さなど）",
  "player.error": "エラー: {message}",
  "player.restarted":
    "mpv が応答しなくなったため再生を再起動します（{attempt} 回目）",
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

  // Phase 53: PiP 窓の一括整列（FR-41、仕様決定 BQ）
  "player.pipTile": "PiP を整列",
  "player.pipTile.hint":
    "すべての PiP 窓を対象モニターへ格子状に並べます（対象は設定の起動ディスプレイ。自動ならプライマリ）",
  "player.pipTile.count": "PiP 窓: {count} 台",
  "player.pipTile.done": "{count} 台の PiP 窓を整列しました",
  "player.pipTile.partial":
    "{ok}/{total} 台を整列しました（失敗: {message}）",
  "player.pipTile.failed": "PiP 窓の整列に失敗: {message}",
  "player.pipTile.noMonitor": "整列先のモニターが見つかりません",
  "player.pipTile.wayland":
    "Wayland では窓の整列に対応していません（mpv の恒久制限）",

  // Phase 25: 再生の既定表示を PiP 化（FR-18、仕様決定 AJ）
  "player.playWindow": "通常窓で再生",
  "player.window.hint":
    "既定が PiP のため、このボタンは通常のウィンドウで再生します（既定表示の設定は変わりません）",

  // Phase 42: 再生中動画のタイトルとリンクのコピー（FR-34、仕様決定 BC）
  "player.copyLink": "リンクをコピー",
  "player.copyLink.hint":
    "再生中の動画タイトルと YouTube の視聴ページ URL をクリップボードへコピーします",
  "player.linkCopied": "タイトルとリンクをコピーしました",
  "player.copyLinkFailed": "リンクのコピーに失敗: {message}",

  // Phase 40: ウインドウごとの音声出力先（FR-32、仕様決定 BA）
  "player.audio": "音声出力",
  "player.audio.hint":
    "このウィンドウだけの音声出力先を変更します（セッション内のみ有効。次回の再生は自動に戻ります）",
  "player.audio.placeholder": "デバイス名を直接入力",

  // Phase 52: 字幕トラックの選択（FR-40、仕様決定 BP）
  "player.subtitle": "字幕",
  "player.subtitle.hint":
    "このウィンドウの字幕トラックを選択します（セッション内のみ有効。連続再生の次動画では自動選択に戻ります）",
  "player.subtitle.none": "なし",
  "player.subtitle.auto": "自動",

  // Phase 17: ループ再生（FR-16、仕様決定 AA）
  "player.loop.none": "ループ: なし",
  "player.loop.all": "ループ: 全体",
  "player.loop.one": "ループ: 1 項目",
  "player.loop.hint":
    "ループ再生の切り替え。押すたび なし → 全体 → 1 項目 の順に変わります",

  // Phase 51: A-B リピート（FR-37、仕様決定 BM）
  "player.ab.off": "A-B: なし",
  "player.ab.a": "A-B: A 点",
  "player.ab.loop": "A-B: ループ中",
  "player.ab.hint":
    "A-B リピート。押すたび A 点設定 → B 点設定（ループ開始）→ 解除の順に変わります",

  // Phase 51: スクリーンショット（FR-38、仕様決定 BN）
  "player.screenshot": "スクリーンショット",
  "player.screenshot.hint": "再生中のフレームを字幕込みの PNG で保存します",
  "player.screenshot.saved": "スクリーンショットを保存しました: {path}",
  "player.screenshot.failed": "スクリーンショットの保存に失敗: {message}",
  "player.screenshot.openFolder": "フォルダを開く",
  // mpv 窓の前面化（FR-42、仕様決定 BR）
  "player.focus": "前面に出す",
  "player.focus.hint": "mpv の再生窓を最前面に出します",
  "player.focus.wayland":
    "Wayland では mpv 窓の前面化に対応していません（プロトコルの制約）",
  "player.focus.other": "この表示環境では mpv 窓の前面化に対応していません",
  "player.focus.failed": "mpv 窓を前面に出せませんでした: {message}",

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
  "deeplink.playFailed": "動画を開けませんでした: {message}",
  "deeplink.importFailed": "プレイリストを取り込めませんでした: {message}",
  "deeplink.unsupported": "対応していないリンクです",
  "deeplink.queueFailed": "キューへの追加に失敗しました: {message}",
  "feed.category.label": "カテゴリ",
  "feed.category.none": "未分類",
  "feed.category.placeholder": "新しいカテゴリ名",
  "feed.category.add": "カテゴリ追加",
  "feed.categoryAdded": "カテゴリ「{name}」を追加しました",
  "feed.channels.title": "購読チャンネル",
  "feed.channels.filter": "表示するカテゴリ",
  "feed.channels.empty": "このカテゴリのチャンネルはありません",
  "feed.channels.showFeed": "このチャンネルのフィードを表示",
  "feed.items.title": "フィード",
  "feed.items.empty": "フィード項目がありません",
  "feed.items.markRead": "既読",
  "feed.items.markAllRead": "すべて既読",
  "feed.items.loadMore": "さらに読み込む",
  "feed.channelFilter.label": "チャンネル: {title}",
  "feed.channelFilter.clear": "チャンネル絞り込みを解除",
  "feed.backfill": "このチャンネルの過去動画を取得",
  "feed.backfilling": "過去動画を取得中…",
  "feed.backfilled": "過去動画を {count} 件取り込みました",
  "feed.backfillPartial": "一部のタブで取得に失敗しました: {message}",
  "feed.backfillFailed": "過去動画の取得に失敗: {message}",
  "feed.readMarkFailed": "再生は開始しましたが既読の記録に失敗: {message}",
  "feed.markedAllRead": "すべて既読にしました",
  "feed.filters.unreadOnly": "未読のみ",
  "feed.filters.all": "すべて",
  "feed.filters.category": "カテゴリ",
  "feed.filters.kind": "種別",
  "feed.filters.kindVideo": "動画",
  "feed.filters.kindShort": "Shorts",
  "feed.filters.kindLive": "ライブ",
  "feed.filters.groupByChannel": "チャンネル別",
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
  "search.loadMore": "さらに読み込む",

  "related.title": "関連動画",
  "related.show": "関連動画を表示",
  "related.hide": "関連動画を閉じる",
  "related.loading": "読み込み中…",
  "related.empty": "関連動画が見つかりません",
  "related.failed": "関連動画の取得に失敗: {message}",

  // Phase 41: 再生中動画の概要欄表示（FR-33、仕様決定 BB）
  "desc.title": "概要欄",
  "desc.show": "概要欄を表示",
  "desc.hide": "概要欄を閉じる",
  "desc.loading": "読み込み中…",
  "desc.empty": "この動画に概要欄はありません",
  "desc.failed": "概要欄の取得に失敗: {message}",

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
  "settings.hdr.extraArgs.desc": "空白区切りで起動引数の末尾に追加します（例: --target-colorspace-hint=yes --gpu-api=d3d11）。空白を含む値は引用符で囲めます（例: --icc-profile=\"C:\\dir\\a.icc\"）。無効な引数は mpv 起動失敗になります",
  "settings.hdr.networkTimeout": "ストリーム読み取りタイムアウト (ms)",
  "settings.hdr.networkTimeout.desc": "応答しない接続での読み取り待ち時間。空欄・不正値は既定の 30000ms、0 で無効化します。ライブ再生中に mpv が応答しなくなる事象への対策です",
  "settings.hdr.hwdec": "HW デコード方式",
  "settings.hdr.hwdec.desc":
    "mpv の --hwdec に渡す値。既定の auto-safe は mpv 既定と同じです。再生中に mpv が応答しなくなる環境（GPU ドライバのデッドロック等）では no でハードウェアデコードを無効化して回避できますが、CPU 負荷が増えます",
  "settings.hdr.unsaved":
    "保存中に HDR・mpv 引数の値が変更されました。DB には変更前の値が保存されたため、もう一度保存してください",

  // Phase 18: アプリの自動更新（FR-15、仕様決定 AB）
  "update.title": "アプリの更新",
  "update.body": "新しいバージョン {version} を利用できます。ダウンロードして適用し、再起動しますか？",
  "update.apply": "更新する",
  "update.later": "後で",
  "update.installing": "更新を適用しています…",
  "update.found": "バージョン {version} の更新があります",
  "update.upToDate": "最新のバージョンです",
  "update.failed": "更新の確認に失敗: {message}",
  "update.installFailed": "更新の適用に失敗: {message}",
  "settings.update.title": "アプリの更新",
  "settings.update.desc": "AppImage / NSIS 版は自動更新に対応しています。起動時にも自動で確認します（deb / rpm 版は対象外です）",
  "settings.update.check": "今すぐ確認",
  "settings.update.checking": "確認中…",

  // Phase 3: SponsorBlock のカテゴリ設定
  "settings.sponsor.title": "SponsorBlock",
  "settings.sponsor.desc":
    "カテゴリごとに区間へ入ったときの動作を選ぶ。変更は次回の再生から有効",
  "settings.sponsor.action.skip": "スキップ",
  "settings.sponsor.action.notify": "通知のみ",
  "settings.sponsor.action.off": "無効",
  // Phase 47: シークバーへの区間帯表示（FR-36、仕様決定 BI）
  "settings.sponsor.showSegments": "シークバーに区間を表示",
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
  "chat.popup": "ポップアップ",
  "chat.popup.hint": "別ウィンドウでチャットを開きます（このパネルは閉じます）",
  "chat.pin": "最前面",
  "chat.pin.hint": "ポップアップを最前面に固定します",
  "chat.toPanel": "パネルに戻す",
  "chat.title": "ライブチャット",
  "chat.empty": "メッセージがありません",
  "chat.deleted": "このメッセージは削除されました",
  "chat.anonymous": "匿名",
  "chat.membership": "メンバー",
  "chat.status": "チャット: {message}",

  "settings.filters.title": "NG フィルタ",
  "settings.filters.desc":
    "チャット本文・投稿者のほか、動画タイトル・チャンネル名・チャンネル ID も対象にできる（フィード・検索・関連動画の一覧から除外）。種別「ハイライト」はチャット発言を除去せず強調色で表示する。登録・削除は即時に反映される。",
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
  // Phase 68: NG ユーザー節（FR-70、仕様決定 CU）。chat_author フィルタの専用管理
  "settings.ngusers.title": "NG ユーザー",
  "settings.ngusers.desc":
    "登録した投稿者名またはチャンネル ID にマッチするチャット発言を表示しません。追加・解除は即時に反映されます（chat_author フィルタの部分一致を登録。正規表現は NG フィルタ節で登録できます）",
  "settings.ngusers.placeholder": "投稿者名またはチャンネル ID",
  "settings.ngusers.add": "追加",
  "settings.ngusers.added": "NG ユーザーを追加しました",
  "settings.ngusers.addFailed": "NG ユーザーの追加に失敗: {message}",
  "settings.ngusers.remove": "解除",
  "settings.ngusers.removed": "NG ユーザーを解除しました",
  "settings.ngusers.removeFailed": "NG ユーザーの解除に失敗: {message}",
  "settings.ngusers.empty": "NG ユーザーはありません",
  "filter.target.video_title": "動画タイトル",
  "filter.target.video_desc": "動画説明文",
  "filter.target.channel_title": "チャンネル名",
  "filter.target.channel_id": "チャンネル ID",
  "filter.target.chat_text": "チャット本文",
  "filter.target.chat_author": "チャット投稿者",
  "filter.kind.literal": "部分一致",
  "filter.kind.regex": "正規表現",
  "filter.kind.highlight": "ハイライト",

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
  // Phase 61: 履歴一覧のページングと検索（FR-52・FR-61、仕様決定 CC・CL）
  "library.history.loadMore": "さらに読み込む",
  "library.history.searchPlaceholder": "タイトルやチャンネル名で絞り込み",
  "library.history.searchEmpty": "一致する履歴はありません",
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
  "library.playlist.loadMore": "さらに読み込む",
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
  "library.playlist.reverse": "逆順にする",
  "library.playlist.reversed": "項目順を反転しました",
  "library.playlist.moveUp": "上へ",
  "library.playlist.moveDown": "下へ",
  "library.playlist.reorderFailed": "並べ替えに失敗: {message}",
  "library.playlist.dragHint": "ドラッグで並べ替え",
  "library.playlist.queue": "ここから連続再生",
  "library.playlist.queueFailed": "連続再生の開始に失敗: {message}",
  "library.playlist.queueActive": "連続再生中: {name}（{index}/{count}）",
  "library.playlist.queueSaved": "キュー停止中: {name}（{index}/{count}）",
  "library.playlist.queueStop": "連続再生を止める",
  "queue.tab.temp": "一時キュー",
  "queue.add": "キューに追加",
  "queue.playNext": "次に再生",
  "queue.added": "キューに追加しました",
  "queue.addedNext": "次に再生に追加しました",
  "queue.show": "キュー",
  "queue.panel.title": "キュー",
  "queue.play": "再生",
  "queue.clear": "全消去",
  "queue.shuffle": "残りをシャッフル",
  "queue.saveAsPlaylist": "プレイリストとして保存",
  "queue.savedAsPlaylist": "プレイリスト「{name}」に保存しました",
  "queue.saveAsPlaylistFailed": "プレイリストの保存に失敗: {message}",
  "queue.empty": "キューは空です",
  "queue.startFailed": "キューの再生開始に失敗: {message}",
  "queue.persistFailed": "キューの保存に失敗しました: {message}",
  "queue.restoreFailed": "キューの復元に失敗しました: {message}",
  "library.removed": "削除しました",
  "library.removeFailed": "削除に失敗: {message}",
  // Phase 63: 削除の取り消し（FR-54、仕様決定 CE）。
  // トーストのアクションボタン「元に戻す」で削除前スナップショットを復元する
  "library.undo": "元に戻す",
  "library.restored": "元に戻しました",
  "library.restoreFailed": "復元に失敗: {message}",

  // Phase 64: あとで見るとライブラリ横断検索（FR-59・FR-60、仕様決定 CJ・CK）
  "library.tab.watchLater": "あとで見る",
  "library.tab.search": "検索",
  "library.watchLater.add": "あとで見る",
  "library.watchLater.remove": "あとで見るを解除",
  "library.watchLater.added": "あとで見るに登録しました",
  "library.watchLater.removed": "あとで見るを解除しました",
  "library.watchLater.failed": "あとで見る操作に失敗: {message}",
  "library.watchLater.empty": "あとで見るはありません",
  "library.search.placeholder":
    "履歴・お気に入り・プレイリスト・あとで見るを検索",
  "library.search.hint":
    "タイトルやチャンネル名でライブラリ全体を検索できます",
  "library.search.empty": "一致する項目はありません",
  "library.search.source.history": "履歴",
  "library.search.source.favorites": "お気に入り",
  "library.search.source.playlists": "プレイリスト",
  "library.search.source.watchLater": "あとで見る",

  // Phase 7: ホイール割り当て（決定記録: script-opts 注入のため次回再生から有効）
  "settings.wheel.title": "ホイール割り当て",
  "settings.wheel.desc":
    "mpv のホイール操作は「一時停止中＝コマ送り、再生中＝音量」。ここでは再生中の 1 ノッチあたりの音量変化量を設定する。mpv 起動オプションで渡すため、変更は次回の再生から有効",
  "settings.wheel.volumeDelta": "音量変化量（ノッチあたり）",

  // Phase 52: 再生の初期速度と初期音量（FR-39、仕様決定 BO）
  "settings.player.title": "再生",
  "settings.player.desc":
    "新規に再生を開始するインスタンスの初期値。mpv の起動引数で渡すため、変更は次回の再生開始から有効です（再生中インスタンスの速度・音量はカード上で変えられますが、セッション限りです）",
  "settings.player.initialSpeed": "初期速度",
  "settings.player.initialSpeed.desc":
    "mpv の --speed に渡す値（0.25〜4.0、既定 1.0）。空欄は既定値に戻します",
  "settings.player.initialVolume": "初期音量（%）",
  "settings.player.initialVolume.desc":
    "mpv の --volume に渡す値（0〜130、既定 100）。空欄は既定値に戻します",
  "settings.player.unsaved":
    "保存中に再生設定の値が変更されました。もう一度保存してください",

  // Phase 8: PiP 小窓の位置（次回の PiP 化から有効）
  "settings.pip.title": "PiP",
  "settings.pip.desc":
    "PiP（最前面・枠なしの小窓）の位置とサイズ。mpv の --geometry 形式（例: 480x270-40-40 は幅480・高さ270・右下から 40px 内側）。次回の PiP 化から有効",
  "settings.pip.default": "再生を既定で PiP 表示にする",
  "settings.pip.default.desc":
    "オンにするとすべての再生（連続再生の先頭・外部からの起動を含む）が PiP 小窓で始まります。起動後はプレイヤーカードで切り替えられます",
  "settings.pip.fitAspect": "小窓サイズを動画のアスペクト比に合わせる",
  "settings.pip.fitAspect.desc":
    "オンにすると小窓の幅・高さを上限枠として、動画の比率に内接するサイズへ自動調整します（例: 480x270 の枠で横長動画は約480x201、縦長動画は約152x270）。オフなら常に指定サイズ",
  "settings.pip.geometry": "小窓の位置とサイズ（WxH+±x±y、追従時は上限枠）",
  "settings.pip.quality": "PiP の既定画質（ytdl-format 式）",
  "settings.pip.quality.desc":
    "空欄なら全体の画質設定に従う。小窓向けに低めの画質を指定しておくと起動が軽くなる",
  "settings.pip.unsaved":
    "保存中に PiP の値が変更されました。DB には既定値が保存されたため、もう一度保存してください",

  // Phase 39: 再生窓の起動ディスプレイ（FR-31、仕様決定 AZ）
  "settings.window.title": "再生窓",
  "settings.window.desc":
    "mpv 再生窓を開くディスプレイの選択。変更は次回の再生開始から有効（PiP 小窓の位置とサイズは PiP 節で設定）",
  "settings.window.screen": "起動ディスプレイ",
  "settings.window.screen.auto": "自動（mpv 既定）",
  "settings.window.screen.desc":
    "選択したモニター上で再生窓を開きます。選択したモニターが外れているときは自動へフォールバックします",
  "settings.window.screen.none": "接続中のモニターを取得できませんでした",
  "settings.window.screen.missing": "{name}（現在は未接続）",
  "settings.window.monitor": "{name}（{dims}）",
  "settings.window.monitorPrimary": "{name}（{dims}・プライマリ）",
  // Phase 55: 再生完了時の自動クローズ（FR-43、仕様決定 BS）
  "settings.window.closeOnEof": "再生完了時に窓を自動で閉じる",
  "settings.window.closeOnEof.desc":
    "オンにすると、連続再生を伴わない最後までの再生（eof）で mpv 窓とプレイヤーカードを自動的に閉じます。オフ（既定）は従来どおり終了後も残します。途中終了やエラー終了は設定に関わらず閉じられます",
  "settings.window.unsaved":
    "保存中に再生窓の値が変更されました。もう一度保存してください",

  // Phase 29: フィード一覧の shorts 既定非表示（FR-23、仕様決定 AP）
  "settings.feed.title": "フィード",
  "settings.feed.desc":
    "フィード一覧の表示と新着通知・自動既読の設定。一覧の表示は次回の一覧取得から、通知と自動既読は次回のフィード更新から反映される",
  "settings.feed.showShorts": "フィードに Shorts を表示する",
  "settings.feed.showShorts.desc":
    "オフのとき、フィード一覧の「すべて」「動画」の選択は Shorts を除外します。種別フィルタで「Shorts」を選ぶと常に表示します",
  // Phase 65: 新着のデスクトップ通知と自動既読（FR-58・FR-64、仕様決定 CI・CO）
  "settings.feed.notifyNew": "新着をデスクトップ通知する",
  "settings.feed.notifyNew.desc":
    "オンにすると、フィード更新で新着を検出したとき OS のデスクトップ通知を出します（既定はオフ）",
  "settings.feed.autoReadPlayed": "再生した項目を自動で既読にする",
  "settings.feed.autoReadPlayed.desc":
    "オンにすると、再生履歴のあるフィード項目を自動で既読にします（既定はオフ）。起動時とフィード更新のたびに適用されます",
  "settings.feed.autoReadDays": "投稿からこの日数が経過した項目を既読にする",
  "settings.feed.autoReadDays.desc":
    "投稿日時が指定した日数を経過したフィード項目を自動で既読にします。0（既定）は無効です。起動時とフィード更新のたびに適用されます",
  "settings.feed.unsaved":
    "保存中にフィードの値が変更されました。もう一度保存してください",

  // Phase 56: ライブ配信の待機（FR-44、仕様決定 BT）
  "livewait.add": "配信を待機",
  "livewait.waiting": "待機中",
  "livewait.remove": "待機を解除",
  "livewait.added": "配信を待機します: {title}",
  "livewait.duplicate": "この動画は既に待機中です",
  "livewait.live": "この動画はすでに放送中です（待機登録しません）",
  "livewait.playNow": "再生する",
  "livewait.ended": "この配信は終了しています（待機登録しません）",
  "livewait.notBroadcast":
    "放送予定のライブ・プレミア公開ではありません（待機登録しません）",
  "livewait.addFailed": "配信待機の登録に失敗: {message}",
  "livewait.removed": "待機を解除しました",
  "livewait.removeFailed": "待機の解除に失敗: {message}",
  "livewait.listFailed": "待機一覧の取得に失敗: {message}",
  "livewait.tab": "待機",
  "livewait.empty": "待機中の配信はありません",
  "livewait.scheduledUnknown": "開始予定 不明",
  "livewait.status": "ライブ待機: {message}",

  // Phase 56: 設定画面のライブ待機セクション（live_wait.action）
  "settings.livewait.title": "ライブ待機",
  "settings.livewait.desc":
    "待機中の配信が開始されたときの動作。次回の開始検出から反映される",
  "settings.livewait.action": "配信開始時の動作",
  "settings.livewait.action.play": "新規インスタンスで自動再生",
  "settings.livewait.action.notify": "トースト通知のみ",
  "settings.livewait.unsaved":
    "保存中にライブ待機の値が変更されました。もう一度保存してください",

  // Phase 67: チャットポップアップの表示カスタマイズ（FR-68、仕様決定 CS）
  "settings.chatpopup.title": "チャットポップアップ",
  "settings.chatpopup.desc":
    "別ウィンドウで開くチャット（ポップアップ）の見た目。開いているポップアップにも即時に反映されます",
  "settings.chatpopup.showAuthor": "投稿者名を表示する",
  "settings.chatpopup.showAuthor.desc":
    "オフ（既定）は投稿時刻と本文のみの表示です。superchat / membership のバッジは投稿者名を表示しても出しません",
  "settings.chatpopup.fontSize": "文字サイズ（px）",
  "settings.chatpopup.fontSize.desc": "10〜32 の数値。空欄は既定値 14 に戻します",
  "settings.chatpopup.opacity": "不透明度",
  "settings.chatpopup.opacity.desc":
    "0.2〜1.0 の数値（小さいほど透けます）。空欄は既定値 1.0 に戻します",
  "settings.chatpopup.unsaved":
    "保存中にチャットポップアップの値が変更されました。もう一度保存してください",

  // Phase 71: テーマと表示密度（FR-76・FR-77、仕様決定 DA・DB）
  "settings.ui.title": "表示",
  "settings.ui.desc":
    "画面の配色テーマ・一覧の表示密度・UI 言語。保存すると直ちに表示へ反映されます",
  "settings.ui.theme": "配色テーマ",
  "settings.ui.theme.dark": "ダーク",
  "settings.ui.theme.light": "ライト",
  "settings.ui.theme.system": "システムに合わせる",
  "settings.ui.density": "一覧の表示密度",
  "settings.ui.density.comfortable": "標準",
  "settings.ui.density.compact": "コンパクト",
  // Phase 74: UI 言語（FR-81、仕様決定 DF）
  "settings.ui.language": "UI 言語",
  "settings.ui.language.ja": "日本語",
  "settings.ui.language.en": "English",
  "settings.ui.unsaved":
    "保存中に表示設定の値が変更されました。もう一度保存してください",

  // Phase 72: キーボードショートカットの有効化（FR-79、仕様決定 DD）
  "settings.shortcuts.title": "ショートカット",
  "settings.shortcuts.desc":
    "一覧とキューをキーボードで操作する既定キーマップ。保存で有効・無効が切り替わる（既定はオフ）",
  "settings.shortcuts.enabled": "キーボードショートカットを有効にする",
  "settings.shortcuts.enabled.desc":
    "一覧の動画行で j / ↓（次の行）・k / ↑（前の行）・Enter（選択行を再生）・a（選択行をキューに追加）・q（キューパネルの開閉）が使えます。入力欄の編集中やコンテキストメニュー表示中は無効です",
  "settings.shortcuts.unsaved":
    "保存中にショートカットの値が変更されました。もう一度保存してください",

  // Phase 59: 動画内ブックマーク（FR-49、仕様決定 BZ）
  "bookmark.add": "ブックマーク",
  "bookmark.add.hint": "現在位置をブックマークへ保存",
  "bookmark.tab": "ブックマーク",
  "bookmark.empty": "ブックマークはありません",
  "bookmark.saved": "ブックマークを保存しました: {position}",
  "bookmark.duplicate": "この位置はすでに保存されています: {position}",
  "bookmark.addFailed": "ブックマークの保存に失敗: {message}",
  "bookmark.removed": "ブックマークを削除しました",
  "bookmark.removeFailed": "ブックマークの削除に失敗: {message}",
  "bookmark.remove": "ブックマークを削除",
  "bookmark.listFailed": "ブックマーク一覧の取得に失敗: {message}",
  "bookmark.play": "この位置から再生",
  "bookmark.playFailed": "ブックマーク位置からの再生に失敗: {message}",
  "bookmark.edit": "編集",
  "bookmark.edit.hint": "ラベルを編集",
  "bookmark.labelSave": "保存",
  "bookmark.labelSaved": "ラベルを更新しました",
  "bookmark.labelFailed": "ラベルの更新に失敗: {message}",

  // Phase 66: 視聴統計タブ（FR-65、仕様決定 CP）
  "library.tab.stats": "統計",
  "library.stats.total": "累計視聴時間",
  "library.stats.entries": "履歴 {count} 件",
  "library.stats.byChannel": "チャンネル別",
  "library.stats.byDay": "日別",
  "library.stats.unknownChannel": "（不明）",
  "library.stats.empty": "視聴履歴がまだありません",
  "library.stats.failed": "統計の取得に失敗: {message}",
  "library.stats.refresh": "再読み込み",

  // Phase 66: 設定画面のカテゴリ並べ替え（FR-63、仕様決定 CN）
  "settings.categories.title": "カテゴリ",
  "settings.categories.desc":
    "フィードのカテゴリ表示順。上下ボタンかドラッグ＆ドロップで並べ替え（プレイリストと同じ操作。変更は即時に保存されます）",
  "settings.categories.empty": "カテゴリはありません",
  "settings.categories.moveUp": "上へ",
  "settings.categories.moveDown": "下へ",
  "settings.categories.dragHint": "ドラッグで並べ替え",
  "settings.categories.reorderFailed": "並べ替えに失敗: {message}",

  // Phase 66: 設定画面の運用節（FR-62・FR-66、仕様決定 CM・CQ）
  "settings.ops.title": "運用",
  "settings.ops.desc":
    "データのエクスポート/インポートとデータベースのメンテナンス",
  "settings.ops.export": "データをエクスポート",
  "settings.ops.export.desc":
    "購読チャンネル・カテゴリ・設定・プレイリストを JSON ファイルに書き出します",
  "settings.ops.exported": "エクスポートしました: {path}",
  "settings.ops.exportFailed": "エクスポートに失敗: {message}",
  "settings.ops.import": "データをインポート",
  "settings.ops.import.desc":
    "エクスポートした JSON を読み込みます。購読・カテゴリ・設定・プレイリストはファイルの内容で置き換わります",
  "settings.ops.importConfirm":
    "現在の購読・カテゴリ・設定・プレイリストがファイルの内容で置き換わります。実行しますか？",
  "settings.ops.imported": "インポートしました: {path}",
  "settings.ops.importFailed": "インポートに失敗: {message}",
  "settings.ops.dbSize": "データベースサイズ",
  "settings.ops.dbSize.desc":
    "DB 本体と WAL ファイルの合計（メンテナンスの目安）",
  "settings.ops.dbSize.failed": "サイズの取得に失敗: {message}",
  "settings.ops.vacuum": "VACUUM",
  "settings.ops.vacuum.desc":
    "データベースを再構築して空き領域を縮めます",
  "settings.ops.vacuumDone": "VACUUM を実行しました（{before} → {after}）",
  "settings.ops.vacuumFailed": "VACUUM に失敗: {message}",
  "settings.ops.cleanup": "孤児行整理",
  "settings.ops.cleanup.desc":
    "購読解除後に残った動画や、参照先の無い項目行を削除します（履歴は消えません）",
  "settings.ops.cleanupConfirm": "孤児行を削除します。実行しますか？",
  "settings.ops.cleanupDone": "{count} 件の孤児行を削除しました",
  "settings.ops.cleanupFailed": "孤児行整理に失敗: {message}",
  "settings.ops.diag": "診断情報をエクスポート",
  "settings.ops.diag.desc":
    "アプリログ・設定・環境情報を 1 つの zip にまとめて書き出します。署名 URL やトークン、パス内のユーザー名はマスクされます（FR-82）",
  "settings.ops.diagExported": "診断情報をエクスポートしました: {path}",
  "settings.ops.diagFailed": "診断情報のエクスポートに失敗: {message}",
  "settings.ops.busy": "実行中…",

  // Phase 70: 設定と依存診断の拡充（FR-72・FR-73・FR-74・FR-75、仕様決定 CW・CX・CY・CZ）
  "settings.filter.placeholder": "節を絞り込み（例: 画質、mpv、フィード）",
  "settings.filter.noMatch": "一致する節がありません",
  "settings.deps.title": "依存コマンド",
  "settings.deps.desc":
    "mpv・yt-dlp・deno など外部コマンドの設定。検出状況の一覧は「診断」節を参照",
  "settings.deps.ytdlpPath": "yt-dlp のパス",
  "settings.deps.ytdlpPath.placeholder": "自動検出（空欄）",
  "settings.deps.ytdlpPath.desc":
    "yt-dlp 実行ファイルを直接指定します。空欄は自動検出（同梱リソース → PATH の順）です。変更は保存後、次の yt-dlp 呼び出しから有効になります",
  "settings.deps.unsaved":
    "保存中に依存コマンドの値が変更されました。もう一度保存してください",
  "settings.diag.title": "診断",
  "settings.diag.desc":
    "外部コマンドの検出状況・バージョン・解決パスの一覧。初回起動時にも同じ一覧が表示されます",
  "deps.detected": "検出",
  "deps.missing": "未検出",
  "deps.refresh": "再検出",
  "deps.refreshing": "検出中…",
  "deps.failed": "依存コマンドの検出に失敗: {message}",
  "setup.title": "初期セットアップ",
  "setup.desc":
    "yt-browser の動作に必要な外部コマンドの検出状況です。未検出のコマンドはインストールするか、設定画面の「依存コマンド」節でパスを指定してください。この一覧は設定画面の「診断」節でいつでも再確認できます",
  "setup.ok": "はじめる",

  // Phase 75: 通知履歴（FR-83、仕様決定 DH）
  "settings.notices.title": "通知履歴",
  "settings.notices.desc":
    "このセッションで発行された通知の一覧です。新しい順に最大 {max} 件を保持し、アプリの再起動で消えます",
  "settings.notices.empty": "このセッションではまだ通知は発行されていません",
  "settings.notices.action": "アクション: {label}",
  "settings.notices.clear": "履歴を消去",

} as const;

export type MessageKey = keyof typeof ja;

// Phase 74: 英語リソース（FR-81、仕様決定 DF）。
// ja の全キーを網羅する。satisfies で網羅をコンパイル時に検査し、
// 実行時も未登録キーは t() が ja へフォールバックする（MESSAGES は Partial 型）。
const en = {
  "home.lead":
    "A dedicated YouTube browser combining lightweight mpv playback with fully local data management.",
  "home.db.checking": "Checking DB connection…",
  "home.db.ok": "DB connection: OK (schema v{version})",
  "home.db.ng": "DB connection: failed — {error}",

  "nav.player": "Player",
  "nav.feed": "Feed",
  "nav.search": "Search",
  "nav.library": "Library",
  "nav.settings": "Settings",
  "nav.queue": "Queue",

  // Phase 1: mpv 再生の最小 UI
  "player.input.placeholder": "YouTube URL or video ID",
  "player.play": "Play",
  "player.playResume": "Resume",
  "player.history.hint": "Watched up to {position} last time",
  "player.resumeApplied": "Resumed from {position}",
  "player.pause": "Pause",
  "player.resume": "Resume",
  "player.close": "Close",
  "player.buffering": "Buffering…",
  "player.noDuration": "—:—",
  "player.fps": "{fps} fps",
  "player.volume": "Volume",
  "player.speed": "Speed",
  // Phase 47: シークバー自前化＋SponsorBlock 区間表示（FR-36、仕様決定 BI）
  "player.seekbar": "Seek position (Left/Right: 5s steps, Home/End: start/end)",
  "player.quality": "Quality",
  "player.quality.hint":
    "Change the quality format for this instance only (this session only; the next playback reverts to the default quality)",
  "player.ended": "Playback ended ({reason})",
  "player.failed":
    "Could not start playback ({videoId}). Stream resolution via yt-dlp may have failed (YouTube blocking, format mismatch, or an outdated yt-dlp)",
  "player.error": "Error: {message}",
  "player.restarted":
    "mpv became unresponsive; restarting playback (attempt {attempt})",
  "player.frameStep": "Step forward 1 frame",
  "player.frameBackStep": "Step back 1 frame",

  // Phase 8: PiP（設計書 §4.5: 最前面・枠なしの小窓）
  "player.playPip": "Play in PiP",
  "player.pip": "PiP",
  "player.unpip": "Exit PiP",
  "player.subscribe": "Subscribe to channel",
  "player.subscribed": "Subscribed",
  "player.pip.hint":
    "Plays in a small always-on-top frameless window. mpv shortcuts (q to quit, m to mute) work on the PiP window too",

  // Phase 53: PiP 窓の一括整列（FR-41、仕様決定 BQ）
  "player.pipTile": "Tile PiP windows",
  "player.pipTile.hint":
    "Arranges all PiP windows in a grid on the target monitor (the startup display in settings; primary when set to auto)",
  "player.pipTile.count": "PiP windows: {count}",
  "player.pipTile.done": "Tiled {count} PiP windows",
  "player.pipTile.partial": "Tiled {ok}/{total} windows (failed: {message})",
  "player.pipTile.failed": "Failed to tile PiP windows: {message}",
  "player.pipTile.noMonitor": "No target monitor found",
  "player.pipTile.wayland":
    "Window tiling is not supported on Wayland (a permanent mpv limitation)",

  // Phase 25: 再生の既定表示を PiP 化（FR-18、仕様決定 AJ）
  "player.playWindow": "Play in window",
  "player.window.hint":
    "Since PiP is the default, this button plays in a normal window (the default display setting is unchanged)",

  // Phase 42: 再生中動画のタイトルとリンクのコピー（FR-34、仕様決定 BC）
  "player.copyLink": "Copy link",
  "player.copyLink.hint":
    "Copies the playing video's title and YouTube watch URL to the clipboard",
  "player.linkCopied": "Copied title and link",
  "player.copyLinkFailed": "Failed to copy link: {message}",

  // Phase 40: ウインドウごとの音声出力先（FR-32、仕様決定 BA）
  "player.audio": "Audio output",
  "player.audio.hint":
    "Changes the audio output for this window only (this session only; the next playback reverts to auto)",
  "player.audio.placeholder": "Enter a device name",

  // Phase 52: 字幕トラックの選択（FR-40、仕様決定 BP）
  "player.subtitle": "Subtitles",
  "player.subtitle.hint":
    "Selects the subtitle track for this window (this session only; the next video in continuous playback reverts to auto)",
  "player.subtitle.none": "None",
  "player.subtitle.auto": "Auto",

  // Phase 17: ループ再生（FR-16、仕様決定 AA）
  "player.loop.none": "Loop: off",
  "player.loop.all": "Loop: all",
  "player.loop.one": "Loop: one",
  "player.loop.hint":
    "Toggles loop playback. Each press cycles off → all → one",

  // Phase 51: A-B リピート（FR-37、仕様決定 BM）
  "player.ab.off": "A-B: off",
  "player.ab.a": "A-B: point A",
  "player.ab.loop": "A-B: looping",
  "player.ab.hint":
    "A-B repeat. Each press cycles set A → set B (loop starts) → clear",

  // Phase 51: スクリーンショット（FR-38、仕様決定 BN）
  "player.screenshot": "Screenshot",
  "player.screenshot.hint":
    "Saves the current frame as a PNG including subtitles",
  "player.screenshot.saved": "Screenshot saved: {path}",
  "player.screenshot.failed": "Failed to save screenshot: {message}",
  "player.screenshot.openFolder": "Open folder",
  // mpv 窓の前面化（FR-42、仕様決定 BR）
  "player.focus": "Bring to front",
  "player.focus.hint": "Brings the mpv playback window to the front",
  "player.focus.wayland":
    "Raising the mpv window is not supported on Wayland (protocol limitation)",
  "player.focus.other":
    "Raising the mpv window is not supported in this display environment",
  "player.focus.failed": "Could not raise the mpv window: {message}",

  "ytdlp.checking": "Checking yt-dlp…",
  "ytdlp.ok": "yt-dlp {version} ({path})",
  "ytdlp.missing":
    "yt-dlp was not found. Install it or set its path with the ytdlp.path setting",
  "ytdlp.update": "Update yt-dlp",
  "ytdlp.updating": "Updating…",
  "ytdlp.updated": "yt-dlp updated: {output}",
  "ytdlp.updateFailed": "yt-dlp update failed: {message}",

  // Phase 2: 設定画面（画質プリセット）
  "settings.title": "Settings",
  "settings.quality.title": "Quality (ytdl-format)",
  "settings.quality.desc":
    "Choose the yt-dlp format expression passed to mpv via preset or free text. Saving also applies it immediately to currently playing videos.",
  "settings.quality.preset.1080": "Up to 1080p",
  "settings.quality.preset.1080p60": "Prefer 1080p60",
  "settings.quality.preset.av1": "Prefer AV1",
  "settings.quality.preset.best": "Best quality",
  "settings.quality.custom": "Custom",
  "settings.quality.format.label": "Format expression",
  "settings.save": "Save",
  "settings.saving": "Saving…",
  "settings.saved": "Saved",
  "settings.applied": "Saved and applied to {count} playing instance(s)",
  "settings.applyPartial":
    "Saved, but immediate application to {count} playing instance(s) failed (takes effect on their next playback)",
  "settings.failed": "Save failed: {message}",
  "settings.savedQualitySkipped":
    "Saved (quality unchanged because the format expression was empty)",

  // Phase 4: 購読フィード
  "feed.title": "Subscription feed",
  "feed.subscribe.title": "Subscribe to a channel",
  "feed.subscribe.placeholder": "Channel ID (UC...) / URL / @handle",
  "feed.subscribe.button": "Subscribe",
  "feed.subscribed": "Subscribed to {title}",
  "feed.subscribeFailed": "Subscribe failed: {message}",
  "feed.unsubscribed": "Unsubscribed from {title}",
  "feed.unsubscribe": "Unsubscribe",
  "feed.failed": "Operation failed: {message}",
  "deeplink.playFailed": "Could not open the video: {message}",
  "deeplink.importFailed": "Could not import the playlist: {message}",
  "deeplink.unsupported": "Unsupported link",
  "deeplink.queueFailed": "Failed to add to queue: {message}",
  "feed.category.label": "Category",
  "feed.category.none": "Uncategorized",
  "feed.category.placeholder": "New category name",
  "feed.category.add": "Add category",
  "feed.categoryAdded": "Added category \"{name}\"",
  "feed.channels.title": "Subscribed channels",
  "feed.channels.filter": "Category to show",
  "feed.channels.empty": "No channels in this category",
  "feed.channels.showFeed": "Show this channel's feed",
  "feed.items.title": "Feed",
  "feed.items.empty": "No feed items",
  "feed.items.markRead": "Mark read",
  "feed.items.markAllRead": "Mark all read",
  "feed.items.loadMore": "Load more",
  "feed.channelFilter.label": "Channel: {title}",
  "feed.channelFilter.clear": "Clear channel filter",
  "feed.backfill": "Fetch this channel's past videos",
  "feed.backfilling": "Fetching past videos…",
  "feed.backfilled": "Imported {count} past videos",
  "feed.backfillPartial": "Fetching failed in some tabs: {message}",
  "feed.backfillFailed": "Failed to fetch past videos: {message}",
  "feed.readMarkFailed":
    "Playback started but failed to mark as read: {message}",
  "feed.markedAllRead": "Marked all as read",
  "feed.filters.unreadOnly": "Unread only",
  "feed.filters.all": "All",
  "feed.filters.category": "Category",
  "feed.filters.kind": "Type",
  "feed.filters.kindVideo": "Videos",
  "feed.filters.kindShort": "Shorts",
  "feed.filters.kindLive": "Live",
  "feed.filters.groupByChannel": "By channel",
  "feed.refresh": "Refresh now",
  "feed.refreshQueued": "Refresh started",
  "feed.newItems": "Received {count} new items",
  "feed.statusEvent": "Feed status: {message}",
  "feed.item.block": "Block channel",

  // Phase 5: 検索・関連動画・チャンネルブロック
  "search.title": "Search",
  "search.placeholder": "Search keywords",
  "search.button": "Search",
  "search.searching": "Searching…",
  "search.empty": "No results",
  "search.failed": "Search failed: {message}",
  "search.play": "Play",
  "search.subscribe": "Subscribe",
  "search.block": "Block",
  "search.views.count": "{count} views",
  "search.views.man": "{count}M views",
  "search.views.oku": "{count}B views",
  "search.loadMore": "Load more",

  "related.title": "Related videos",
  "related.show": "Show related videos",
  "related.hide": "Close related videos",
  "related.loading": "Loading…",
  "related.empty": "No related videos found",
  "related.failed": "Failed to fetch related videos: {message}",

  // Phase 41: 再生中動画の概要欄表示（FR-33、仕様決定 BB）
  "desc.title": "Description",
  "desc.show": "Show description",
  "desc.hide": "Close description",
  "desc.loading": "Loading…",
  "desc.empty": "This video has no description",
  "desc.failed": "Failed to fetch the description: {message}",

  "blocked.title": "Blocked channels",
  "blocked.desc":
    "Videos from blocked channels are hidden from feeds, search, and related videos.",
  "blocked.empty": "No blocked channels",
  "blocked.added": "Blocked {title}",
  "blocked.addFailed": "Failed to block: {message}",
  "blocked.unblock": "Unblock",
  "blocked.unblocked": "Unblocked {title}",
  "blocked.unblockFailed": "Failed to unblock: {message}",

  // Phase 15: HDR・mpv 引数（仕様決定 Y、INV-1 調査結果の設定化）
  "settings.hdr.title": "HDR & mpv args",
  "settings.hdr.desc":
    "How HDR videos are converted and extra args for mpv. Applied from the next playback (effective for HDR→SDR conversion on SDR screens)",
  "settings.hdr.toneMapping": "Tone mapping",
  "settings.hdr.computePeak": "Peak brightness detection",
  "settings.hdr.auto": "Auto (mpv default)",
  "settings.hdr.yes": "Detect",
  "settings.hdr.no": "Do not detect",
  "settings.hdr.extraArgs": "Extra mpv args",
  "settings.hdr.extraArgs.desc":
    "Appended to the end of launch args, separated by spaces (e.g. --target-colorspace-hint=yes --gpu-api=d3d11). Values containing spaces can be quoted (e.g. --icc-profile=\"C:\\dir\\a.icc\"). Invalid args cause mpv launch failure",
  "settings.hdr.networkTimeout": "Stream read timeout (ms)",
  "settings.hdr.networkTimeout.desc":
    "Read wait time on unresponsive connections. Empty or invalid values fall back to the default 30000ms; 0 disables it. A countermeasure for mpv becoming unresponsive during live playback",
  "settings.hdr.hwdec": "HW decode mode",
  "settings.hdr.hwdec.desc":
    "Value passed to mpv's --hwdec. The default auto-safe is the same as mpv's default. In environments where mpv becomes unresponsive during playback (e.g. GPU driver deadlocks), setting no disables hardware decoding as a workaround at the cost of CPU load",
  "settings.hdr.unsaved":
    "HDR & mpv args values were changed while saving. The previous values were stored in the DB; please save again",

  // Phase 18: アプリの自動更新（FR-15、仕様決定 AB）
  "update.title": "App update",
  "update.body":
    "Version {version} is available. Download, apply, and restart?",
  "update.apply": "Update",
  "update.later": "Later",
  "update.installing": "Applying the update…",
  "update.found": "An update to version {version} is available",
  "update.upToDate": "Up to date",
  "update.failed": "Failed to check for updates: {message}",
  "update.installFailed": "Failed to apply the update: {message}",
  "settings.update.title": "App update",
  "settings.update.desc":
    "AppImage / NSIS builds support auto-update. It also checks automatically at startup (deb / rpm builds are not covered)",
  "settings.update.check": "Check now",
  "settings.update.checking": "Checking…",

  // Phase 3: SponsorBlock のカテゴリ設定
  "settings.sponsor.title": "SponsorBlock",
  "settings.sponsor.desc":
    "Choose the action when entering a segment of each category. Changes take effect from the next playback",
  "settings.sponsor.action.skip": "Skip",
  "settings.sponsor.action.notify": "Notify only",
  "settings.sponsor.action.off": "Off",
  // Phase 47: シークバーへの区間帯表示（FR-36、仕様決定 BI）
  "settings.sponsor.showSegments": "Show segments on the seek bar",
  "sponsor.cat.sponsor": "Ads & sponsored segments",
  "sponsor.cat.selfpromo": "Self-promotion / free promotion",
  "sponsor.cat.interaction": "Subscribe/like reminders",
  "sponsor.cat.intro": "Intro",
  "sponsor.cat.outro": "Outro (end cards)",
  "sponsor.cat.preview": "Preview / recap / hook",
  "sponsor.cat.poi_highlight": "Highlight",
  "sponsor.cat.music_offtopic": "Off-topic parts in music videos",
  "sponsor.cat.filler": "Filler / tangents",

  "sponsor.skipped": "SponsorBlock: skipped a {category} segment",
  "sponsor.notified":
    "SponsorBlock: detected a {category} segment (notify only)",

  // Phase 6: ライブチャット・NG フィルタ・履歴検索
  "chat.show": "Show chat",
  "chat.hide": "Close chat",
  "chat.popup": "Popup",
  "chat.popup.hint":
    "Opens the chat in a separate window (this panel closes)",
  "chat.pin": "Always on top",
  "chat.pin.hint": "Pins the popup on top",
  "chat.toPanel": "Back to panel",
  "chat.title": "Live chat",
  "chat.empty": "No messages",
  "chat.deleted": "This message was deleted",
  "chat.anonymous": "Anonymous",
  "chat.membership": "Member",
  "chat.status": "Chat: {message}",

  "settings.filters.title": "NG filters",
  "settings.filters.desc":
    "In addition to chat text and authors, video titles, channel names, and channel IDs can be targeted (excluded from feed, search, and related lists). The \"highlight\" kind does not remove chat messages but displays them in an accent color. Additions and removals take effect immediately.",
  "settings.filters.target": "Target",
  "settings.filters.kind": "Kind",
  "settings.filters.pattern.placeholder": "Pattern (NG word / regex)",
  "settings.filters.add": "Add",
  "settings.filters.added": "Filter added",
  "settings.filters.addFailed": "Failed to add filter: {message}",
  "settings.filters.remove": "Remove",
  "settings.filters.removed": "Filter removed",
  "settings.filters.removeFailed": "Failed to remove filter: {message}",
  "settings.filters.empty": "No filters",
  // Phase 68: NG ユーザー節（FR-70、仕様決定 CU）。chat_author フィルタの専用管理
  "settings.ngusers.title": "NG users",
  "settings.ngusers.desc":
    "Hides chat messages whose author name or channel ID matches the registered entries. Additions and removals take effect immediately (registers a substring chat_author filter; regex filters can be registered in the NG filters section)",
  "settings.ngusers.placeholder": "Author name or channel ID",
  "settings.ngusers.add": "Add",
  "settings.ngusers.added": "NG user added",
  "settings.ngusers.addFailed": "Failed to add NG user: {message}",
  "settings.ngusers.remove": "Remove",
  "settings.ngusers.removed": "NG user removed",
  "settings.ngusers.removeFailed": "Failed to remove NG user: {message}",
  "settings.ngusers.empty": "No NG users",
  "filter.target.video_title": "Video title",
  "filter.target.video_desc": "Video description",
  "filter.target.channel_title": "Channel name",
  "filter.target.channel_id": "Channel ID",
  "filter.target.chat_text": "Chat text",
  "filter.target.chat_author": "Chat author",
  "filter.kind.literal": "Substring",
  "filter.kind.regex": "Regex",
  "filter.kind.highlight": "Highlight",

  // Phase 7: ローカルデータ（履歴・お気に入り・プレイリスト）
  "library.title": "Library",
  "library.tab.history": "History",
  "library.tab.favorites": "Favorites",
  "library.tab.playlists": "Playlists",
  "library.play": "Play",
  "library.playFromStart": "Play from start",
  "library.remove": "Remove",
  "library.failed": "Operation failed: {message}",
  "library.history.empty": "No watch history",
  "library.history.progress": "{percent}% watched",
  "library.history.completed": "Watched",
  "library.history.lastWatched": "Last watched {at}",
  // Phase 61: 履歴一覧のページングと検索（FR-52・FR-61、仕様決定 CC・CL）
  "library.history.loadMore": "Load more",
  "library.history.searchPlaceholder": "Filter by title or channel name",
  "library.history.searchEmpty": "No matching history",
  "library.favorite.add": "Add to favorites",
  "library.favorite.remove": "Remove from favorites",
  "library.favorite.added": "Added to favorites",
  "library.favorite.removed": "Removed from favorites",
  "library.favorite.failed": "Favorite operation failed: {message}",
  "library.favorite.empty": "No favorites",
  "library.playlist.addTo": "Add to playlist",
  "library.playlist.added": "Added to \"{name}\"",
  "library.playlist.addFailed": "Failed to add to playlist: {message}",
  "library.playlist.none": "No playlists",
  "library.playlist.newPlaceholder": "New playlist name",
  "library.playlist.createAdd": "Create & add",
  "library.playlist.create": "Create",
  "library.playlist.created": "Created playlist \"{name}\"",
  "library.playlist.createFailed": "Failed to create playlist: {message}",
  "library.playlist.rename": "Rename",
  "library.playlist.renamed": "Renamed",
  "library.playlist.delete": "Delete",
  "library.playlist.deleted": "Playlist deleted",
  "library.playlist.deleteFailed": "Delete failed: {message}",
  "library.playlist.items": "{count} items",
  "library.playlist.loadMore": "Load more",
  "library.playlist.empty": "No videos registered",
  "library.playlist.selectHint": "Select a playlist on the left",
  "library.playlists.empty": "No playlists",
  "library.playlist.importUrl": "YouTube playlist URL",
  "library.playlist.importName": "Playlist name (optional)",
  "library.playlist.import": "Import",
  "library.playlist.importing": "Importing…",
  "library.playlist.imported": "Imported \"{name}\" ({count} items)",
  "library.playlist.importFailed": "Import failed: {message}",
  "library.playlist.sort": "Sort by published date",
  "library.playlist.sorted": "Sorted by published date",
  "library.playlist.reverse": "Reverse order",
  "library.playlist.reversed": "Reversed item order",
  "library.playlist.moveUp": "Up",
  "library.playlist.moveDown": "Down",
  "library.playlist.reorderFailed": "Failed to reorder: {message}",
  "library.playlist.dragHint": "Drag to reorder",
  "library.playlist.queue": "Play continuously from here",
  "library.playlist.queueFailed":
    "Failed to start continuous playback: {message}",
  "library.playlist.queueActive":
    "Playing continuously: {name} ({index}/{count})",
  "library.playlist.queueSaved": "Queue paused: {name} ({index}/{count})",
  "library.playlist.queueStop": "Stop continuous playback",
  "queue.tab.temp": "Temporary queue",
  "queue.add": "Add to queue",
  "queue.playNext": "Play next",
  "queue.added": "Added to queue",
  "queue.addedNext": "Added to play next",
  "queue.show": "Queue",
  "queue.panel.title": "Queue",
  "queue.play": "Play",
  "queue.clear": "Clear all",
  "queue.shuffle": "Shuffle rest",
  "queue.saveAsPlaylist": "Save as playlist",
  "queue.savedAsPlaylist": "Saved to playlist \"{name}\"",
  "queue.saveAsPlaylistFailed": "Failed to save playlist: {message}",
  "queue.empty": "Queue is empty",
  "queue.startFailed": "Failed to start queue playback: {message}",
  "queue.persistFailed": "Failed to save queue: {message}",
  "queue.restoreFailed": "Failed to restore queue: {message}",
  "library.removed": "Removed",
  "library.removeFailed": "Remove failed: {message}",
  // Phase 63: 削除の取り消し（FR-54、仕様決定 CE）。
  // トーストのアクションボタン「元に戻す」で削除前スナップショットを復元する
  "library.undo": "Undo",
  "library.restored": "Restored",
  "library.restoreFailed": "Restore failed: {message}",

  // Phase 64: あとで見るとライブラリ横断検索（FR-59・FR-60、仕様決定 CJ・CK）
  "library.tab.watchLater": "Watch later",
  "library.tab.search": "Search",
  "library.watchLater.add": "Watch later",
  "library.watchLater.remove": "Remove from watch later",
  "library.watchLater.added": "Added to watch later",
  "library.watchLater.removed": "Removed from watch later",
  "library.watchLater.failed": "Watch later operation failed: {message}",
  "library.watchLater.empty": "No watch later items",
  "library.search.placeholder":
    "Search history, favorites, playlists, and watch later",
  "library.search.hint":
    "Search the whole library by title or channel name",
  "library.search.empty": "No matching items",
  "library.search.source.history": "History",
  "library.search.source.favorites": "Favorites",
  "library.search.source.playlists": "Playlists",
  "library.search.source.watchLater": "Watch later",

  // Phase 7: ホイール割り当て（決定記録: script-opts 注入のため次回再生から有効）
  "settings.wheel.title": "Wheel bindings",
  "settings.wheel.desc":
    "mpv's wheel actions are \"frame step while paused, volume while playing\". This sets the volume change per notch while playing. Passed as an mpv launch option, so changes take effect from the next playback",
  "settings.wheel.volumeDelta": "Volume change (per notch)",

  // Phase 52: 再生の初期速度と初期音量（FR-39、仕様決定 BO）
  "settings.player.title": "Playback",
  "settings.player.desc":
    "Initial values for newly started instances. Passed as mpv launch args, so changes take effect from the next playback (speed and volume of running instances can be changed on their cards, but only for the session)",
  "settings.player.initialSpeed": "Initial speed",
  "settings.player.initialSpeed.desc":
    "Value passed to mpv's --speed (0.25-4.0, default 1.0). Empty restores the default",
  "settings.player.initialVolume": "Initial volume (%)",
  "settings.player.initialVolume.desc":
    "Value passed to mpv's --volume (0-130, default 100). Empty restores the default",
  "settings.player.unsaved":
    "Playback settings were changed while saving. Please save again",

  // Phase 8: PiP 小窓の位置（次回の PiP 化から有効）
  "settings.pip.title": "PiP",
  "settings.pip.desc":
    "Position and size of the PiP window (always-on-top, frameless). mpv --geometry format (e.g. 480x270-40-40 means width 480, height 270, 40px inside from the bottom-right). Effective from the next PiP activation",
  "settings.pip.default": "Play in PiP by default",
  "settings.pip.default.desc":
    "When on, all playback (including the start of continuous playback and external launches) starts in a PiP window. It can be toggled on the player card after startup",
  "settings.pip.fitAspect": "Fit the window size to the video aspect ratio",
  "settings.pip.fitAspect.desc":
    "When on, the window width/height are treated as a bounding box and the size is auto-adjusted to fit the video's aspect (e.g. in a 480x270 box a landscape video becomes ~480x201 and a portrait video ~152x270). Off always uses the specified size",
  "settings.pip.geometry":
    "Window position and size (WxH+±x±y; bounding box when fitting)",
  "settings.pip.quality": "Default PiP quality (ytdl-format expression)",
  "settings.pip.quality.desc":
    "Follows the global quality setting when empty. Specifying a lower quality for the small window makes startup lighter",
  "settings.pip.unsaved":
    "PiP values were changed while saving. Default values were stored in the DB; please save again",

  // Phase 39: 再生窓の起動ディスプレイ（FR-31、仕様決定 AZ）
  "settings.window.title": "Playback window",
  "settings.window.desc":
    "Selects the display on which mpv playback windows open. Effective from the next playback (PiP window position and size are set in the PiP section)",
  "settings.window.screen": "Startup display",
  "settings.window.screen.auto": "Auto (mpv default)",
  "settings.window.screen.desc":
    "Opens the playback window on the selected monitor. Falls back to auto when the selected monitor is disconnected",
  "settings.window.screen.none": "Could not enumerate connected monitors",
  "settings.window.screen.missing": "{name} (currently disconnected)",
  "settings.window.monitor": "{name} ({dims})",
  "settings.window.monitorPrimary": "{name} ({dims}, primary)",
  // Phase 55: 再生完了時の自動クローズ（FR-43、仕様決定 BS）
  "settings.window.closeOnEof":
    "Close window automatically when playback completes",
  "settings.window.closeOnEof.desc":
    "When on, playback to the end (eof) without continuous playback closes the mpv window and player card automatically. Off (default) keeps them as before. Early or error termination always closes them regardless of this setting",
  "settings.window.unsaved":
    "Playback window values were changed while saving. Please save again",

  // Phase 29: フィード一覧の shorts 既定非表示（FR-23、仕様決定 AP）
  "settings.feed.title": "Feed",
  "settings.feed.desc":
    "Feed list display, new-item notifications, and auto-read settings. List display applies from the next list fetch; notifications and auto-read apply from the next feed refresh",
  "settings.feed.showShorts": "Show Shorts in the feed",
  "settings.feed.showShorts.desc":
    "When off, the \"All\" and \"Videos\" selections in the feed list exclude Shorts. Selecting \"Shorts\" in the type filter always shows them",
  // Phase 65: 新着のデスクトップ通知と自動既読（FR-58・FR-64、仕様決定 CI・CO）
  "settings.feed.notifyNew": "Desktop-notify new items",
  "settings.feed.notifyNew.desc":
    "When on, an OS desktop notification is posted when new items are detected on feed refresh (off by default)",
  "settings.feed.autoReadPlayed": "Auto-mark played items as read",
  "settings.feed.autoReadPlayed.desc":
    "When on, feed items with watch history are automatically marked as read (off by default). Applied at startup and after each feed refresh",
  "settings.feed.autoReadDays":
    "Mark items read after this many days since posting",
  "settings.feed.autoReadDays.desc":
    "Automatically marks feed items older than the specified number of days as read. 0 (default) disables it. Applied at startup and after each feed refresh",
  "settings.feed.unsaved":
    "Feed values were changed while saving. Please save again",

  // Phase 56: ライブ配信の待機（FR-44、仕様決定 BT）
  "livewait.add": "Wait for stream",
  "livewait.waiting": "Waiting",
  "livewait.remove": "Stop waiting",
  "livewait.added": "Waiting for stream: {title}",
  "livewait.duplicate": "This video is already being waited on",
  "livewait.live": "This video is already live (not registered)",
  "livewait.playNow": "Play now",
  "livewait.ended": "This stream has ended (not registered)",
  "livewait.notBroadcast":
    "Not a scheduled live or premiere (not registered)",
  "livewait.addFailed": "Failed to register live wait: {message}",
  "livewait.removed": "Stopped waiting",
  "livewait.removeFailed": "Failed to stop waiting: {message}",
  "livewait.listFailed": "Failed to fetch the wait list: {message}",
  "livewait.tab": "Waiting",
  "livewait.empty": "No streams being waited on",
  "livewait.scheduledUnknown": "Scheduled start unknown",
  "livewait.status": "Live wait: {message}",

  // Phase 56: 設定画面のライブ待機セクション（live_wait.action）
  "settings.livewait.title": "Live wait",
  "settings.livewait.desc":
    "Action when a waited-on stream starts. Applied from the next start detection",
  "settings.livewait.action": "Action on stream start",
  "settings.livewait.action.play": "Auto-play in a new instance",
  "settings.livewait.action.notify": "Toast notification only",
  "settings.livewait.unsaved":
    "Live wait values were changed while saving. Please save again",

  // Phase 67: チャットポップアップの表示カスタマイズ（FR-68、仕様決定 CS）
  "settings.chatpopup.title": "Chat popup",
  "settings.chatpopup.desc":
    "Appearance of the chat opened in a separate window (popup). Applied immediately to open popups too",
  "settings.chatpopup.showAuthor": "Show author name",
  "settings.chatpopup.showAuthor.desc":
    "Off (default) shows only the timestamp and text. superchat / membership badges are not shown even with author names on",
  "settings.chatpopup.fontSize": "Font size (px)",
  "settings.chatpopup.fontSize.desc":
    "A number between 10 and 32. Empty restores the default 14",
  "settings.chatpopup.opacity": "Opacity",
  "settings.chatpopup.opacity.desc":
    "A number between 0.2 and 1.0 (smaller is more transparent). Empty restores the default 1.0",
  "settings.chatpopup.unsaved":
    "Chat popup values were changed while saving. Please save again",

  // Phase 71: テーマと表示密度（FR-76・FR-77、仕様決定 DA・DB）
  "settings.ui.title": "Appearance",
  "settings.ui.desc":
    "Color theme, list density, and UI language. Applied to the display immediately when saved",
  "settings.ui.theme": "Color theme",
  "settings.ui.theme.dark": "Dark",
  "settings.ui.theme.light": "Light",
  "settings.ui.theme.system": "Follow system",
  "settings.ui.density": "List density",
  "settings.ui.density.comfortable": "Comfortable",
  "settings.ui.density.compact": "Compact",
  // Phase 74: UI 言語（FR-81、仕様決定 DF）
  "settings.ui.language": "UI language",
  "settings.ui.language.ja": "日本語",
  "settings.ui.language.en": "English",
  "settings.ui.unsaved":
    "Appearance settings were changed while saving. Please save again",

  // Phase 72: キーボードショートカットの有効化（FR-79、仕様決定 DD）
  "settings.shortcuts.title": "Shortcuts",
  "settings.shortcuts.desc":
    "Default key map for operating lists and the queue by keyboard. Saving toggles it on/off (off by default)",
  "settings.shortcuts.enabled": "Enable keyboard shortcuts",
  "settings.shortcuts.enabled.desc":
    "On video rows in lists you can use j / Down (next row), k / Up (previous row), Enter (play selection), a (add selection to queue), and q (toggle queue panel). Disabled while editing inputs or while the context menu is open",
  "settings.shortcuts.unsaved":
    "Shortcut values were changed while saving. Please save again",

  // Phase 59: 動画内ブックマーク（FR-49、仕様決定 BZ）
  "bookmark.add": "Bookmark",
  "bookmark.add.hint": "Save the current position to bookmarks",
  "bookmark.tab": "Bookmarks",
  "bookmark.empty": "No bookmarks",
  "bookmark.saved": "Bookmark saved: {position}",
  "bookmark.duplicate": "This position is already saved: {position}",
  "bookmark.addFailed": "Failed to save bookmark: {message}",
  "bookmark.removed": "Bookmark removed",
  "bookmark.removeFailed": "Failed to remove bookmark: {message}",
  "bookmark.remove": "Remove bookmark",
  "bookmark.listFailed": "Failed to fetch bookmarks: {message}",
  "bookmark.play": "Play from this position",
  "bookmark.playFailed": "Failed to play from bookmark position: {message}",
  "bookmark.edit": "Edit",
  "bookmark.edit.hint": "Edit label",
  "bookmark.labelSave": "Save",
  "bookmark.labelSaved": "Label updated",
  "bookmark.labelFailed": "Failed to update label: {message}",

  // Phase 66: 視聴統計タブ（FR-65、仕様決定 CP）
  "library.tab.stats": "Stats",
  "library.stats.total": "Total watch time",
  "library.stats.entries": "{count} history entries",
  "library.stats.byChannel": "By channel",
  "library.stats.byDay": "By day",
  "library.stats.unknownChannel": "(unknown)",
  "library.stats.empty": "No watch history yet",
  "library.stats.failed": "Failed to fetch stats: {message}",
  "library.stats.refresh": "Reload",

  // Phase 66: 設定画面のカテゴリ並べ替え（FR-63、仕様決定 CN）
  "settings.categories.title": "Categories",
  "settings.categories.desc":
    "Display order of categories in the feed. Reorder with the up/down buttons or drag & drop (same operation as playlists; changes are saved immediately)",
  "settings.categories.empty": "No categories",
  "settings.categories.moveUp": "Up",
  "settings.categories.moveDown": "Down",
  "settings.categories.dragHint": "Drag to reorder",
  "settings.categories.reorderFailed": "Failed to reorder: {message}",

  // Phase 66: 設定画面の運用節（FR-62・FR-66、仕様決定 CM・CQ）
  "settings.ops.title": "Operations",
  "settings.ops.desc":
    "Data export/import and database maintenance",
  "settings.ops.export": "Export data",
  "settings.ops.export.desc":
    "Writes subscribed channels, categories, settings, and playlists to a JSON file",
  "settings.ops.exported": "Exported: {path}",
  "settings.ops.exportFailed": "Export failed: {message}",
  "settings.ops.import": "Import data",
  "settings.ops.import.desc":
    "Reads an exported JSON file. Subscriptions, categories, settings, and playlists are replaced by the file contents",
  "settings.ops.importConfirm":
    "Current subscriptions, categories, settings, and playlists will be replaced by the file contents. Continue?",
  "settings.ops.imported": "Imported: {path}",
  "settings.ops.importFailed": "Import failed: {message}",
  "settings.ops.dbSize": "Database size",
  "settings.ops.dbSize.desc":
    "Total of the DB file and WAL files (maintenance guide)",
  "settings.ops.dbSize.failed": "Failed to get size: {message}",
  "settings.ops.vacuum": "VACUUM",
  "settings.ops.vacuum.desc":
    "Rebuilds the database to shrink free space",
  "settings.ops.vacuumDone": "VACUUM done ({before} → {after})",
  "settings.ops.vacuumFailed": "VACUUM failed: {message}",
  "settings.ops.cleanup": "Clean orphan rows",
  "settings.ops.cleanup.desc":
    "Deletes videos left after unsubscribing and item rows with no referents (history is not removed)",
  "settings.ops.cleanupConfirm": "Delete orphan rows. Continue?",
  "settings.ops.cleanupDone": "Deleted {count} orphan rows",
  "settings.ops.cleanupFailed": "Cleanup failed: {message}",
  "settings.ops.diag": "Export diagnostics",
  "settings.ops.diag.desc":
    "Writes app logs, settings, and environment info into a single zip. Signed URLs, tokens, and user names in paths are masked (FR-82)",
  "settings.ops.diagExported": "Diagnostics exported: {path}",
  "settings.ops.diagFailed": "Failed to export diagnostics: {message}",
  "settings.ops.busy": "Running…",

  // Phase 70: 設定と依存診断の拡充（FR-72・FR-73・FR-74・FR-75、仕様決定 CW・CX・CY・CZ）
  "settings.filter.placeholder": "Filter sections (e.g. quality, mpv, feed)",
  "settings.filter.noMatch": "No matching sections",
  "settings.deps.title": "Dependent commands",
  "settings.deps.desc":
    "Settings for external commands such as mpv, yt-dlp, and deno. See the \"Diagnostics\" section for detection status",
  "settings.deps.ytdlpPath": "yt-dlp path",
  "settings.deps.ytdlpPath.placeholder": "Auto-detect (empty)",
  "settings.deps.ytdlpPath.desc":
    "Specify the yt-dlp executable directly. Empty means auto-detect (bundled resource → PATH, in that order). Changes take effect from the next yt-dlp invocation after saving",
  "settings.deps.unsaved":
    "Dependent command values were changed while saving. Please save again",
  "settings.diag.title": "Diagnostics",
  "settings.diag.desc":
    "Detection status, versions, and resolved paths of external commands. The same list is shown on first launch",
  "deps.detected": "Detected",
  "deps.missing": "Not found",
  "deps.refresh": "Re-detect",
  "deps.refreshing": "Detecting…",
  "deps.failed": "Failed to detect dependent commands: {message}",
  "setup.title": "Initial setup",
  "setup.desc":
    "Detection status of external commands required by yt-browser. Install any missing commands or set their paths in the \"Dependent commands\" section of settings. This list can be re-checked anytime in the \"Diagnostics\" section of settings",
  "setup.ok": "Get started",

  // Phase 75: 通知履歴（FR-83、仕様決定 DH）
  "settings.notices.title": "Notification history",
  "settings.notices.desc":
    "Notifications issued during this session. Up to {max} most recent entries are kept and cleared when the app restarts",
  "settings.notices.empty": "No notifications have been issued in this session",
  "settings.notices.action": "Action: {label}",
  "settings.notices.clear": "Clear history",
} satisfies Record<MessageKey, string>;

const MESSAGES: Record<Locale, Partial<Record<MessageKey, string>>> = {
  ja,
  en,
};

export function setLocale(locale: Locale): void {
  i18nLocale.current = locale;
}

/// 現行ロケール。フォーマッタなど表示系の言語分岐に使う
export function currentLocale(): Locale {
  return i18nLocale.current;
}

type Params = Record<string, string | number>;

/// 指定ロケールでメッセージキーを解決する純粋関数（t() の解決実体）。
/// locale に無いキーは ja へフォールバックし、それでも無ければキー名を返す。
/// 網羅・フォールバックの検証（LN-NN 系テスト）から直接使える
export function resolveMessage(locale: Locale, key: MessageKey): string {
  return MESSAGES[locale][key] ?? MESSAGES.ja[key] ?? key;
}

/// メッセージキーから表示文字列を得る。`{name}` 形式のプレースホルダーを params で埋める。
/// 現行ロケールに無いキーは ja へフォールバックし、それでも無ければキー名を返す。
/// i18nLocale.current は $state のため、テンプレート内の呼び出しは
/// ロケール変更で即時に再評価される
export function t(key: MessageKey, params?: Params): string {
  const template = resolveMessage(i18nLocale.current, key);
  if (!params) return template;
  return template.replace(/\{(\w+)\}/g, (_, name: string) =>
    String(params[name] ?? `{${name}}`),
  );
}

/// 登録されている全メッセージキー（ja が正）。
/// リソース網羅の検証（LN-NN 系テスト）に使う
export function messageKeys(): MessageKey[] {
  return Object.keys(MESSAGES.ja) as MessageKey[];
}

/// 指定ロケールで未登録（ja へフォールバックする）キーの一覧。
/// リソース網羅の検証（LN-NN 系テスト）に使う
export function fallbackKeys(locale: Locale): MessageKey[] {
  return messageKeys().filter((k) => MESSAGES[locale][k] === undefined);
}

// トーンマッピング選択肢 ID → mpv の --tone-mapping 値。
// ドット入り値（bt.2390 / bt.2446a）を持つエントリだけを置く。
// check_docs_consistency の i18n 参照検査はこのファイルを対象外とするため、
// ドット入りリテラルはここに置き、選択肢側はドット無し ID を使う
// （src/lib/settings-consts.ts の TONE_MAPPINGS を参照）
export const TONE_MAPPING_MPV: Record<string, string> = {
  bt2390: "bt.2390",
  bt2446a: "bt.2446a",
};
