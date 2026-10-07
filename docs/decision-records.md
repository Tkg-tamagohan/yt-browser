# 決定記録

要件定義の協議で確定した判断を ID 付きで記録する。
要件定義書、設計書、実装計画からは「仕様決定 A」「技術方針 K」のように参照する。

## 仕様決定

| ID | 項目 | 決定内容 |
|----|------|----------|
| A | ストリームとメタデータ取得の中核 | yt-dlp 子プロセスを中核とする。`YoutubeBackend` トレイトで抽象化し、将来の InnerTube 自前実装への差し替えを可能にする |
| B | プレイヤー統合方式 | mpv プロセス + JSON IPC で制御し、mpv 自身のウィンドウで再生する。再生制御は抽象化し、libmpv 埋め込みや `--wid` 埋め込みの後追加を可能にする |
| C | フロントエンド | Svelte 5 + TypeScript |
| D | コマ送り | mpv の `frame-step` / `frame-back-step` を利用する。一時停止中ホイールはコマ送り、再生中ホイールは音量調整（yt-frame-scrub の操作感を踏襲）。割り当ては設定で変更可能にする |
| E | チャンネルブロック | 適用範囲はフィード、検索、関連動画からの除外。`blocked_channels` 独立テーブルで管理し、未購読チャンネルもブロック可能にする。チャットの NG ユーザーとは別系統とする |
| F | ライブチャット | InnerTube `get_live_chat` の継続トークンポーラーを Rust で実装する。終了済み配信のチャットリプレイ再生はスコープ外（後フェーズ候補） |
| G | 対象 OS と配布 | Linux と Windows を第一対象とする。macOS は後回し。配布は AppImage と MSI、Tauri updater は後フェーズ |
| H | スコープ境界 | Google アカウント連携は完全除外（評価、投稿、メンバー限定コンテンツは対象外）。動画ダウンロード機能は設けない。YouTube 検索 UI は設ける |
| I | 非機能と運用 | メモリ目標はアイドル 300MB 未満、再生中 600MB 未満の目安値。チャットログは配信単位で保存、視聴履歴は無制限で手動削除のみとする。mpv はシステム依存、yt-dlp は同梱または自動更新機構を持つ |
| J | リポジトリ | 公開リポジトリ（`Tkg-tamagohan/yt-browser`）で開発する。ライセンスは MIT |

## 技術方針

| ID | 項目 | 決定内容 |
|----|------|----------|
| K | DB アクセス | rusqlite + WAL + バージョン管理されたマイグレーション |
| L | エラー設計 | モジュール境界は thiserror の enum、アプリ層は anyhow で文脈を付し、UI には直列化した `{ code, message }` を返す |
| M | NG フィルタ | target × match（literal / regex）のモデル。literal は Aho-Corasick、regex は `RegexSet` でプリコンパイルし、変更時に再構築する |
| N | SponsorBlock | API を自前取得し、mpv IPC の再生位置監視でスキップする。mpv スクリプトには依存しない |
| O | ポーリングと取得 | 指数バックオフ + 条件付き取得（ETag / Last-Modified）。パーサーは golden fixture でテストする |
| P | yt-dlp 運用 | 環境や IP 次第で PO Token を要求される事例がある前提で、導入手順と自動更新機構を設計に含める |

## Phase 1 で確定した事項

実装計画の未決事項として繰り越していた yt-dlp 導入方針を、Phase 1 の実装に合わせて確定した。

| 項目 | 決定内容 |
|------|----------|
| yt-dlp の供給 | 解決順は `settings` の `ytdlp.path`（ユーザー指定）→ 同梱リソースの `yt-dlp` → PATH の `yt-dlp`。同梱は配布フェーズ（Phase 8）で bundle に含め、それまでは PATH のシステムインストールを使う暫定運用 |
| 更新機構 | アプリ内の `ytdlp_update` コマンドが解決済みパスに対して `yt-dlp -U` を実行する。システム管理のパスでは権限不足で失敗し得るため、失敗時は yt-dlp の出力をそのまま UI に返す |
| 起動時チェック | `ytdlp_status` でパスと `--version` 出力を表示する。解決不能時は UI に警告を出す（詳細な設定画面は Phase 7 で整備） |
| JS ランタイム | yt-dlp の YouTube 解読用に deno を PATH で解決する前提とする。同梱の要否は配布フェーズで再検討 |
| PO Token | 要求される環境では yt-dlp 側の手順（`--cookies-from-browser` や外部プロバイダ）を利用する。アプリからの伝達経路は後フェーズの課題とし、README に手順へのポインタを置く |

