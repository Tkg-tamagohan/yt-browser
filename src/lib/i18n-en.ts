import type { MessageKey } from "./i18n-ja";

// Phase 74: 英語リソース（FR-81、仕様決定 DF）。
// ja の全キーを網羅する。satisfies で網羅をコンパイル時に検査し、
// 実行時も未登録キーは t() が ja へフォールバックする（MESSAGES は Partial 型）。
export const en = {
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
  "player.quality.hint": "This session only (next playback uses the default)",
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
  "player.pip.hint": "mpv shortcuts: q to quit, m to mute",

  // Phase 53: PiP 窓の一括整列（FR-41、仕様決定 BQ）
  "player.pipTile": "Tile PiP windows",
  "player.pipTile.hint":
    "Tile on the configured startup display (primary when auto)",
  "player.pipTile.count": "PiP windows: {count}",
  "player.pipTile.done": "Tiled {count} PiP windows",
  "player.pipTile.partial": "Tiled {ok}/{total} windows (failed: {message})",
  "player.pipTile.failed": "Failed to tile PiP windows: {message}",
  "player.pipTile.noMonitor": "No target monitor found",
  "player.pipTile.wayland":
    "Window tiling is not supported on Wayland (a permanent mpv limitation)",

  // Phase 25: 再生の既定表示を PiP 化（FR-18、仕様決定 AJ）
  "player.playWindow": "Play in window",
  "player.window.hint": "Does not change the default setting",

  // Phase 42: 再生中動画のタイトルとリンクのコピー（FR-34、仕様決定 BC）
  "player.copyLink": "Copy link",
  "player.copyLink.hint": "Copy title and URL",
  "player.linkCopied": "Copied title and link",
  "player.copyLinkFailed": "Failed to copy link: {message}",

  // Phase 40: ウインドウごとの音声出力先（FR-32、仕様決定 BA）
  "player.audio": "Audio output",
  "player.audio.hint": "This session only (next playback reverts to auto)",
  "player.audio.placeholder": "Device name",

  // Phase 52: 字幕トラックの選択（FR-40、仕様決定 BP）
  "player.subtitle": "Subtitles",
  "player.subtitle.hint":
    "This session only (next video in queue reverts to auto)",
  "player.subtitle.none": "None",
  "player.subtitle.auto": "Auto",

  // Phase 17: ループ再生（FR-16、仕様決定 AA）
  "player.loop.none": "Loop: off",
  "player.loop.all": "Loop: all",
  "player.loop.one": "Loop: one",

  // Phase 51: A-B リピート（FR-37、仕様決定 BM）
  "player.ab.off": "A-B: off",
  "player.ab.a": "A-B: point A",
  "player.ab.loop": "A-B: looping",

  // Phase 51: スクリーンショット（FR-38、仕様決定 BN）
  "player.screenshot": "Screenshot",
  "player.screenshot.hint": "Save current frame as PNG with subtitles",
  "player.screenshot.saved": "Screenshot saved: {path}",
  "player.screenshot.failed": "Failed to save screenshot: {message}",
  "player.screenshot.openFolder": "Open folder",
  // mpv 窓の前面化（FR-42、仕様決定 BR）
  "player.focus": "Bring to front",
  "player.focus.wayland":
    "Raising the mpv window is not supported on Wayland (protocol limitation)",
  "player.focus.other":
    "Raising the mpv window is not supported in this display environment",
  "player.focus.failed": "Could not raise the mpv window: {message}",

  "ytdlp.checking": "Checking yt-dlp…",
  "ytdlp.ok": "yt-dlp {version} ({path})",
  "ytdlp.missing":
    "yt-dlp not found (install it or set \"yt-dlp path\" in Settings)",
  "ytdlp.update": "Update yt-dlp",
  "ytdlp.updating": "Updating…",
  "ytdlp.updated": "yt-dlp updated: {output}",
  "ytdlp.updateFailed": "yt-dlp update failed: {message}",

  // Phase 2: 設定画面（画質プリセット）
  "settings.title": "Settings",
  "settings.quality.title": "Quality (ytdl-format)",
  "settings.quality.desc": "Saving applies immediately to playing videos",
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
  "feed.category.placeholder": "Category name",
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
  "blocked.desc": "Hidden from feed, search, and related videos",
  "blocked.empty": "No blocked channels",
  "blocked.added": "Blocked {title}",
  "blocked.addFailed": "Failed to block: {message}",
  "blocked.unblock": "Unblock",
  "blocked.unblocked": "Unblocked {title}",
  "blocked.unblockFailed": "Failed to unblock: {message}",

  // Phase 15: HDR・mpv 引数（仕様決定 Y、INV-1 調査結果の設定化）
  "settings.hdr.title": "HDR & mpv args",
  "settings.hdr.desc":
    "Takes effect from the next playback (applies to HDR→SDR on SDR screens)",
  "settings.hdr.toneMapping": "Tone mapping",
  "settings.hdr.computePeak": "Peak brightness detection",
  "settings.hdr.auto": "Auto (mpv default)",
  "settings.hdr.yes": "Detect",
  "settings.hdr.no": "Do not detect",
  "settings.hdr.extraArgs": "Extra mpv args",
  "settings.hdr.extraArgs.desc":
    "Space-separated (quote values with spaces). Invalid args prevent mpv from starting",
  "settings.hdr.networkTimeout": "Stream read timeout (ms)",
  "settings.hdr.networkTimeout.desc":
    "0 to disable (empty or invalid defaults to 30000ms)",
  "settings.hdr.hwdec": "HW decode mode",
  "settings.hdr.hwdec.desc":
    "Set to no to disable hardware decoding (increases CPU load)",
  "settings.hdr.unsaved":
    "HDR & mpv args changed while saving; previous values were saved (save again)",

  // Phase 18: アプリの自動更新（FR-15、仕様決定 AB）
  "update.title": "App update",
  "update.body": "Download and apply version {version}, then restart.",
  "update.apply": "Update",
  "update.later": "Later",
  "update.installing": "Applying the update…",
  "update.found": "An update to version {version} is available",
  "update.upToDate": "Up to date",
  "update.failed": "Failed to check for updates: {message}",
  "update.installFailed": "Failed to apply the update: {message}",
  "settings.update.title": "App update",
  "settings.update.desc":
    "Checks automatically at startup (auto-update supported on AppImage / NSIS only)",
  "settings.update.check": "Check now",
  "settings.update.checking": "Checking…",

  // Phase 3: SponsorBlock のカテゴリ設定
  "settings.sponsor.title": "SponsorBlock",
  "settings.sponsor.desc": "Takes effect from the next playback",
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
  "chat.popup.hint": "Open in a separate window (closes this panel)",
  "chat.pin": "Always on top",
  "chat.toPanel": "Back to panel",
  "chat.title": "Live chat",
  "chat.empty": "No messages",
  "chat.deleted": "This message was deleted",
  "chat.anonymous": "Anonymous",
  "chat.membership": "Member",
  "chat.status": "Chat: {message}",

  "settings.filters.title": "NG filters",
  "settings.filters.desc":
    "\"Highlight\" emphasizes chat messages without hiding them. Changes apply immediately",
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
    "Registers a substring filter and applies immediately (use \"NG filters\" for regex)",
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
  "library.history.searchPlaceholder": "Title or channel name",
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
  "library.playlist.newPlaceholder": "Playlist name",
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
  "library.playlist.selectHint": "No playlist selected",
  "library.playlists.empty": "No playlists",
  "library.playlist.importUrl": "e.g. https://www.youtube.com/playlist?list=...",
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
  "library.search.placeholder": "Title or channel name",
  "library.search.empty": "No matching items",
  "library.search.source.history": "History",
  "library.search.source.favorites": "Favorites",
  "library.search.source.playlists": "Playlists",
  "library.search.source.watchLater": "Watch later",

  // Phase 7: ホイール割り当て（決定記録: script-opts 注入のため次回再生から有効）
  "settings.wheel.title": "Wheel bindings",
  "settings.wheel.desc":
    "Frame step while paused, volume while playing. Takes effect from the next playback",
  "settings.wheel.volumeDelta": "Volume change (per notch)",

  // Phase 52: 再生の初期速度と初期音量（FR-39、仕様決定 BO）
  "settings.player.title": "Playback",
  "settings.player.desc": "Takes effect from the next playback",
  "settings.player.initialSpeed": "Initial speed",
  "settings.player.initialSpeed.desc": "0.25-4.0 (empty for default 1.0)",
  "settings.player.initialVolume": "Initial volume (%)",
  "settings.player.initialVolume.desc": "0-130 (empty for default 100)",
  "settings.player.unsaved":
    "Playback settings changed while saving (save again)",

  // Phase 8: PiP 小窓の位置（次回の PiP 化から有効）
  "settings.pip.title": "PiP",
  "settings.pip.desc": "Takes effect from the next PiP activation",
  "settings.pip.default": "Play in PiP by default",
  "settings.pip.fitAspect": "Fit the window size to the video aspect ratio",
  "settings.pip.fitAspect.desc":
    "Uses the geometry width and height as a bounding box for the video's aspect ratio",
  "settings.pip.geometry":
    "Window position and size (WxH+±x±y; bounding box when fitting)",
  "settings.pip.quality": "Default PiP quality (ytdl-format expression)",
  "settings.pip.quality.desc": "Uses the global quality setting when empty",
  "settings.pip.unsaved":
    "PiP changed while saving; defaults were saved (save again)",

  // Phase 39: 再生窓の起動ディスプレイ（FR-31、仕様決定 AZ）
  "settings.window.title": "Playback window",
  "settings.window.desc": "Takes effect from the next playback",
  "settings.window.screen": "Startup display",
  "settings.window.screen.auto": "Auto (mpv default)",
  "settings.window.screen.desc": "Falls back to auto when disconnected",
  "settings.window.screen.none": "Could not enumerate connected monitors",
  "settings.window.screen.missing": "{name} (currently disconnected)",
  "settings.window.monitor": "{name} ({dims})",
  "settings.window.monitorPrimary": "{name} ({dims}, primary)",
  // Phase 55: 再生完了時の自動クローズ（FR-43、仕様決定 BS）
  "settings.window.closeOnEof":
    "Close window automatically when playback completes",
  "settings.window.closeOnEof.desc":
    "Does not close during continuous playback (early/error exits always close)",
  "settings.window.unsaved":
    "Playback window changed while saving (save again)",

  // Phase 29: フィード一覧の shorts 既定非表示（FR-23、仕様決定 AP）
  "settings.feed.title": "Feed",
  "settings.feed.desc":
    "List display applies on next fetch; notifications and auto-read apply on next refresh",
  "settings.feed.showShorts": "Show Shorts in the feed",
  "settings.feed.showShorts.desc":
    "When off, excluded from \"All\" and \"Videos\" (shown when type is \"Shorts\")",
  // Phase 65: 新着のデスクトップ通知と自動既読（FR-58・FR-64、仕様決定 CI・CO）
  "settings.feed.notifyNew": "Desktop-notify new items",
  "settings.feed.autoReadPlayed": "Auto-mark played items as read",
  "settings.feed.autoReadPlayed.desc":
    "Applied at startup and on each feed refresh",
  "settings.feed.autoReadDays":
    "Mark items read after this many days since posting",
  "settings.feed.autoReadDays.desc":
    "0 to disable (applied at startup and on each feed refresh)",
  "settings.feed.unsaved": "Feed changed while saving (save again)",

  // Phase 56: ライブ配信の待機（FR-44、仕様決定 BT）
  "livewait.add": "Wait for stream",
  "livewait.waiting": "Waiting",
  "livewait.remove": "Stop waiting",
  "livewait.added": "Waiting for stream: {title}",
  "livewait.duplicate": "This video is already being waited on",
  "livewait.live": "Already live (not registered)",
  "livewait.playNow": "Play now",
  "livewait.ended": "Stream ended (not registered)",
  "livewait.notBroadcast": "Not a scheduled live or premiere (not registered)",
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
  "settings.livewait.desc": "Takes effect on the next stream start detection",
  "settings.livewait.action": "Action on stream start",
  "settings.livewait.action.play": "Auto-play in a new instance",
  "settings.livewait.action.notify": "Toast notification only",
  "settings.livewait.unsaved": "Live wait changed while saving (save again)",

  // Phase 67: チャットポップアップの表示カスタマイズ（FR-68、仕様決定 CS）
  "settings.chatpopup.title": "Chat popup",
  "settings.chatpopup.desc": "Applies immediately to open popups",
  "settings.chatpopup.showAuthor": "Show author name",
  "settings.chatpopup.showAuthor.desc":
    "superchat / membership badges remain hidden",
  "settings.chatpopup.fontSize": "Font size (px)",
  "settings.chatpopup.fontSize.desc": "10-32 (empty for default 14)",
  "settings.chatpopup.opacity": "Opacity",
  "settings.chatpopup.opacity.desc": "0.2-1.0 (empty for default 1.0)",
  "settings.chatpopup.unsaved": "Chat popup changed while saving (save again)",

  // Phase 71: テーマと表示密度（FR-76・FR-77、仕様決定 DA・DB）
  "settings.ui.title": "Appearance",
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
  "settings.ui.unsaved": "Appearance changed while saving (save again)",

  // Phase 72: キーボードショートカットの有効化（FR-79、仕様決定 DD）
  "settings.shortcuts.title": "Shortcuts",
  "settings.shortcuts.enabled": "Enable keyboard shortcuts",
  "settings.shortcuts.enabled.desc":
    "j/Down: next, k/Up: previous, Enter: play, a: queue, q: toggle queue (disabled in inputs/menus)",
  "settings.shortcuts.unsaved": "Shortcuts changed while saving (save again)",

  // Phase 59: 動画内ブックマーク（FR-49、仕様決定 BZ）
  "bookmark.add": "Bookmark",
  "bookmark.add.hint": "Save current position",
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
  "settings.categories.desc": "Changes are saved immediately",
  "settings.categories.empty": "No categories",
  "settings.categories.moveUp": "Up",
  "settings.categories.moveDown": "Down",
  "settings.categories.reorderFailed": "Failed to reorder: {message}",

  // Phase 66: 設定画面の運用節（FR-62・FR-66、仕様決定 CM・CQ）
  "settings.ops.title": "Operations",
  "settings.ops.export": "Export",
  "settings.ops.export.label": "Data export",
  "settings.ops.export.desc":
    "Exports subscriptions, categories, settings, and playlists as JSON",
  "settings.ops.exported": "Exported: {path}",
  "settings.ops.exportFailed": "Export failed: {message}",
  "settings.ops.import": "Import",
  "settings.ops.import.label": "Data import",
  "settings.ops.import.desc":
    "Overwrites current subscriptions, categories, settings, and playlists",
  "settings.ops.importConfirm":
    "This will overwrite current subscriptions, categories, settings, and playlists. This cannot be undone.",
  "settings.ops.imported": "Imported: {path}",
  "settings.ops.importFailed": "Import failed: {message}",
  "settings.ops.dbSize": "Database size",
  "settings.ops.dbSize.desc": "Total of DB and WAL files",
  "settings.ops.dbSize.failed": "Failed to get size: {message}",
  "settings.ops.vacuum": "VACUUM",
  "settings.ops.vacuum.label": "Shrink free space",
  "settings.ops.vacuum.desc": "Rebuild database",
  "settings.ops.vacuumDone": "VACUUM done ({before} → {after})",
  "settings.ops.vacuumFailed": "VACUUM failed: {message}",
  "settings.ops.cleanup": "Clean orphan rows",
  "settings.ops.cleanup.label": "Orphan row cleanup",
  "settings.ops.cleanup.desc":
    "Delete unreferenced videos and items (history is kept)",
  "settings.ops.cleanupConfirm":
    "Unreferenced videos and items will be deleted (history is kept). This cannot be undone.",
  "settings.ops.cleanupDone": "Deleted {count} orphan rows",
  "settings.ops.cleanupFailed": "Cleanup failed: {message}",
  "settings.ops.diag": "Diagnostics",
  "settings.ops.diag.desc":
    "Exports logs, settings, and environment info as zip (signed URLs, tokens, and usernames masked)",
  "settings.ops.diagExported": "Diagnostics exported: {path}",
  "settings.ops.diagFailed": "Failed to export diagnostics: {message}",
  "settings.ops.busy": "Running…",

  // Phase 70: 設定と依存診断の拡充（FR-72・FR-73・FR-74・FR-75、仕様決定 CW・CX・CY・CZ）
  "settings.filter.placeholder": "Filter sections (e.g. quality, mpv, feed)",
  "settings.filter.noMatch": "No matching sections",
  "settings.deps.title": "Dependent commands",
  "settings.deps.ytdlpPath": "yt-dlp path",
  "settings.deps.ytdlpPath.placeholder": "Auto-detect (empty)",
  "settings.deps.ytdlpPath.desc":
    "Empty for auto-detect (bundled → PATH). Takes effect on the next call after saving",
  // Phase 77: PO Token の伝達（FR-86、仕様決定 DK）
  "settings.deps.poToken": "PO Token",
  "settings.deps.poToken.placeholder": "Not set (empty)",
  "settings.deps.poToken.desc":
    "Format: e.g. mweb.gvs+<token> (per yt-dlp PO Token Guide). Takes effect on the next call after saving",
  "settings.deps.unsaved":
    "Dependent commands changed while saving (save again)",
  "settings.diag.title": "Diagnostics",
  "deps.detected": "Detected",
  "deps.missing": "Not found",
  "deps.refresh": "Re-detect",
  "deps.refreshing": "Detecting…",
  "deps.failed": "Failed to detect dependent commands: {message}",
  "setup.title": "Initial setup",
  "setup.desc":
    "Install any missing commands or set their paths in Settings > Dependent commands.",
  "setup.ok": "Get started",

  // Phase 75: 通知履歴（FR-83、仕様決定 DH）
  "settings.notices.title": "Notification history",
  "settings.notices.desc": "Up to {max} newest entries (cleared on restart)",
  "settings.notices.empty": "No notifications",
  "settings.notices.action": "Action: {label}",
  "settings.notices.clear": "Clear history",

} satisfies Record<MessageKey, string>;
