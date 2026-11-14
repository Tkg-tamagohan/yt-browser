import type { MessageKey } from "./i18n-ja";

// Phase 74: 英語リソース（FR-81、仕様決定 DF）。
// ja の全キーを網羅する。satisfies で網羅をコンパイル時に検査し、
// 実行時も未登録キーは t() が ja へフォールバックする（MESSAGES は Partial 型）。
export const en = {
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
  // Phase 77: PO Token の伝達（FR-86、仕様決定 DK）
  "settings.deps.poToken": "PO Token",
  "settings.deps.poToken.placeholder": "Not set (empty)",
  "settings.deps.poToken.desc":
    "For environments where YouTube requires a PO Token. When set, --extractor-args \"youtube:po_token=<value>\" is automatically configured and passed to yt-dlp invocations (including stream resolution via mpv). The value format follows the yt-dlp PO Token Guide (e.g. mweb.gvs+<token>). Empty means unset; changes take effect from the next yt-dlp invocation after saving",
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
