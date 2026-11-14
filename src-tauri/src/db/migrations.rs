//! バージョン付きスキーママイグレーション（技術方針 K）。
//! `version` は連番で増やすだけにし、適用済みのものは変更しない。

pub struct Migration {
    pub version: u32,
    pub name: &'static str,
    pub sql: &'static str,
}

/// 設計書 §8 の DDL をフェーズごとに分割投入する。
/// Phase 0 は `settings`、Phase 1 は `watch_history`（実装計画の受け入れ条件）。
pub const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "settings",
        sql: "CREATE TABLE settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
              );",
    },
    Migration {
        version: 2,
        name: "watch_history",
        sql: "CREATE TABLE watch_history (
                video_id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                channel_id TEXT,
                channel_title TEXT,
                position_sec INTEGER NOT NULL DEFAULT 0,
                duration_sec INTEGER,
                last_watched_at TEXT NOT NULL DEFAULT (datetime('now')),
                completed INTEGER NOT NULL DEFAULT 0
              );
              CREATE INDEX idx_history_recent ON watch_history(last_watched_at DESC);",
    },
    // Phase 4: 購読フィードの基盤（設計書 §8 の DDL から該当分）
    Migration {
        version: 3,
        name: "feed",
        sql: "CREATE TABLE categories (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                sort_order INTEGER NOT NULL DEFAULT 0
              );
              CREATE TABLE channels (
                channel_id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                thumbnail_url TEXT,
                category_id INTEGER REFERENCES categories(id) ON DELETE SET NULL,
                subscribed_at TEXT NOT NULL DEFAULT (datetime('now')),
                last_polled_at TEXT,
                rss_etag TEXT,
                rss_last_modified TEXT
              );
              CREATE TABLE blocked_channels (
                channel_id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
              );
              CREATE TABLE videos (
                video_id TEXT PRIMARY KEY,
                channel_id TEXT NOT NULL,
                channel_title TEXT,
                title TEXT NOT NULL,
                thumbnail_url TEXT,
                published_at TEXT,
                duration_sec INTEGER,
                kind TEXT NOT NULL DEFAULT 'video'
                  CHECK (kind IN ('video','short','live','upcoming')),
                is_read INTEGER NOT NULL DEFAULT 1,
                first_seen_at TEXT NOT NULL DEFAULT (datetime('now'))
              );
              CREATE INDEX idx_videos_channel_pub ON videos(channel_id, published_at DESC);
              CREATE INDEX idx_videos_unread ON videos(is_read, published_at DESC);",
    },
    // Phase 4 修正: 初期実装が feed 直下の UC プレフィックスなし channelId を
    // 保存していた環境の修復。旧形式は `UC` を欠いた 22 文字の識別子
    // （先頭が `UC`/`uc` の場合もあるため NOT LIKE ではなく長さで判定）。
    // 正規 `UC` 付き行が既にある衝突側は、削除前にカテゴリ・購読日時・
    // ポーリングメタの欠落分を UC 付き行へ引き継いでから重複を消す。
    Migration {
        version: 4,
        name: "channel_id_uc_normalize",
        sql: "UPDATE channels
              SET category_id = COALESCE(
                    category_id,
                    (SELECT c2.category_id FROM channels c2
                     WHERE c2.channel_id = substr(channels.channel_id, 3))),
                  subscribed_at = MIN(
                    subscribed_at,
                    (SELECT c2.subscribed_at FROM channels c2
                     WHERE c2.channel_id = substr(channels.channel_id, 3))),
                  last_polled_at = COALESCE(
                    last_polled_at,
                    (SELECT c2.last_polled_at FROM channels c2
                     WHERE c2.channel_id = substr(channels.channel_id, 3))),
                  rss_etag = COALESCE(
                    rss_etag,
                    (SELECT c2.rss_etag FROM channels c2
                     WHERE c2.channel_id = substr(channels.channel_id, 3))),
                  rss_last_modified = COALESCE(
                    rss_last_modified,
                    (SELECT c2.rss_last_modified FROM channels c2
                     WHERE c2.channel_id = substr(channels.channel_id, 3)))
              WHERE channel_id LIKE 'UC%' AND length(channel_id) = 24
                AND EXISTS (
                  SELECT 1 FROM channels c2
                  WHERE c2.channel_id = substr(channels.channel_id, 3)
                    AND length(c2.channel_id) = 22
                );
              DELETE FROM channels
              WHERE length(channel_id) = 22
                AND EXISTS (
                  SELECT 1 FROM channels c2
                  WHERE c2.channel_id = 'UC' || channels.channel_id
                );
              UPDATE channels SET channel_id = 'UC' || channel_id
              WHERE length(channel_id) = 22;
              UPDATE OR IGNORE blocked_channels SET channel_id = 'UC' || channel_id
              WHERE length(channel_id) = 22;
              DELETE FROM blocked_channels WHERE length(channel_id) = 22;
              UPDATE videos SET channel_id = 'UC' || channel_id
              WHERE length(channel_id) = 22;
              UPDATE watch_history SET channel_id = 'UC' || channel_id
              WHERE length(channel_id) = 22;",
    },
    // Phase 6: ライブチャットの保存基盤（設計書 §8 の DDL から該当分）。
    // chat_logs は削除アクションや未正規化イベントも原文で残し、
    // 表示側の絞り込みは NG フィルタ（filters）が担う。
    Migration {
        version: 5,
        name: "chat",
        sql: "CREATE TABLE chat_logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                video_id TEXT NOT NULL,
                posted_at_usec INTEGER NOT NULL,
                author_channel_id TEXT,
                author_name TEXT,
                kind TEXT NOT NULL DEFAULT 'text'
                  CHECK (kind IN ('text','superchat','membership','deleted','other')),
                message TEXT NOT NULL,
                amount_display TEXT,
                raw_json TEXT NOT NULL
              );
              CREATE INDEX idx_chat_video_ts ON chat_logs(video_id, posted_at_usec);
              -- 日本語の部分文字列検索に対応させるため trigram トークナイザを使う
              -- （unicode61 では「こんにちは世界」全体が 1 語になり、
              --   「こんにちは」で検索できない）。3 文字未満の検索語は
              --   部分一致に掛からない（trigram の制約）。
              CREATE VIRTUAL TABLE chat_logs_fts USING fts5(
                message, author_name, content='chat_logs', content_rowid='id',
                tokenize='trigram'
              );
              CREATE TRIGGER chat_logs_ai AFTER INSERT ON chat_logs BEGIN
                INSERT INTO chat_logs_fts(rowid, message, author_name)
                VALUES (new.id, new.message, new.author_name);
              END;
              CREATE TRIGGER chat_logs_ad AFTER DELETE ON chat_logs BEGIN
                INSERT INTO chat_logs_fts(chat_logs_fts, rowid, message, author_name)
                VALUES ('delete', old.id, old.message, old.author_name);
              END;
              CREATE TABLE filters (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                target TEXT NOT NULL CHECK (target IN
                  ('video_title','video_desc','channel_title','channel_id',
                   'chat_text','chat_author')),
                kind TEXT NOT NULL CHECK (kind IN ('literal','regex')),
                pattern TEXT NOT NULL,
                enabled INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
              );",
    },
    // Phase 7: お気に入りとカスタムプレイリスト（設計書 §8 の DDL から該当分、FR-7）。
    // 動画のメタ情報は videos を台帳として JOIN で取り、
    // favorites / playlist_items は video_id と並びだけを持つ。
    Migration {
        version: 6,
        name: "localdata",
        sql: "CREATE TABLE favorites (
                video_id TEXT PRIMARY KEY,
                added_at TEXT NOT NULL DEFAULT (datetime('now'))
              );
              CREATE TABLE playlists (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                sort_order INTEGER NOT NULL DEFAULT 0
              );
              CREATE TABLE playlist_items (
                playlist_id INTEGER NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,
                video_id TEXT NOT NULL,
                position INTEGER NOT NULL,
                PRIMARY KEY (playlist_id, video_id)
              );",
    },
    // Phase 7 レビュー対応: ライブラリ登録で先にできた videos 行（プレースホルダ）と
    // フィード投入済みの行を published_at の有無では判別できない（投稿日なしの
    // RSS エントリが毎回新着扱いになる）。独立したフラグ列を追加する。
    // 0=ライブラリ由来のプレースホルダ（フィード非表示、初回 RSS 到達で補完）、
    // 1=フィード投入済み（表示）。
    // バックフィルは全行 1 でよい: ライブラリ由来行を作れる機能
    // （favorites/playlists）はこの v6 マイグレーションと同じリリースで
    // 初めて出るため、リリース済みの DB には v7 適用時点でプレースホルダが
    // 存在し得ず、既存行はすべてフィード由来と確定できる。
    // 安全網として、「お気に入り・プレイリスト参照を持つ日付なし行」と
    // 「チャンネル ID を持たない日付なし行」（いずれもフィード由来は
    // あり得ない: RSS エントリは常に <published> と所属チャンネルを持つ）
    // だけを 0 に戻す。この 0 行はこの PR の開発ビルドで作られた
    // プレースホルダに限られる。
    // 参照解除済みで残った開発ビルド由来の日付なし行だけは 1 のまま
    // （判別不能の残存だがリリース版では発生しない）ことを決定記録に明記。
    Migration {
        version: 7,
        name: "videos_ingested_flag",
        sql: "ALTER TABLE videos
                ADD COLUMN ingested INTEGER NOT NULL DEFAULT 0;
              UPDATE videos SET ingested = 1;
              UPDATE videos SET ingested = 0
              WHERE published_at IS NULL
                AND (channel_id = ''
                     OR video_id IN (SELECT video_id FROM favorites
                                     UNION SELECT video_id FROM playlist_items));",
    },
    // この PR の開発途中で v7 が一度「0/1/2 の 3 状態」版で書き込まれており、
    // その版を適用した開発用 DB には ingested=2 の行が残り得る。新版の
    // feed_list は ingested=1 のみを表示するため、2 は投入済みに昇格する。
    // （3 状態版は未リリースのため、リリース済み DB ではこの UPDATE は無害な
    // 空操作になる）
    Migration {
        version: 8,
        name: "videos_ingested_promote_state2",
        sql: "UPDATE videos SET ingested = 1 WHERE ingested = 2;",
    },
    // 残課題対応: チャットパネルの再オープンでバックログが再取得され
    // chat_logs に重複行が作られていた問題への対応。イベントの
    // item_id を列として持たせ、(video_id, item_id) に一意制約を置き
    // INSERT OR IGNORE で冪等化する。item_id を持たないイベント
    // （空文字列）は NULL として入れ、UNIQUE 制約の対象外にする
    // （SQLite は NULL を個別の値として扱い衝突しない）。
    // 既存行の item_id は raw_json（原文）から復元する。形は 2 通りで、
    // 通常イベントは {"<renderer名>": {…}} のラッパ形でイベント ID は
    // 内側オブジェクトの "id"、削除イベントは削除アクションの内側
    // オブジェクトがそのまま入り、トップレベルの "targetItemId" から
    // 実行時と同じ合成 ID 'del:<targetItemId>' を作る。
    // json_each の値がスカラーだと json_extract が malformed JSON で
    // 失敗するため、内側を辿るのは object 値だけに限定する。
    // バックフィルで重複が発生し得る（一意制約導入前に重複保存された行）
    // ので、索引作成の前に最古の 1 行だけ残して掃除する。
    Migration {
        version: 9,
        name: "chat_logs_item_id",
        sql: "ALTER TABLE chat_logs ADD COLUMN item_id TEXT;
              UPDATE chat_logs SET item_id = COALESCE(
                'del:' || json_extract(raw_json, '$.targetItemId'),
                (SELECT json_extract(je.value, '$.id')
                   FROM json_each(chat_logs.raw_json) je
                   WHERE json_type(je.value) = 'object'
                     AND json_extract(je.value, '$.id') IS NOT NULL
                   LIMIT 1)
              ) WHERE json_type(raw_json) = 'object';
              DELETE FROM chat_logs
                WHERE item_id IS NOT NULL
                  AND id NOT IN (
                    SELECT MIN(id) FROM chat_logs
                      WHERE item_id IS NOT NULL
                      GROUP BY video_id, item_id);
              CREATE UNIQUE INDEX idx_chat_item
                ON chat_logs(video_id, item_id);",
    },
    // yt-dlp 解決失敗時に media-title が URL 片（`watch?v=...`）として
    // 保存されていた行を修復する。行自体は履歴として残し、タイトルだけを
    // 空に戻す（空タイトルは次回再生時に正しい値で更新される）。
    Migration {
        version: 10,
        name: "history_title_cleanup",
        sql: "UPDATE watch_history SET title = ''
              WHERE title LIKE 'watch?%'
                 OR title LIKE 'http://%'
                 OR title LIKE 'https://%';",
    },
    // チャンネル別過去動画バックフィル（FR-21、仕様決定 AN）の進捗。
    // `/videos`・`/streams` タブごとに「次に取得するまでに遡った件数」を保持する
    // （= yt-dlp --playlist-start の開始位置。次回は pos+1 から取る）。
    Migration {
        version: 11,
        name: "channels_backfill_pos",
        sql: "ALTER TABLE channels ADD COLUMN backfill_videos_pos INTEGER NOT NULL DEFAULT 0;
              ALTER TABLE channels ADD COLUMN backfill_streams_pos INTEGER NOT NULL DEFAULT 0;",
    },
    // Phase 36: chat_logs 廃止（仕様決定 AV）。
    // チャット履歴の用途が設定画面の全文検索のみとなり保持コストに見合わないため、
    // FTS トリガー・仮想テーブル・本体テーブルをまとめて DROP する。
    Migration {
        version: 12,
        name: "drop_chat_logs",
        sql: "DROP TRIGGER IF EXISTS chat_logs_ai;
              DROP TRIGGER IF EXISTS chat_logs_ad;
              DROP TABLE IF EXISTS chat_logs_fts;
              DROP TABLE IF EXISTS chat_logs;",
    },
    // Phase 38: 一時キューの永続化（FR-30、仕様決定 AY）。
    // queue_items.position の連番を主キーとして同一動画の重複を許容し、
    // 表示メタは項目行へ持たせて videos 台帳を汚染しない自己完結型とする。
    // queue_state は単一行（id=1 固定）で現在位置だけを持つ。
    Migration {
        version: 13,
        name: "queue_persistence",
        sql: "CREATE TABLE queue_items (
                position INTEGER PRIMARY KEY,
                video_id TEXT NOT NULL,
                title TEXT NOT NULL,
                channel_title TEXT,
                thumbnail_url TEXT
              );
              CREATE TABLE queue_state (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                current_index INTEGER NOT NULL DEFAULT 0
              );",
    },
    // Phase 49: キュー項目のユニーク ID（BG-10、仕様決定 BK）。
    // video_id 基準の照合では同一動画の連続登録を区別できないため、
    // 項目ごとの連番 uid を持たせる。既存行は一律 0（旧形式）で、
    // 復元側が 0 を新規払い出しへ振り替えるためバックフィルは要らない。
    Migration {
        version: 14,
        name: "queue_items_uid",
        sql: "ALTER TABLE queue_items ADD COLUMN uid INTEGER NOT NULL DEFAULT 0;",
    },
    // Phase 56: ライブ配信の待機（FR-44、仕様決定 BT）。
    // video_id を主キーにして同一動画の重複待機を抑止する。
    // 表示メタは項目行へ持たせて videos 台帳を汚染しない（queue_items と同じ方針）。
    // 予定時刻は epoch ms の INTEGER で持ち、不明は NULL。registered_ms は
    // 予定時刻を持たない項目の自動解除（24 時間超過）の起点になる。
    Migration {
        version: 15,
        name: "live_waits",
        sql: "CREATE TABLE live_waits (
                video_id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                channel_id TEXT,
                channel_title TEXT,
                thumbnail_url TEXT,
                scheduled_start_ms INTEGER,
                registered_ms INTEGER NOT NULL
              );",
    },
    // Phase 59: 動画内ブックマーク（FR-49、仕様決定 BZ）。
    // 同一動画の同一位置の重複保存は UNIQUE (video_id, position_sec) で抑止する
    // （暫定解釈: 位置表記が既定ラベルのため同位置の重複は実質同一行になる）。
    // 表示用タイトルはテーブルに持たず、一覧取得時に videos → watch_history
    // の順で解決する（video_meta_lookup と同じ規則）。
    Migration {
        version: 16,
        name: "video_bookmarks",
        sql: "CREATE TABLE video_bookmarks (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                video_id TEXT NOT NULL,
                position_sec INTEGER NOT NULL,
                label TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                UNIQUE (video_id, position_sec)
              );",
    },
    // Phase 58: プレイリストキューの永続化（FR-48、仕様決定 BY）。
    // queue_state を単一行（CHECK (id = 1)）からキューごとの行へ広げる。
    // playlist_id が NULL の行（id=1）は従来どおり一時キューの現在位置、
    // NULL 以外の行は永続化するプレイリストキュー（playlist_id = 対象
    // プレイリストへの FK、現在位置のみ。項目列は playlist_items を正と
    // し複製しない）。プレイリスト削除で行も消えるよう CASCADE とする。
    // CHECK 制約は ALTER で外せないためテーブルの作り直しで対応する
    // （旧行は id・current_index をそのまま引き継ぐ）。
    // 部分一意索引で playlist_id ごとに高々 1 行を保証する。
    Migration {
        version: 17,
        name: "queue_state_playlist",
        sql: "CREATE TABLE queue_state_migr AS SELECT * FROM queue_state;
              DROP TABLE queue_state;
              CREATE TABLE queue_state (
                id INTEGER PRIMARY KEY,
                playlist_id INTEGER REFERENCES playlists(id) ON DELETE CASCADE,
                current_index INTEGER NOT NULL DEFAULT 0
              );
              INSERT INTO queue_state (id, current_index)
                SELECT id, current_index FROM queue_state_migr;
              DROP TABLE queue_state_migr;
              CREATE UNIQUE INDEX idx_queue_state_playlist
                ON queue_state(playlist_id) WHERE playlist_id IS NOT NULL;",
    },
    // Phase 62: メタの遅延解決基盤（FR-53・FR-56・FR-57、仕様決定 CD・CG・CH）。
    // kind の値域に 'premiere' を追加し、kind の判定状態（kind_state）と
    // 投稿日時解決の試行印（meta_checked）を新設する。
    // CHECK 制約は ALTER で外せないため queue_state と同じくテーブルの
    // 作り直しで対応する。既存行の kind_state は 'short' のみ確定済み（1）
    // とし、'video'/'live'/'upcoming' は未確定（0）へ戻す:
    // 'video' は放送系・shorts の双方が未検査、'live'/'upcoming' は
    // バックフィルのタブ名による推定値のため InnerTube で実値を裏付ける
    // （暫定解釈。放送系の誤分類は遅延解決で修正される）。
    // meta_checked は一律 0（InnerTube メタ応答を未試行）へ初期化する。
    Migration {
        version: 18,
        name: "videos_meta_resolve",
        sql: "CREATE TABLE videos_migr AS SELECT * FROM videos;
              DROP TABLE videos;
              CREATE TABLE videos (
                video_id TEXT PRIMARY KEY,
                channel_id TEXT NOT NULL,
                channel_title TEXT,
                title TEXT NOT NULL,
                thumbnail_url TEXT,
                published_at TEXT,
                duration_sec INTEGER,
                kind TEXT NOT NULL DEFAULT 'video'
                  CHECK (kind IN ('video','short','live','upcoming','premiere')),
                is_read INTEGER NOT NULL DEFAULT 1,
                first_seen_at TEXT NOT NULL DEFAULT (datetime('now')),
                ingested INTEGER NOT NULL DEFAULT 0,
                kind_state INTEGER NOT NULL DEFAULT 0,
                meta_checked INTEGER NOT NULL DEFAULT 0
              );
              INSERT INTO videos
                (video_id, channel_id, channel_title, title, thumbnail_url,
                 published_at, duration_sec, kind, is_read, first_seen_at,
                 ingested, kind_state, meta_checked)
              SELECT video_id, channel_id, channel_title, title, thumbnail_url,
                published_at, duration_sec, kind, is_read, first_seen_at,
                ingested,
                CASE WHEN kind = 'short' THEN 1 ELSE 0 END,
                0
              FROM videos_migr;
              DROP TABLE videos_migr;
              CREATE INDEX idx_videos_channel_pub ON videos(channel_id, published_at DESC);
              CREATE INDEX idx_videos_unread ON videos(is_read, published_at DESC);",
    },
    // Phase 63: バックフィルの /shorts タブ対応（FR-55、仕様決定 CF）。
    // `/videos`・`/streams` と同じく `/shorts` タブの取得済み件数を保持する。
    // （初版は v18 だったが、Phase 62 の v18 `videos_meta_resolve` との
    // 番号衝突を避けて v19 に繰り上げた）
    Migration {
        version: 19,
        name: "channels_backfill_shorts_pos",
        sql: "ALTER TABLE channels ADD COLUMN backfill_shorts_pos INTEGER NOT NULL DEFAULT 0;",
    },
    // Phase 64: 「あとで見る」（FR-59、仕様決定 CJ）。
    // favorites と同じく video_id 主キーと登録日時だけを持ち、
    // 表示メタは videos 台帳へ登録して一覧取得時に JOIN で取る。
    // （初版は v19 だったが、Phase 63 の v19 `channels_backfill_shorts_pos`
    // との番号衝突を避けて v20 に繰り上げた）
    Migration {
        version: 20,
        name: "watch_later",
        sql: "CREATE TABLE watch_later (
                video_id TEXT PRIMARY KEY,
                added_at TEXT NOT NULL DEFAULT (datetime('now'))
              );",
    },
    // Phase 68: チャットのハイライト（FR-69、仕様決定 CT）。
    // filters.kind の値域に 'highlight' を追加する。CHECK 制約は ALTER で
    // 変更できないため queue_state / videos と同じくテーブルの作り直しで
    // 対応し、既存行はすべてそのまま引き継ぐ。
    Migration {
        version: 21,
        name: "filters_highlight_kind",
        sql: "CREATE TABLE filters_migr AS SELECT * FROM filters;
              DROP TABLE filters;
              CREATE TABLE filters (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                target TEXT NOT NULL CHECK (target IN
                  ('video_title','video_desc','channel_title','channel_id',
                   'chat_text','chat_author')),
                kind TEXT NOT NULL CHECK (kind IN ('literal','regex','highlight')),
                pattern TEXT NOT NULL,
                enabled INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
              );
              INSERT INTO filters
                (id, target, kind, pattern, enabled, created_at)
                SELECT id, target, kind, pattern, enabled, created_at
                FROM filters_migr;
              DROP TABLE filters_migr;",
    },
    // Phase 76: video_desc フィルタの評価対象化（FR-85、仕様決定 DJ）。
    // InnerTube `player` 応答の `videoDetails.shortDescription` を遅延解決で
    // 保存する説明列。NULL は未解決、空文字は解決済みで説明なし
    // （応答に shortDescription が無い・空の場合に空文字を書き、
    //   説明を持たない行への繰り返し問い合わせを抑止する）。
    // 既存行はすべて NULL になり、遅延解決の対象として順次解決される。
    Migration {
        version: 22,
        name: "videos_description",
        sql: "ALTER TABLE videos ADD COLUMN description TEXT;",
    },
];
