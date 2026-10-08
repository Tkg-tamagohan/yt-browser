# 決定記録

要件定義の協議で確定した判断を ID 付きで記録する。
要件定義書、設計書、実装計画からは「仕様決定 A」「技術方針 K」のように参照する。

## 仕様決定

| ID | 項目 | 決定内容 |
|----|------|----------|
| A | ストリームとメタデータ取得の中核 | yt-dlp 子プロセスを中核とする。呼び出しは `yt` モジュール内の関数群に隔離する。`YoutubeBackend` トレイトによる抽象化は第 2 のバックエンドやテスト差し替えが必要になった時点で導入する（Phase 5 確定事項で導入を遅延） |
| B | プレイヤー統合方式 | mpv プロセス + JSON IPC で制御し、mpv 自身のウィンドウで再生する。再生制御は抽象化し、libmpv 埋め込みや `--wid` 埋め込みの後追加を可能にする |
| C | フロントエンド | Svelte 5 + TypeScript |
| D | コマ送り | mpv の `frame-step` / `frame-back-step` を利用する。一時停止中ホイールはコマ送り、再生中ホイールは音量調整（yt-frame-scrub の操作感を踏襲）。割り当ては設定で変更可能にする |
| E | チャンネルブロック | 適用範囲はフィード、検索、関連動画からの除外。`blocked_channels` 独立テーブルで管理し、未購読チャンネルもブロック可能にする。チャットの NG ユーザーとは別系統とする |
| F | ライブチャット | InnerTube `get_live_chat` の継続トークンポーラーを Rust で実装する。終了済み配信のチャットリプレイ再生はスコープ外（後フェーズ候補） |
| G | 対象 OS と配布 | Linux と Windows を第一対象とする。macOS は後回し。配布は AppImage と MSI、Tauri updater は後フェーズ |
| H | スコープ境界 | Google アカウント連携は完全除外（評価、投稿、メンバー限定コンテンツは対象外）。動画ダウンロード機能は設けない。YouTube 検索 UI は設ける |
| I | 非機能と運用 | メモリ目標はアイドル 300MB 未満、再生中 600MB 未満の目安値。チャットログは配信単位で保存、視聴履歴は無制限で手動削除のみとする。mpv はシステム依存、yt-dlp は同梱または自動更新機構を持つ |
| J | リポジトリ | 公開リポジトリ（`Tkg-tamagohan/yt-browser`）で開発する。ライセンスは MIT |
| Q | Windows チャット欄クラッシュ（BG-1） | 対象はプレイヤーカードの「チャット欄を表示」ボタンで、押下時にアプリ本体のみが終了する（mpv は存続）。原因調査と修正を行い、修正後は Windows 実機で再現しないことを受け入れ条件とする |
| R | プレイリストの取り込み範囲 | YouTube プレイリスト URL（`list=...`）をローカルプレイリストへ取り込む方式とする（ローカルのみ案・直接再生のみ案は不採用）。取り込みは yt-dlp の flat-playlist 基盤を流用する |
| S | 連続再生の方式 | プレイリストに「連続再生」の開始点を設け、同一インスタンスで次項目へ切り替える。末尾到達で停止し、ループは持たない |
| T | プレイリスト項目の並べ替え | 「上へ」「下へ」ボタンとドラッグ＆ドロップの双方を提供する。投稿日時順ソートは `position` を一括で書き換える操作とし、`published_at` を持たない項目は末尾に置く |
| U | 再生中チャンネルの購読導線 | プレイヤーカードに購読ボタンを設ける。チャンネル ID は videos / watch_history の既知情報を優先し、無ければ yt-dlp で解決する。購読済みはボタン無効化＋「購読済み」表示、ブロック中のチャンネルは購読可でブロックは維持する |
| V | kind（live / short）検出 | RSS は kind を持たないため、shorts は `youtube.com/shorts/<id>` への判定を新規投入時に非同期で行い `videos.kind` を付与する。live / upcoming の検出は初版の対象外とする。フィルタ UI は live / short / video の選択肢を提供する |
| W | PiP 向け解像度設定 | 「解像度」はストリーム画質と解釈し、PiP で起動するインスタンスの既定画質式を `pip.quality.format` として新設する。未設定は `quality.format` に従う。窓サイズは既存の `pip.geometry` が担う |
| X | インスタンス別画質 | 稼働中インスタンスごとの画質変更をプレイヤーカードから行えるようにする。変更値はセッション内に限り有効とし DB には保存しない（次回再生は既定画質に戻る） |
| Y | 自動 HDR 対応 | Windows 11 の自動 HDR に mpv の表示経路を認識させられるかを Windows 環境で調査する。可能なら設定項目として組み込み、不可なら対象外として記録する |

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
| yt-dlp の供給 | 解決順は `settings` の `ytdlp.path`（ユーザー指定）→ 同梱リソースの `yt-dlp` → PATH の `yt-dlp`。Phase 8 の配布物（AppImage/deb/rpm）はアプリ本体のみで yt-dlp は同梱せず、実行環境では PATH のシステムインストールを前提とする。同梱は解決順の対応が済んでいるため今後の選択肢として残す（Phase 8 の決定記録行も参照） |
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