## Phase 2 で確定した事項

ホイール挙動の設定反映方法（設計書 §4.2 の脚注で確定が保留されていた件）と画質の即時適用経路を定めた。

| 項目 | 決定内容 |
|------|----------|
| wheel.lua の同梱形態 | Lua ソースをバイナリに埋め込み（`include_str!`）、起動時に `app_data/mpv/wheel.lua` へ上書き書き出して `--script` で読ませる。リソースディレクトリ参照よりデバッグ/配布で一貫する |
| ホイール挙動の変更経路 | 音量変化量を `script-opts` の `wheel-volume_delta` で注入する方式を確定。設定キー `wheel.volume_delta`、既定 2。mpv 起動時の注入のため変更は次回再生から有効（稼働中インスタンスには適用されない）。UI 公開は設定画面の本格整備（Phase 7）に委ねる |
| 画質の即時適用 | `settings_set` 保存後、フロント側が `player_control` の `quality` アクションを全稼働インスタンスへ送る（set_property ytdl-format + loadfile replace で再読み込み） |

## Phase 4 で確定した事項

購読フィード（設計書の詳細が薄かった部分）の暫定仕様を定めた。

| 項目 | 決定内容 |
|------|----------|
| RSS エンドポイント | `https://www.youtube.com/feeds/videos.xml?channel_id=<UC...>`（公式 Atom フィード） |
| 購読入力 | `UC...`（24 文字）、`youtube.com/channel/UC...`、`@handle` / `youtube.com/@handle` を受理。@handle は yt-dlp の `--flat-playlist --playlist-end 1 --dump-single-json` で channel_id へ解決する（暫定仕様） |
| 間隔の適応化 | 基本 15 分。新着なしで ×1.5（上限 60 分）。連続失敗は 5 分から倍々バックオフ（上限 60 分）。連続失敗 2 回目で `feed://status` に warn、復帰時に info を通知 |
| 条件付き取得 | `channels.rss_etag` / `rss_last_modified` を If-None-Match / If-Modified-Since に使う。304 時は `last_polled_at` のみ更新 |
| 初回購読時 | RSS を 1 回取得し、得られたエントリをすべて未読（is_read=0）で投入する |
| 購読解除時 | チャンネル行を削除し、そのチャンネルの未読動画を既読化する（videos 行自体は残す） |
| フィード除外 | `list_feed` は `blocked_channels` の動画を常に除外する（FR-5 のフィード側を先行実装） |
| ブロック操作 UI | Phase 5 の検索・関連動画画面に合わせて後送り |
| XML パーサー | `roxmltree`（読み取り専用・依存最小）。media:group のサムネイルはローカル名で拾う |
| フィードからの再生 | フィード項目の再生時はその動画を既読化する |

### Phase 4 レビュー対応で追加確定

| 項目 | 決定内容 |
|------|----------|
| feed 直下の `yt:channelId` | 実測で `UC` プレフィックスなしで返る応答を確認（そのまま `?channel_id=` に使うと 404）。entry 直下の値を優先し、どちらも `UC` 欠落時は `UC` を補う（暫定仕様） |
| ポーリング投入の原子性 | チャンネル存在確認〜エントリ挿入〜取得メタ更新を `feed_ingest` の一トランザクションで行い、応答到着前の購読解除で未読が復活する競合を防ぐ |
| 再購読時の既読 | 初回購読（`channels` 行が無い）での投入は `reset_unread` で既存行も未読へ戻す。既に購読中の再投入では既読を維持する |
| フィード一覧の上限 | `list_feed` は `ORDER BY published_at DESC LIMIT 500` の暫定仕様。ページングは Phase 7（ローカルデータ管理）以降で検討 |
| サムネイル origin | `media:thumbnail` は `https://*.ytimg.com` / `*.ggpht.com` のみ採用（CSP img-src と一致） |
