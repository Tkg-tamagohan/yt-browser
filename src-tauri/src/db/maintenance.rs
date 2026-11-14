//! DB メンテナンス（FR-66、仕様決定 CQ）。
//!
//! 設定画面の運用節が使う DB サイズ取得・`VACUUM`・孤児行整理。
//! いずれも設定画面のボタン操作で明示的に実行する。

use super::*;
use crate::model::DbSize;

impl Db {
    /// DB 関連ファイルの合計サイズ（バイト）。
    /// `PRAGMA database_list` の `main` データベースファイルと、WAL モードの
    /// 副作用ファイル（`-wal`・`-shm`）の合計とする（暫定解釈）。
    /// インメモリ接続などファイルを持たない場合は `PRAGMA page_count ×
    /// page_size` の近似値を返す。
    pub fn db_size(&self) -> Result<DbSize, DbError> {
        let conn = self.lock()?;
        let path: Option<String> = conn.query_row(
            "SELECT file FROM pragma_database_list WHERE name = 'main'",
            [],
            |row| row.get(0),
        )?;
        let path = path.filter(|p| !p.is_empty());
        if let Some(path) = path {
            // 本体 + 存在するサイドファイルを合算する。
            // 読み取り失敗はサイズ 0 として扱い、全体を失敗にしない
            // （表示用の目安値のため）
            let mut total = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            for suffix in ["-wal", "-shm"] {
                let side = format!("{path}{suffix}");
                total += std::fs::metadata(&side).map(|m| m.len()).unwrap_or(0);
            }
            return Ok(DbSize { bytes: total });
        }
        // ファイルを持たない接続（テストのインメモリ DB）の近似値
        let bytes: i64 = conn.query_row(
            "SELECT page_count * page_size FROM pragma_page_count, pragma_page_size",
            [],
            |row| row.get(0),
        )?;
        Ok(DbSize {
            bytes: bytes.max(0) as u64,
        })
    }

    /// `VACUUM` で DB ファイルを再構築する（FR-66）。
    /// WAL モードではチェックポイント済みの内容で本体ファイルが再構築され、
    /// 空きページが実際に縮む。接続は 1 本だけなのでロック内でそのまま実行できる。
    pub fn db_vacuum(&self) -> Result<(), DbError> {
        let conn = self.lock()?;
        conn.execute_batch("VACUUM")?;
        Ok(())
    }

    /// 孤児行の整理（FR-66）。戻り値は削除した行数。
    ///
    /// 対象とする孤児行（暫定解釈）:
    /// - `videos`: `channels` に存在しないチャンネル ID を持つ行のうち、
    ///   どの参照元（お気に入り・プレイリスト項目・あとで見る・履歴・
    ///   キュー・ライブ待機・ブックマーク）にも残っていないもの。
    ///   購読解除後に残った配信行を消す（`channel_delete` は videos を
    ///   消さないため溜まる）。参照がある行は表示メタ解決のため残す。
    ///   チャンネル ID が空文字の行は「台帳メタ」として残す
    ///   （お気に入り等から参照される動画を消さないため）。
    /// - `playlist_items` / `favorites` / `watch_later`: `videos` 台帳に
    ///   無い `video_id` の行。一覧が台帳と INNER JOIN のため参照切れの
    ///   行は表示されず、掃除しても見え方は変わらない。
    /// - `watch_history` は表示メタを自列に持ち台帳へ依存しないため、
    ///   ここでは消さない（ユーザーの履歴データ）。
    pub fn db_cleanup_orphans(&self) -> Result<u64, DbError> {
        let mut conn = self.lock()?;
        let tx = conn.transaction()?;
        let videos = tx.execute(
            "DELETE FROM videos
             WHERE channel_id <> ''
               AND channel_id NOT IN (SELECT channel_id FROM channels)
               AND video_id NOT IN (SELECT video_id FROM favorites)
               AND video_id NOT IN (SELECT video_id FROM playlist_items)
               AND video_id NOT IN (SELECT video_id FROM watch_later)
               AND video_id NOT IN (SELECT video_id FROM watch_history)
               AND video_id NOT IN (SELECT video_id FROM queue_items)
               AND video_id NOT IN (SELECT video_id FROM live_waits)
               AND video_id NOT IN (SELECT video_id FROM video_bookmarks)",
            [],
        )?;
        let mut items = tx.execute(
            "DELETE FROM playlist_items
             WHERE video_id NOT IN (SELECT video_id FROM videos)",
            [],
        )?;
        items += tx.execute(
            "DELETE FROM favorites
             WHERE video_id NOT IN (SELECT video_id FROM videos)",
            [],
        )?;
        items += tx.execute(
            "DELETE FROM watch_later
             WHERE video_id NOT IN (SELECT video_id FROM videos)",
            [],
        )?;
        tx.commit()?;
        Ok((videos + items) as u64)
    }
}