### Phase 4 マージ後の修復で追加確定

| 項目 | 決定内容 |
|------|----------|
| UC 無しで保存された既存行 | マイグレーション v4 で `channels`・`blocked_channels`・`videos`・`watch_history` の channel_id を `UC` 付きに正規化。UC 付き行が既に存在するチャンネルは UC 無し行を削除して統合する |
| 解除後の残存動画の表示 | videos 行は残すが、`list_feed` は `channels` への JOIN（INNER）で購読中チャンネルの項目だけを返す。解除したチャンネルの残存動画は「すべて」表示でも出ない |

## Phase 5 で確定した事項

検索・関連動画・チャンネルブロック（設計書 §5、§6.1、FR-4、FR-5）の暫定仕様を定めた。

| 項目 | 決定内容 |
|------|----------|
| 検索の実体 | `yt-dlp "ytsearch<上限>:<クエリ>" --dump-json --flat-playlist --no-warnings` の行単位 JSONL を読む（設計書 §5 表どおり）。パース不能行はスキップし、全体失敗にはしない。上限は暫定 20 件（`SEARCH_LIMIT`）で、ページングは持たない（設計書の `page` 引数は見送り、必要になれば `ytsearch` 件数を増やす方向で対応） |
| 検索結果のサムネイル | `thumbnails[]` の末尾（最大解像）を採用。無い場合は `https://i.ytimg.com/vi/<id>/hqdefault.jpg` にフォールバック（CSP img-src 内） |
| `next` 応答の形式 | 2026-10 の実応答は `lockupViewModel` 形式で、`compactVideoRenderer` / `videoWithContextRenderer` は含まれない。パーサーは両形式をキー名で再帰探索し、`LOCKUP_CONTENT_TYPE_PLAYLIST`（Mix 等）は除外、同一 video_id は重複除去する（実応答を golden fixture `youtube_next_related.json` として保存） |
| 関連動画のチャンネル ID | `lockupViewModel` のチャンネルリンクは `/@handle` だが、アバターの `browseEndpoint.browseId` に UC ID が入っているためそこから取る（ブロック判定が UC 前提のため必須） |
| `block_channel` の `title` 引数 | 設計書表は `channel_id` のみだが、設定画面のブロック一覧に表示名を出すため `title` を併せて保存する（`blocked_channels.title` は既に NOT NULL）。呼び出し側は channelTitle が無いとき channelId 自体を渡す |
| ブロックの適用面 | フィード一覧は `NOT IN`（設計書 §7 どおり）、検索・関連動画はコマンド層で結果を後段フィルタする。フィード画面・検索画面・関連動画パネルの各項目にブロック導線を置き、解除は設定画面の一覧から行う（FR-5） |
| 検索結果のチャンネル ID | ytsearch の `channel_id` は実測で常に UC 形だが、`uploader_id` は `@handle` や文字列 `"None"` が入ることがある（実測確認）。ブロック・購読の判定は UC 形キー前提なので、`channel_id` → `uploader_id` の順で UC 形（`UC` + 22 文字）の値だけを採用し、該当なしなら `channel_id=None` とする。`@` 始まりの `uploader_id` は `subscribe_channel` が解決できるため `SearchResult.uploader_id` として別途露出し、UC ID の無い結果でも検索画面の購読導線を残す（ブロックキーには使わない）。ハンドル検証は YouTube の実仕様に合わせ Unicode 文字を許容し、`is_handle` は Unicode alphanumeric 3〜30 文字として URL パスはパーセントデコードしてから判定する |
| 関連動画パネルの再取得方針 | パネルを開くたびに `get_related` を再取得し、結果キャッシュはしない。ブロック／解除の変化が即座に反映され、前回失敗時の再試行経路も自然に確保できるため。取得応答の書き戻しは「その時点でパネルが開いている、かつ世代番号が一致する」場合に限定する（閉じた後や開き直し前の応答で状態を上書きしない） |
| `YoutubeBackend` トレイト | 設計書 §9.2 はトレイト抽象化を想定していたが、yt-dlp 依存は `yt` モジュール内の関数群に既に隔離されており、第 2 のバックエンドやテスト差し替えの必要が現時点でないため導入を遅延する（実装は関数群・設計書も実態に更新） |

