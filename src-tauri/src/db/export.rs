//! データダンプのエクスポート/インポート（FR-62、仕様決定 CM）。
//!
//! `channels` / `categories` / `settings` / `playlists`（＋項目）を
//! 1 つの JSON (`ExportDump`) に固めて書き出し、同じ形式を読み戻す。
//! インポートは「ダンプした時点の状態へ戻す」リストア扱いとし、
//! 対象 4 テーブルをダンプ内容で置き換える（暫定解釈）。
//! 置き換えは 1 トランザクションで行い、途中失敗では何も変えない。

use super::*;
use crate::model::{
    Category, ExportChannel, ExportDump, ExportPlaylist, ExportPlaylistItem, ExportSetting,
    EXPORT_FORMAT_VERSION,
};

/// インポート検証エラー。形式版の不一致や参照先の欠落など、
/// ダンプが受理できない内容のとき `DbError::Import` として返す。
fn import_error(message: impl Into<String>) -> DbError {
    DbError::Import(message.into())
}

impl Db {
    /// `ExportDump` 全体を組み立てる（FR-62）。取得は 1 回のロック内で行い、
    /// テーブル間の時点ずれ（片方だけ新しい状態）を避ける。
    pub fn export_dump(&self) -> Result<ExportDump, DbError> {
        let conn = self.lock()?;
        let channels = {
            let mut stmt = conn.prepare(
                "SELECT channel_id, title, thumbnail_url, category_id,
                        subscribed_at, last_polled_at, rss_etag, rss_last_modified,
                        backfill_videos_pos, backfill_streams_pos, backfill_shorts_pos
                 FROM channels ORDER BY channel_id",
            )?;
            let mut rows = stmt.query([])?;
            let mut out = Vec::new();
            while let Some(row) = rows.next()? {
                out.push(ExportChannel {
                    channel_id: row.get(0)?,
                    title: row.get(1)?,
                    thumbnail_url: row.get(2)?,
                    category_id: row.get(3)?,
                    subscribed_at: row.get(4)?,
                    last_polled_at: row.get(5)?,
                    rss_etag: row.get(6)?,
                    rss_last_modified: row.get(7)?,
                    backfill_videos_pos: row.get(8)?,
                    backfill_streams_pos: row.get(9)?,
                    backfill_shorts_pos: row.get(10)?,
                });
            }
            out
        };
        let categories = {
            let mut stmt = conn
                .prepare("SELECT id, name, sort_order FROM categories ORDER BY sort_order, id")?;
            let mut rows = stmt.query([])?;
            let mut out = Vec::new();
            while let Some(row) = rows.next()? {
                out.push(Category {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    sort_order: row.get(2)?,
                });
            }
            out
        };
        let settings = {
            let mut stmt = conn.prepare("SELECT key, value FROM settings ORDER BY key")?;
            let mut rows = stmt.query([])?;
            let mut out = Vec::new();
            while let Some(row) = rows.next()? {
                out.push(ExportSetting {
                    key: row.get(0)?,
                    value: row.get(1)?,
                });
            }
            out
        };
        let playlists = {
            let mut stmt =
                conn.prepare("SELECT id, name, sort_order FROM playlists ORDER BY sort_order, id")?;
            let mut rows = stmt.query([])?;
            let mut out = Vec::new();
            while let Some(row) = rows.next()? {
                out.push((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                ));
            }
            let mut item_stmt = conn.prepare(
                "SELECT i.video_id, i.position, v.title,
                        NULLIF(v.channel_id, ''), v.channel_title, v.thumbnail_url
                 FROM playlist_items i LEFT JOIN videos v ON v.video_id = i.video_id
                 WHERE i.playlist_id = ?1 ORDER BY i.position",
            )?;
            let mut playlists = Vec::with_capacity(out.len());
            for (id, name, sort_order) in out {
                let mut items = Vec::new();
                let mut item_rows = item_stmt.query([id])?;
                while let Some(row) = item_rows.next()? {
                    items.push(ExportPlaylistItem {
                        video_id: row.get(0)?,
                        position: row.get(1)?,
                        title: row.get(2)?,
                        channel_id: row.get(3)?,
                        channel_title: row.get(4)?,
                        thumbnail_url: row.get(5)?,
                    });
                }
                playlists.push(ExportPlaylist {
                    id,
                    name,
                    sort_order,
                    items,
                });
            }
            playlists
        };
        let exported_at: String = conn.query_row("SELECT datetime('now')", [], |row| row.get(0))?;
        Ok(ExportDump {
            version: EXPORT_FORMAT_VERSION,
            exported_at,
            channels,
            categories,
            settings,
            playlists,
        })
    }

