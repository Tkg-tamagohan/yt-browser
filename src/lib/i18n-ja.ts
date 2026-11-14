// UI 文字列の日本語リソース（正・全キー網羅）。
// scripts/check_docs_consistency.py の check_i18n がここから全キーを抽出する。
export const ja = {
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
  "player.quality.hint": "この再生限り有効（次回は既定画質）",
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
  "player.pip.hint": "mpv ショートカット: q で終了 / m でミュート",

  // Phase 53: PiP 窓の一括整列（FR-41、仕様決定 BQ）
  "player.pipTile": "PiP を整列",
  "player.pipTile.hint":
    "起動ディスプレイ設定のモニターへ整列（自動はプライマリ）",
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
  "player.window.hint": "既定設定は変わりません",

  // Phase 42: 再生中動画のタイトルとリンクのコピー（FR-34、仕様決定 BC）
  "player.copyLink": "リンクをコピー",
  "player.copyLink.hint": "タイトルと URL をコピー",
  "player.linkCopied": "タイトルとリンクをコピーしました",
  "player.copyLinkFailed": "リンクのコピーに失敗: {message}",

  // Phase 40: ウインドウごとの音声出力先（FR-32、仕様決定 BA）
  "player.audio": "音声出力",
  "player.audio.hint": "この再生限り有効（次回は自動）",
  "player.audio.placeholder": "デバイス名",

  // Phase 52: 字幕トラックの選択（FR-40、仕様決定 BP）
  "player.subtitle": "字幕",
  "player.subtitle.hint": "この再生限り有効（連続再生の次動画は自動）",
  "player.subtitle.none": "なし",
  "player.subtitle.auto": "自動",

  // Phase 17: ループ再生（FR-16、仕様決定 AA）
  "player.loop.none": "ループ: なし",
  "player.loop.all": "ループ: 全体",
  "player.loop.one": "ループ: 1 項目",

  // Phase 51: A-B リピート（FR-37、仕様決定 BM）
  "player.ab.off": "A-B: なし",
  "player.ab.a": "A-B: A 点",
  "player.ab.loop": "A-B: ループ中",

  // Phase 51: スクリーンショット（FR-38、仕様決定 BN）
  "player.screenshot": "スクリーンショット",
  "player.screenshot.hint": "字幕込み PNG で保存",
  "player.screenshot.saved": "スクリーンショットを保存しました: {path}",
  "player.screenshot.failed": "スクリーンショットの保存に失敗: {message}",
  "player.screenshot.openFolder": "フォルダを開く",
  // mpv 窓の前面化（FR-42、仕様決定 BR）
  "player.focus": "前面に出す",
  "player.focus.wayland":
    "Wayland では mpv 窓の前面化に対応していません（プロトコルの制約）",
  "player.focus.other": "この表示環境では mpv 窓の前面化に対応していません",
  "player.focus.failed": "mpv 窓を前面に出せませんでした: {message}",

  "ytdlp.checking": "yt-dlp を確認中…",
  "ytdlp.ok": "yt-dlp {version}（{path}）",
  "ytdlp.missing":
    "yt-dlp が見つかりません（インストールまたは設定「yt-dlp のパス」で指定）",
  "ytdlp.update": "yt-dlp 更新",
  "ytdlp.updating": "更新中…",
  "ytdlp.updated": "yt-dlp 更新: {output}",
  "ytdlp.updateFailed": "yt-dlp 更新失敗: {message}",

  // Phase 2: 設定画面（画質プリセット）
  "settings.title": "設定",
  "settings.quality.title": "画質（ytdl-format）",
  "settings.quality.desc": "保存すると再生中の動画にも即時反映",
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
  "feed.category.placeholder": "カテゴリ名",
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
  "blocked.desc": "フィード・検索・関連動画から非表示",
  "blocked.empty": "ブロック中のチャンネルはありません",
  "blocked.added": "{title} をブロックしました",
  "blocked.addFailed": "ブロックに失敗: {message}",
  "blocked.unblock": "解除",
  "blocked.unblocked": "{title} のブロックを解除しました",
  "blocked.unblockFailed": "ブロック解除に失敗: {message}",

  // Phase 15: HDR・mpv 引数（仕様決定 Y、INV-1 調査結果の設定化）
  "settings.hdr.title": "HDR・mpv 引数",
  "settings.hdr.desc":
    "変更は次回再生から反映（SDR 画面では HDR→SDR 変換に適用）",
  "settings.hdr.toneMapping": "トーンマッピング方式",
  "settings.hdr.computePeak": "ピーク輝度の計測",
  "settings.hdr.auto": "自動（mpv 既定）",
  "settings.hdr.yes": "計測する",
  "settings.hdr.no": "計測しない",
  "settings.hdr.extraArgs": "mpv 追加引数",
  "settings.hdr.extraArgs.desc":
    "空白区切り（空白を含む値は引用符で囲む）。無効な引数は mpv 起動に失敗します",
  "settings.hdr.networkTimeout": "ストリーム読み取りタイムアウト (ms)",
  "settings.hdr.networkTimeout.desc":
    "0 で無効（空欄・不正値は既定 30000ms）",
  "settings.hdr.hwdec": "HW デコード方式",
  "settings.hdr.hwdec.desc":
    "no でハードウェアデコードを無効化（CPU 負荷増）",
  "settings.hdr.unsaved":
    "保存中に HDR・mpv 引数が変更され、変更前の値が保存されました（要再保存）",

  // Phase 18: アプリの自動更新（FR-15、仕様決定 AB）
  "update.title": "アプリの更新",
  "update.body":
    "バージョン {version} をダウンロード・適用して再起動します。",
  "update.apply": "更新する",
  "update.later": "後で",
  "update.installing": "更新を適用しています…",
  "update.found": "バージョン {version} の更新があります",
  "update.upToDate": "最新のバージョンです",
  "update.failed": "更新の確認に失敗: {message}",
  "update.installFailed": "更新の適用に失敗: {message}",
  "settings.update.title": "アプリの更新",
  "settings.update.desc":
    "起動時に自動確認（自動更新は AppImage / NSIS 版のみ対応）",
  "settings.update.check": "今すぐ確認",
  "settings.update.checking": "確認中…",

  // Phase 3: SponsorBlock のカテゴリ設定
  "settings.sponsor.title": "SponsorBlock",
  "settings.sponsor.desc": "変更は次回再生から反映",
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
  "chat.popup.hint": "別窓で開く（このパネルは閉じます）",
  "chat.pin": "最前面",
  "chat.toPanel": "パネルに戻す",
  "chat.title": "ライブチャット",
  "chat.empty": "メッセージがありません",
  "chat.deleted": "このメッセージは削除されました",
  "chat.anonymous": "匿名",
  "chat.membership": "メンバー",
  "chat.status": "チャット: {message}",

  "settings.filters.title": "NG フィルタ",
  "settings.filters.desc":
    "「ハイライト」は除去せず強調表示。追加・削除は即時反映",
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
    "部分一致で登録・即時反映（正規表現は「NG フィルタ」で登録）",
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
  "library.history.searchPlaceholder": "タイトル・チャンネル名",
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
  "library.playlist.newPlaceholder": "プレイリスト名",
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
  "library.playlist.selectHint": "プレイリスト未選択",
  "library.playlists.empty": "プレイリストはありません",
  "library.playlist.importUrl": "例: https://www.youtube.com/playlist?list=...",
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
  "library.search.placeholder": "タイトル・チャンネル名",
  "library.search.empty": "一致する項目はありません",
  "library.search.source.history": "履歴",
  "library.search.source.favorites": "お気に入り",
  "library.search.source.playlists": "プレイリスト",
  "library.search.source.watchLater": "あとで見る",

  // Phase 7: ホイール割り当て（決定記録: script-opts 注入のため次回再生から有効）
  "settings.wheel.title": "ホイール割り当て",
  "settings.wheel.desc":
    "一時停止中はコマ送り、再生中は音量。変更は次回再生から反映",
  "settings.wheel.volumeDelta": "音量変化量（ノッチあたり）",

  // Phase 52: 再生の初期速度と初期音量（FR-39、仕様決定 BO）
  "settings.player.title": "再生",
  "settings.player.desc": "変更は次回再生から反映",
  "settings.player.initialSpeed": "初期速度",
  "settings.player.initialSpeed.desc": "0.25〜4.0（空欄で既定 1.0）",
  "settings.player.initialVolume": "初期音量（%）",
  "settings.player.initialVolume.desc": "0〜130（空欄で既定 100）",
  "settings.player.unsaved": "保存中に再生設定が変更されました（要再保存）",

  // Phase 8: PiP 小窓の位置（次回の PiP 化から有効）
  "settings.pip.title": "PiP",
  "settings.pip.desc": "変更は次回の PiP 化から反映",
  "settings.pip.default": "再生を既定で PiP 表示にする",
  "settings.pip.fitAspect": "小窓サイズを動画のアスペクト比に合わせる",
  "settings.pip.fitAspect.desc":
    "位置とサイズの幅・高さを上限枠として動画比率に内接調整",
  "settings.pip.geometry": "小窓の位置とサイズ（WxH+±x±y、追従時は上限枠）",
  "settings.pip.quality": "PiP の既定画質（ytdl-format 式）",
  "settings.pip.quality.desc": "空欄時は全体の画質設定を使用",
  "settings.pip.unsaved":
    "保存中に PiP が変更され、既定値が保存されました（要再保存）",

  // Phase 39: 再生窓の起動ディスプレイ（FR-31、仕様決定 AZ）
  "settings.window.title": "再生窓",
  "settings.window.desc": "変更は次回再生から反映",
  "settings.window.screen": "起動ディスプレイ",
  "settings.window.screen.auto": "自動（mpv 既定）",
  "settings.window.screen.desc": "未接続時は自動にフォールバック",
  "settings.window.screen.none": "接続中のモニターを取得できませんでした",
  "settings.window.screen.missing": "{name}（現在は未接続）",
  "settings.window.monitor": "{name}（{dims}）",
  "settings.window.monitorPrimary": "{name}（{dims}・プライマリ）",
  // Phase 55: 再生完了時の自動クローズ（FR-43、仕様決定 BS）
  "settings.window.closeOnEof": "再生完了時に窓を自動で閉じる",
  "settings.window.closeOnEof.desc":
    "連続再生中は閉じません（途中終了・エラー時は設定に関わらず終了）",
  "settings.window.unsaved": "保存中に再生窓が変更されました（要再保存）",

  // Phase 29: フィード一覧の shorts 既定非表示（FR-23、仕様決定 AP）
  "settings.feed.title": "フィード",
  "settings.feed.desc":
    "一覧表示は次回取得から、通知・自動既読は次回フィード更新から反映",
  "settings.feed.showShorts": "フィードに Shorts を表示する",
  "settings.feed.showShorts.desc":
    "オフ時は「すべて」「動画」から除外（種別で Shorts 選択時は表示）",
  // Phase 65: 新着のデスクトップ通知と自動既読（FR-58・FR-64、仕様決定 CI・CO）
  "settings.feed.notifyNew": "新着をデスクトップ通知する",
  "settings.feed.autoReadPlayed": "再生した項目を自動で既読にする",
  "settings.feed.autoReadPlayed.desc": "起動時とフィード更新時に適用",
  "settings.feed.autoReadDays": "投稿からこの日数が経過した項目を既読にする",
  "settings.feed.autoReadDays.desc":
    "0 で無効（起動時とフィード更新時に適用）",
  "settings.feed.unsaved": "保存中にフィードが変更されました（要再保存）",

  // Phase 56: ライブ配信の待機（FR-44、仕様決定 BT）
  "livewait.add": "配信を待機",
  "livewait.waiting": "待機中",
  "livewait.remove": "待機を解除",
  "livewait.added": "配信を待機します: {title}",
  "livewait.duplicate": "この動画は既に待機中です",
  "livewait.live": "すでに放送中のため待機登録しません",
  "livewait.playNow": "再生する",
  "livewait.ended": "終了済みのため待機登録しません",
  "livewait.notBroadcast": "放送予定ではないため待機登録しません",
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
  "settings.livewait.desc": "次回の開始検出から反映",
  "settings.livewait.action": "配信開始時の動作",
  "settings.livewait.action.play": "新規インスタンスで自動再生",
  "settings.livewait.action.notify": "トースト通知のみ",
  "settings.livewait.unsaved": "保存中にライブ待機が変更されました（要再保存）",

  // Phase 67: チャットポップアップの表示カスタマイズ（FR-68、仕様決定 CS）
  "settings.chatpopup.title": "チャットポップアップ",
  "settings.chatpopup.desc": "開いているポップアップにも即時反映",
  "settings.chatpopup.showAuthor": "投稿者名を表示する",
  "settings.chatpopup.showAuthor.desc": "superchat / membership バッジは非表示",
  "settings.chatpopup.fontSize": "文字サイズ（px）",
  "settings.chatpopup.fontSize.desc": "10〜32（空欄で既定 14）",
  "settings.chatpopup.opacity": "不透明度",
  "settings.chatpopup.opacity.desc": "0.2〜1.0（空欄で既定 1.0）",
  "settings.chatpopup.unsaved":
    "保存中にチャットポップアップが変更されました（要再保存）",

  // Phase 71: テーマと表示密度（FR-76・FR-77、仕様決定 DA・DB）
  "settings.ui.title": "表示",
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
  "settings.ui.unsaved": "保存中に表示設定が変更されました（要再保存）",

  // Phase 72: キーボードショートカットの有効化（FR-79、仕様決定 DD）
  "settings.shortcuts.title": "ショートカット",
  "settings.shortcuts.enabled": "キーボードショートカットを有効にする",
  "settings.shortcuts.enabled.desc":
    "j/↓: 次行、k/↑: 前行、Enter: 再生、a: キュー追加、q: キュー開閉（入力中・メニュー表示中は無効）",
  "settings.shortcuts.unsaved":
    "保存中にショートカットが変更されました（要再保存）",

  // Phase 59: 動画内ブックマーク（FR-49、仕様決定 BZ）
  "bookmark.add": "ブックマーク",
  "bookmark.add.hint": "現在位置を保存",
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
  "settings.categories.desc": "変更は即時保存",
  "settings.categories.empty": "カテゴリはありません",
  "settings.categories.moveUp": "上へ",
  "settings.categories.moveDown": "下へ",
  "settings.categories.reorderFailed": "並べ替えに失敗: {message}",

  // Phase 66: 設定画面の運用節（FR-62・FR-66、仕様決定 CM・CQ）
  "settings.ops.title": "運用",
  "settings.ops.export": "エクスポート",
  "settings.ops.export.label": "データエクスポート",
  "settings.ops.export.desc":
    "購読チャンネル・カテゴリ・設定・プレイリストを JSON 出力",
  "settings.ops.exported": "エクスポートしました: {path}",
  "settings.ops.exportFailed": "エクスポートに失敗: {message}",
  "settings.ops.import": "インポート",
  "settings.ops.import.label": "データインポート",
  "settings.ops.import.desc":
    "現在の購読・カテゴリ・設定・プレイリストを上書き",
  "settings.ops.importConfirm":
    "現在の購読・カテゴリ・設定・プレイリストをファイルの内容で上書きします。この操作は取り消せません。",
  "settings.ops.imported": "インポートしました: {path}",
  "settings.ops.importFailed": "インポートに失敗: {message}",
  "settings.ops.dbSize": "データベースサイズ",
  "settings.ops.dbSize.desc": "DB 本体 + WAL の合計",
  "settings.ops.dbSize.failed": "サイズの取得に失敗: {message}",
  "settings.ops.vacuum": "VACUUM",
  "settings.ops.vacuum.label": "空き領域の縮小",
  "settings.ops.vacuum.desc": "データベースを再構築",
  "settings.ops.vacuumDone": "VACUUM を実行しました（{before} → {after}）",
  "settings.ops.vacuumFailed": "VACUUM に失敗: {message}",
  "settings.ops.cleanup": "孤児行整理",
  "settings.ops.cleanup.label": "未参照データの整理",
  "settings.ops.cleanup.desc": "未参照の動画・項目を削除（履歴は保持）",
  "settings.ops.cleanupConfirm":
    "未参照の動画・項目を削除します（履歴は保持）。この操作は取り消せません。",
  "settings.ops.cleanupDone": "{count} 件の孤児行を削除しました",
  "settings.ops.cleanupFailed": "孤児行整理に失敗: {message}",
  "settings.ops.diag": "診断情報",
  "settings.ops.diag.desc":
    "ログ・設定・環境情報を zip 出力（署名 URL・トークン・ユーザー名はマスク）",
  "settings.ops.diagExported": "診断情報をエクスポートしました: {path}",
  "settings.ops.diagFailed": "診断情報のエクスポートに失敗: {message}",
  "settings.ops.busy": "実行中…",

  // Phase 70: 設定と依存診断の拡充（FR-72・FR-73・FR-74・FR-75、仕様決定 CW・CX・CY・CZ）
  "settings.filter.placeholder": "節を絞り込み（例: 画質、mpv、フィード）",
  "settings.filter.noMatch": "一致する節がありません",
  "settings.deps.title": "依存コマンド",
  "settings.deps.ytdlpPath": "yt-dlp のパス",
  "settings.deps.ytdlpPath.placeholder": "自動検出（空欄）",
  "settings.deps.ytdlpPath.desc":
    "空欄で自動検出（同梱リソース → PATH）。保存後の次回呼び出しから反映",
  // Phase 77: PO Token の伝達（FR-86、仕様決定 DK）
  "settings.deps.poToken": "PO Token",
  "settings.deps.poToken.placeholder": "未設定（空欄）",
  "settings.deps.poToken.desc":
    "形式: mweb.gvs+<token> 等（yt-dlp PO Token Guide 準拠）。保存後の次回呼び出しから反映",
  "settings.deps.unsaved": "保存中に依存コマンドが変更されました（要再保存）",
  "settings.diag.title": "診断",
  "deps.detected": "検出",
  "deps.missing": "未検出",
  "deps.refresh": "再検出",
  "deps.refreshing": "検出中…",
  "deps.failed": "依存コマンドの検出に失敗: {message}",
  "setup.title": "初期セットアップ",
  "setup.desc":
    "未検出のコマンドはインストールするか設定「依存コマンド」で指定します。",
  "setup.ok": "はじめる",

  // Phase 75: 通知履歴（FR-83、仕様決定 DH）
  "settings.notices.title": "通知履歴",
  "settings.notices.desc": "新しい順に最大 {max} 件（再起動で消去）",
  "settings.notices.empty": "通知はありません",
  "settings.notices.action": "アクション: {label}",
  "settings.notices.clear": "履歴を消去",


} as const;

export type MessageKey = keyof typeof ja;