## Phase 6 で確定した事項

ライブチャット・NG フィルタ・履歴保存検索（設計書 §6.2〜§7、§8、FR-6、FR-9）の暫定仕様を定めた。

| 項目 | 決定内容 |
|------|----------|
| 初期継続トークンの取得 | `watch_html` を独立メソッド化し、`ytInitialData` の抽出は「`ytInitialData` の直後の `{` から文字列リテラルを認識する波括弧対応スキャン」で切り出す（`var ytInitialData = {...};` 形を実測確認）。`liveChatRenderer` をキー名で再帰探索し、無い場合はチャットなしとして info 通知して終了 |
| 継続トークンと待機時間 | `continuations[]` の各エントリは `{<type>ContinuationData: {continuation, timeoutMs?}}` 形で、種別を問わず `continuation` を持つ最初のエントリを採る（実測: invalidation 10000ms / reload）。`timeoutMs` 欠落時の下限は 300ms |
| 削除アクションのキー名 | 2026-10 時点の実応答は `removeChatItemAction`（設計書の `markChatItemAsDeletedAction` 相当は旧名）。両方を受理し `deleted` イベントに正規化、`message` に対象 item ID を入れる。UI は対象行を消さず「削除されました」表示に差し替える |
| `chat://message` のバッチ粒度 | ポーリング応答 1 回分を 1 バッチとして emit し、同じ束を 1 トランザクションで `chat_logs` へ保存する（設計書の「数秒または数百件単位」要件を応答単位の束ねで満たす）。表示は直近 500 件に絞る |
| 重複除去の範囲 | セッション内の既処理 item ID を上限 1 万件で保持して除去する（リプレイ再送・invalidation 差分の食い違い対策）。DB 側に一意制約は持たせない（再起動後の短時間の重複は許容し、raw_json に元 ID は残る） |
| NG 判定の対象 | `chat_text` は本文に、`chat_author` は投稿者名と投稿者チャンネル ID の両方に適用する（名前か ID どちらでも書ける）。`deleted`/`other` 種別も NG 判定するが UI は `other` を常に非表示とする |
| NG フィルタの regex 失敗時 | `filter_add` で `regex::Regex::new` による事前検証を行い不正パターンは登録拒否。既存行が rebuild 時にコンパイル失敗した場合はそのパターンだけを除外し（RegexSet は一括コンパイルのため個別検証してから束ねる）、除外件数を `chat://status` に warn 通知する |
| FTS5 トークナイザ | `trigram` を採用（unicode61 では日本語の部分文字列が語分割されず検索に掛からないため）。制約として 3 文字未満の検索語は部分一致に寄与しない（バンドル SQLite では当該語が無制約扱いになることを実測）。検索は空白区切りの AND、各語を `"..."` フレーズ化して FTS5 構文と衝突しないようにする |
| golden fixture | 実際の `get_live_chat` 応答を匿名化して `youtube_live_chat.json` として回帰テスト化（テキスト 2 件・削除 1 件・invalidation 継続トークンを含む）。継続トークン・`invalidationId` 系のセッション識別子・投稿者 ID/ハンドルはすべてダミー値に置き換える |
| dedup と保存失敗 | 既処理 item ID は `chat_insert_batch` 成功時にのみ確定する。保存失敗したバッチの ID は未確定のまま残し、YouTube 側の再送で履歴へ拾い直せるようにする（UI への再送信は item_id で画面側 dedup） |
| `NgMatcher::refresh` の直列化 | 一覧取得からマッチャ差し替えまでを `refresh_lock` で 1 排他区間にし、並行する登録・削除で古い再構築結果が後勝ちするのを防ぐ |
| 動画系 NG の適用経路 | `video_title` / `channel_title` / `channel_id` のフィルタはフィード一覧・検索・関連動画の各取得結果に後段フィルタで適用する（`Matcher::is_video_ng`）。`video_desc` は現行の取得経路（RSS・ytsearch flat・InnerTube next）に説明文フィールドが無いため評価対象外。フィルタの登録自体は仕様どおり受け付け、説明文を持つ経路ができた時点で有効になる |
| 既表示チャットへのフィルタ遡及 | 表示済みメッセージは受信時点の `ng` 判定のまま保持し、フィルタ変更時に遡って再評価しない。適用は新着イベントに限定する（保存済み原文は履歴検索で読めるため） |
| チャット開始・停止の直列化 | `invoke` の到着順は保証されないため、UI 側で videoId ごとの操作キューを持ち `chat_start` / `chat_stop` を直列化する（「閉じる→すぐ開く」で stop が start の後に処理されて共有ポーラーが死ぬ競合を防ぐ）。ポーラーの停止は同じ videoId の開いたパネルが残っていないときのみ発行し、`player://ended` でも同じ経路を通す |
| NG 適用時の一覧の取得枠 | フィード一覧は SQL の LIMIT を掛けず `feed_list_filtered` が述語適合行を 500 件集めるまで走査する（先頭が NG で抜けても後続の適合行を拾える）。検索は `ytsearch` を表示件数（20）の 3 倍フェッチしてからブロック・NG で絞り、絞りきれない分はページング非対応の制約として許容する |