    /// `ExportDump` の内容で対象テーブルを置き換える（FR-62）。
    /// 先に内容の検証（形式版・ID 集合・外部キー参照）を行い、受理できた
    /// ダンプだけを 1 トランザクションで書き込む。
    ///
    /// 置き換えの暫定解釈:
    /// - `channels`/`categories`/`settings`/`playlists`/`playlist_items` は
    ///   ダンプの内容に一致する状態へ戻す（ダンプに無い行は消える）。
    /// - `videos` 台帳は触らず、プレイリスト項目の表示メタだけ
    ///   `video_upsert` と同じ規則で補完する（既存メタは壊さない）。
    /// - `watch_history` など対象外のデータは保持する。
    /// - ダンプ内で `categories.id` / `playlists.id` はエクスポート時の値を
    ///   そのまま使う（別環境で ID が衝突しても参照だけ合えばよい。
    ///   AUTOINCREMENT 採番は最大 ID 以降へ進む）。
    pub fn import_dump(&self, dump: &ExportDump) -> Result<(), DbError> {
        validate_dump(dump)?;
        let mut conn = self.lock()?;
        let tx = conn.transaction()?;
        // 参照順の都合で項目→本体の順に消し、本体→項目の順に戻す。
        // channels.category_id は ON DELETE SET NULL のため、先に
        // channels を消してから categories を消すと NULL 化 UPDATE が
        // 走る。順序を逆にしても結果は同じだが、無駄な更新を避けて
        // channels → categories の順にする
        tx.execute("DELETE FROM playlist_items", [])?;
        tx.execute("DELETE FROM playlists", [])?;
        tx.execute("DELETE FROM channels", [])?;
        tx.execute("DELETE FROM categories", [])?;
        tx.execute("DELETE FROM settings", [])?;
        for c in &dump.categories {
            tx.execute(
                "INSERT INTO categories (id, name, sort_order) VALUES (?1, ?2, ?3)",
                rusqlite::params![c.id, c.name, c.sort_order],
            )?;
        }
        for ch in &dump.channels {
            tx.execute(
                "INSERT INTO channels
                   (channel_id, title, thumbnail_url, category_id, subscribed_at,
                    last_polled_at, rss_etag, rss_last_modified,
                    backfill_videos_pos, backfill_streams_pos, backfill_shorts_pos)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                rusqlite::params![
                    ch.channel_id,
                    ch.title,
                    ch.thumbnail_url,
                    ch.category_id,
                    ch.subscribed_at,
                    ch.last_polled_at,
                    ch.rss_etag,
                    ch.rss_last_modified,
                    ch.backfill_videos_pos,
                    ch.backfill_streams_pos,
                    ch.backfill_shorts_pos,
                ],
            )?;
        }
        for s in &dump.settings {
            tx.execute(
                "INSERT INTO settings (key, value) VALUES (?1, ?2)",
                rusqlite::params![s.key, s.value],
            )?;
        }
        for p in &dump.playlists {
            tx.execute(
                "INSERT INTO playlists (id, name, sort_order) VALUES (?1, ?2, ?3)",
                rusqlite::params![p.id, p.name, p.sort_order],
            )?;
            for it in &p.items {
                // 表示メタの補完登録（台帳の既存メタは非空の値だけで補強する）
                Self::video_upsert(
                    &tx,
                    &crate::model::VideoRef {
                        video_id: it.video_id.clone(),
                        title: it.title.clone().unwrap_or_default(),
                        channel_id: it.channel_id.clone(),
                        channel_title: it.channel_title.clone(),
                        thumbnail_url: it.thumbnail_url.clone(),
                    },
                )?;
                tx.execute(
                    "INSERT INTO playlist_items (playlist_id, video_id, position)
                     VALUES (?1, ?2, ?3)",
                    rusqlite::params![p.id, it.video_id, it.position],
                )?;
            }
        }
        tx.commit()?;
        Ok(())
    }
}

/// ダンプの受理可否を検証する（書き込み前の全文検査）。
/// 形式版の一致に加えて、ID の一意性と外部キー参照の有効性を確認する。
fn validate_dump(dump: &ExportDump) -> Result<(), DbError> {
    if dump.version != EXPORT_FORMAT_VERSION {
        return Err(import_error(format!(
            "未対応のダンプ形式です（version {}、対応は {}）",
            dump.version, EXPORT_FORMAT_VERSION
        )));
    }
    let mut category_ids = std::collections::HashSet::new();
    for c in &dump.categories {
        if c.name.trim().is_empty() {
            return Err(import_error("カテゴリ名が空の行があります"));
        }
        if !category_ids.insert(c.id) {
            return Err(import_error(format!(
                "カテゴリ ID {} が重複しています",
                c.id
            )));
        }
    }
    let mut channel_ids = std::collections::HashSet::new();
    for ch in &dump.channels {
        if ch.channel_id.trim().is_empty() || ch.title.trim().is_empty() {
            return Err(import_error(
                "チャンネル ID またはタイトルが空の行があります",
            ));
        }
        if !channel_ids.insert(ch.channel_id.as_str()) {
            return Err(import_error(format!(
                "チャンネル {} が重複しています",
                ch.channel_id
            )));
        }
        if let Some(cid) = ch.category_id {
            if !category_ids.contains(&cid) {
                return Err(import_error(format!(
                    "チャンネル {} のカテゴリ ID {} はダンプ内にありません",
                    ch.channel_id, cid
                )));
            }
        }
    }
    let mut setting_keys = std::collections::HashSet::new();
    for s in &dump.settings {
        if s.key.is_empty() {
            return Err(import_error("設定キーが空の行があります"));
        }
        if !setting_keys.insert(s.key.as_str()) {
            return Err(import_error(format!("設定キー {} が重複しています", s.key)));
        }
    }
    let mut playlist_ids = std::collections::HashSet::new();
    for p in &dump.playlists {
        if p.name.trim().is_empty() {
            return Err(import_error("プレイリスト名が空の行があります"));
        }
        if !playlist_ids.insert(p.id) {
            return Err(import_error(format!(
                "プレイリスト ID {} が重複しています",
                p.id
            )));
        }
        let mut item_ids = std::collections::HashSet::new();
        for it in &p.items {
            if it.video_id.trim().is_empty() {
                return Err(import_error("動画 ID が空のプレイリスト項目があります"));
            }
            if it.position < 0 {
                return Err(import_error("position が負のプレイリスト項目があります"));
            }
            if !item_ids.insert(it.video_id.as_str()) {
                return Err(import_error(format!(
                    "プレイリスト {} 内で動画 {} が重複しています",
                    p.id, it.video_id
                )));
            }
        }
    }
    Ok(())
}