## Phase 7 で確定した事項

お気に入り・カスタムプレイリスト・履歴一覧・設定画面の完成（設計書 §8、FR-7）の暫定仕様を定めた。

| 項目 | 決定内容 |
|------|----------|
| 動画メタの置き場 | お気に入り・プレイリスト項目は `videos` 台帳を JOIN してタイトル・サムネイル・チャンネル名を表示する。検索・関連・フィード行が持つメタは `favorite_add` / `playlist_add` 呼び出し時に `VideoRef` として UI から渡し、`video_upsert` で台帳を新規登録・差分更新する（フィード未経由の動画でも表示が欠けない）。`videos.channel_id` が空文字の行は購読チャンネルに紐付かない非フィード動画として扱い、一覧 SQL は `NULLIF(channel_id,'')` で NULL に正規化する |
| `video_upsert` の更新方針 | 再登録時は空でない値だけを上書きする: `channel_id`・`title` は空文字なら既存値を保持、`channel_title`・`thumbnail_url` は NULL 以外で上書き。後から不足フィールドが届いても既存データを消さない |
| 重複追加 | `favorites` と `playlist_items` は `INSERT OR IGNORE` で重複を冪等に扱う（再追加時は登録済みの順位・登録日時を保持する）。「再追加すると後に追加した側の順位になる」代替案は既存 `position` を破壊するため不採用 |
| プレイリスト項目の順序 | `position` は末尾追加のみの自動採番（`MAX(position)+1`）。中間項目の削除後は番号の空きを詰めずそのまま残す（順序の正規化やドラッグ並べ替えは今回の受け入れ範囲外として文書に明記） |
| 履歴一覧の上限 | `history_list` は既定 500 件・上限 1000 件。単位時間あたりの視聴本数から十分な余裕として暫定。超過分は現状ページングなしの制約として許容する |
| 削除の確定 | `history_remove`・`favorite_remove`・`playlist_remove`・`playlist_delete` は UI 上の confirm ダイアログを挟まず即時削除する。元に戻す導線（undo）は別途検討課題として残し、Phase 7 の受け入れは「一覧・編集・削除が完結する」まで |
| 履歴削除と再生中の保存 | 再生中の動画を履歴一覧から削除しても、プレイヤーの定期・終了保存（`history_update_progress`）が同じ動画の履歴を再作成してしまうため、`Db` 内にセッション限りの抑止セット（`history_suppressed`）を置く。`history_remove` で登録し `history_update_progress` は抑止中の動画を書き込まず、明示的な再生開始（`history_upsert`）で解除して履歴を再び残せるようにする（回帰テスト DB-LD-05） |
| ライブラリ登録の空行とフィード投入 | お気に入り・プレイリスト登録は `videos` に `published_at` NULL・`is_read=1` のプレースホルダを作る。後着の RSS 投入が `INSERT OR IGNORE` ではこの行を補完しないため、`ingest_rows` を UPSERT に変え、独立フラグ `videos.ingested`（migration v7）が 0 の行（プレースホルダ）だけに `published_at`・`kind`・未読（`is_read=0`）を埋めて `ingested=1` に確定する。フィード投入済み行は既読状態を含めて一切書き換えない。投稿日を欠く RSS エントリも最初の投入で `ingested=1` になるため、再投入で未読化や新着通知が繰り返されない（回帰テスト DB-LD-06） |
| v7 バックフィルの出自判定 | `ingested` は 0=プレースホルダ（フィード非表示・初回到達で補完）/ 1=投入済み（表示）の 2 値。バックフィルは既存行を一律 1 にする。ライブラリ由来行を作れる機能（favorites/playlists）は v6 と同じリリースで初めて出るため、リリース済み DB の v7 適用時点ではプレースホルダが存在し得ず、既存行はすべてフィード由来と確定できる。安全網として「お気に入り・プレイリスト参照のある日付なし行」と「チャンネルなしの日付なし行」（いずれも RSS エントリが常に `<published>` と所属チャンネルを持つためフィード由来はあり得ない）のみ 0 に戻す。この 0 行はこの PR の開発ビルドで作られたプレースホルダに限られる。残存事項: 参照解除済みで残った開発ビルド由来の日付なし行は 1 のままフィードに出るが、リリース版では発生しない。この PR の開発途中で 3 状態版（0/1/2）として一時実装された v7 を通った開発用 DB には ingested=2 の行が残り得るため、v8 で 2→1 に昇格する（回帰テスト migrate_v7_backfills_ingested_by_provenance・migrate_v8_promotes_intermediate_state2・DB-LD-06） |
| 行アクションの共有化 | ☆ お気に入りトグルとプレイリスト追加メニューは `VideoActions` コンポーネントとして共通化し、検索・フィード・関連動画・ライブラリの各行に同じ UI で載せる。お気に入り登録済みの video_id 集合とプレイリスト一覧は `loadLibrary()` で各ページの onMount 時にまとめて読み、行アクション失敗はトースト通知で表す |
| ホイール設定の UI 公開 | Phase 2 の決定記録どおり `wheel.volume_delta` を設定画面の数値入力で公開する。`--script-opts` 注入のため「次回再生から有効」の表記を UI に添え、即時適用の仕組みは持たない。値は保存時に数値として検証し（有限・±100 以内）、不正値は `settings_set` に送らず失敗通知する |
| PiP の切り替え経路 | 設計書 §4.5 は起動時フラグ（`--ontop --no-border --geometry`）の形で書くが、`ontop`/`border`/`geometry` はいずれも実行時に `set_property` で変更可能（mpv 0.34.1 で確認済み）のため、`PlayerAction::Pip{enabled}` で稼働中インスタンスを後から PiP 化・解除できるようにする。`PlayerState.pip` でトグル状態を UI に伝える。起動時 PiP は `play_video` の `pip` 引数（`SpawnOptions.pip_geometry`）で同じ 3 フラグを渡す。`geometry` は mpv にそのまま渡す値なので `WxH`＋任意の符号付き座標の形式のみ受理（`is_valid_pip_geometry`）、未設定・不正値は既定 `480x270-40-40`（右下寄せ）。設定キー `pip.geometry` を設定画面に公開し「次回の PiP 化から有効」と表記。枠なしウィンドウは mpv の既定バインド（`q` 終了・ドラッグ移動の `--window-dragging`）で操作する前提で、アプリ側の閉じる経路はプレイヤーカードの「終了」ボタン。実機検証で判明した mpv 側の挙動: 最大化中は `geometry` が効かないため `set_pip` は有効化時に `window-maximized=false` を先に送る。実行時の `set_property geometry` では位置オフセット（`±x±y`）が mpv x11 側で無視されサイズのみ適用される。spawn 時の `--geometry` では位置も正しく効くため、位置精度が必要なら起動時 PiP 経路を使う |
| 配布物と updater の検討 | 配布ターゲットは仕様決定 G のまま AppImage（Linux）と MSI（Windows）。AppImage は `pnpm tauri build` で生成できることを確認し成果物を同梱報告する。MSI は WiX/Windows 環境が必要でこの VM（Linux）では作れないため、計画書の引き継ぎ手順どおり Windows 環境への移管対象とする。なお AppImage/deb/rpm は mpv ・ yt-dlp を同梱しない（アプリ本体のみ）。実行環境では両者を PATH から参照できることが前提であり、README の実行依存に従う。yt-dlp のバンドル同梱は `YtDlpResolver` の解決順（設定 → 同梱 → PATH）が対応済みなので今後の選択肢として残す。Tauri updater について: `tauri-plugin-updater` は署名鍵（公開鍵を設定・秘密鍵は CI シークレット）と更新 JSON（`latest.json` を静的配信。GitHub Releases なら `releases/latest/download/latest.json` 固定 URL で配える方式）が必須で、リリースワークフローの整備とセットになる。現時点では初回配布の AppImage/MSI 生成が先で、updater 組み込みは「GitHub Releases での公開運用が始まった段階で updater プラグイン＋署名鍵＋latest.json を追加する」と方針を確定する（仕様決定 G の「後フェーズ」の結論をこの内容に具体化） |

## 全フェーズ完了後の検証と修復で確定した事項

フェーズ実装の完了後に行った実機検証とその修復で確定した事項を記録する。

| 項目 | 決定内容 |
|------|----------|
| Windows 版 IPC とビルド（PR #12） | `mpv/ipc.rs` の IPC トランスポートを OS で分岐し、Windows は tokio 内蔵の `windows::named_pipe` を使う（追加クレートなし）。ソケット名は `\\.\pipe\yt-browser-mpv-<instance>-<pid>`。mpv 0.38 以降の `loadfile` は options の第 4 引数形式が必須で古い 3 引数では失敗するため、新形式優先＋旧形式フォールバックで両対応した。Windows 子プロセスには `CREATE_NO_WINDOW` を付けてコンソール窓のちらつきを抑える。Windows Server 2022 で MSI（`yt-browser_0.1.0_x64_en-US.msi`）と NSIS インストーラを生成し、サイレントインストール、起動、名前付きパイプ疎通、mpv 0.37 と 0.41 での実再生まで確認した |
| Windows CI（PR #13） | CI に `windows-latest` ジョブを常設し、`cfg(windows)` 側のコード（名前付きパイプ IPC 等）に clippy と `cargo test` を掛ける。`cfg(test)` の IPC テストは Unix 版と同名の 3 ケースを NamedPipeServer で再現する構成に揃えた |
| 同期コマンドからのタスク起動（PR #14） | `chat_start` のような同期コマンドハンドラは Tokio ランタイムのコンテキスト外で実行されるため、内部で `tokio::spawn` を呼ぶと `there is no reactor running` で panic し GLib コールバック経由でプロセスが abort する。Tauri コマンドからのタスク起動は `tauri::async_runtime::spawn` に統一する。戻り値の `tauri::async_runtime::JoinHandle` は `abort_handle()` を持たないため、停止用には `JoinHandle` 自体を保持して `abort()` を呼ぶ |
| プレイヤーカードの常時マウント（PR #15） | GPU なし環境の webkit2gtk で、DOM 要素のリマウントをきっかけに描画位置と入力ヒット領域がずれる事象を確認した（ヒット領域は描画からずれた座標に残り、空白に見える場所のクリックで発火する）。display リフロー、ノード再作成、`translateZ`/`will-change`、zoom、visibility、resize 送出、`contain`、pointer-events のトグルのいずれでもヒットマップが再同期せず、回復は webview 再作成か `location.reload()` に限られた。一方でリマウントされない要素はずれの影響を受けないことを確認したため、プレイヤーカードを `PlayerCards.svelte` に抽出して `+layout.svelte` で常時マウントし、非トップページでは CSS の `hidden` で隠す構成にした。ページの再マウントに乗っていた resumeHint 更新は `players.svelte.ts` の `playbackHooks` 経由に、お気に入りとプレイリストの一覧の再取得は `$effect` でパス判定して行う。再取得の照合は「発行順の最新のみ採用」＋「ローカル編集のコミット後に必ず再取得」で DB の内容に収束させる |
| 一時停止中インスタンスの復元と chat_logs の冪等化 | 残課題の 2 件を恒久策で解消した。1) 一時停止中は `player://state` が流れずページ再読み込みでカードが消える問題に対し、稼働中インスタンスのスナップショットを返す `player_list` コマンドを新設し、イベント購読の初期化直後に一覧を取得してカードを復元する（これで `location.reload()` を安心して使えるようになり、ヒットずれの回復手段としても機能する）。2) チャットパネル再オープン時のバックログ再保存（`chat_logs` 重複）に対し、migration v9 で `chat_logs` に `item_id` 列を追加して `(video_id, item_id)` に一意制約を置き、`chat_insert_batch` を `INSERT OR IGNORE` に変えて冪等化した。`item_id` を持たないイベントは NULL として入り制約の対象外になる。`raw_json` の格納形はイベント種別で異なり、通常イベントは `{"<renderer名>": {…}}` のラッパ形（ID は内側オブジェクトの `id`）、削除イベントは削除アクションの内側オブジェクトの直書き（トップレベルに `targetItemId`）になる。バックフィルはこの両形に対応させ、通常イベントは `json_each` で内側を辿り（スカラー値への `json_extract` は malformed JSON になるため `json_type='object'` に限定）、削除イベントはトップレベルの `targetItemId` から実行時と同じ `del:` 合成 ID を作る。復元で重複が発生し得るため索引作成前に最古の 1 行だけ残して掃除する。取れない行は NULL のまま残り制約対象外になる。セッション内 dedup（item_id 集合）はポーラー側にも残し、DB 制約はセッションを跨いだ重複を防ぐ第 2 層とする |
| bot wall 回避の検証経路 | YouTube が yt-dlp の直結を bot 判定して再生を拒否する期間があり（`Sign in to confirm you're not a bot`）、発生中は watch ページ取得も止まって InnerTube の初回継続トークンも取れない。検証目的では、ログイン済みブラウザからエクスポートした Netscape 形式 cookies.txt を `~/.config/yt-dlp/config` の `--cookies` 指定で全 yt-dlp 呼び出し（mpv の ytdl_hook 経由を含む）に渡す経路が有効だった。これは開発と検証の環境での回避策であり、アプリの機能や配布物に cookies の仕組みは含めない（仕様決定 H のアカウント連携除外を維持する）。bot wall はその後自然に解除され、cookie なしの匿名アクセスでも watch ページと `get_live_chat` が応答することを確認した |

## 実機フィードバック（2026-10）の協議で併せて確定した事項

要件定義 FR-10 〜 FR-14、BG-1、INV-1 の実装レベルの細目を定めた。

| 項目 | 決定内容 |
|------|----------|
| フィード画面の 2 カラム構成 | 左カラムに購読管理（購読入力・カテゴリ作成・登録チャンネルリスト）、右カラムにフィード一覧を配置する。狭い画面幅では縦積みにフォールバックする。UI 変更のため実装時に browser preview のモックをユーザーへ先行提示する |
| 購読チャンネル一覧のカテゴリフィルタ | フィード一覧と同じ選択肢（すべて・未分類・各カテゴリ）で登録リストを絞り込む。選択状態は画面ローカルとする |
| フィードの種別フィルタ | `list_feed` のフィルタ引数に種別を追加し、`feed_list_filtered` の後段フィルタ方式を踏襲する。kind が未検出（`'video'` のまま）の項目は video として扱う |
| shorts の判定方式 | 新規投入アイテムに限り非同期で `https://www.youtube.com/shorts/<video_id>` へ HEAD を送り、shorts として応答する場合に `kind='short'` に更新する。判定失敗やタイムアウトは `'video'` のままとし、初版ではリトライを持たない（暫定） |
| YouTube プレイリスト取り込み | `yt-dlp --flat-playlist` でプレイリスト URL の項目を取得し、新規ローカルプレイリストとして保存する。各項目のメタは `video_upsert` 経路で `videos` 台帳へ流し、フィード未経由の項目はプレースホルダ（`ingested=0`）になる |
| 連続再生のキュー | 連続再生の文脈はフロント側が保持し、`player://ended`（自然終了）を契機に同一インスタンスで次項目を再生する。途中項目のエラー終了時は次項目へ進み、通知で表す（暫定） |
| 再生中チャンネルの解決順 | `videos.channel_id` → `watch_history.channel_id` → yt-dlp メタ取得（`channel_id` / `uploader_id` / `channel_url`）の順で解決し、`subscribe_channel` の既存解決経路（UC / @handle / URL 受理）へ渡す |
| PiP 画質の解決順 | インスタンス起動時の画質は「インスタンス別の指定 > PiP なら `pip.quality.format` > `quality.format`」の順とする |
| 自動 HDR 調査の範囲 | mpv の vo / gpu-api 設定と Windows の自動 HDR 認識の可否を Windows 子セッションで確認する。実現に mpv 起動引数の追加が要る場合の設定化の形（個別キーか汎用の追加分引数か）は実装段階で判断する |
