//! データダンプのエクスポート/インポート（FR-62、仕様決定 CM）。
//!
//! `channels` / `categories` / `settings` / `playlists`（＋項目）を
//! 1 つの JSON (`ExportDump`) に固めて書き出し、同じ形式を読み戻す。
//! インポートは「ダンプした時点の状態へ戻す」リストア扱いとし、
//! 対象 4 テーブルをダンプ内容で置き換える（暫定解釈）。
//! 置き換えは 1 トランザクションで行い、途中失敗では何も変えない。

use super::*;
use crate::model::{
    is_channel_id, is_playlist_source_id, is_video_id, Category, ExportChannel, ExportDump,
    ExportPlaylist, ExportPlaylistItem, ExportSetting, EXPORT_FORMAT_VERSION,
    MAX_PLAYLIST_NAME_LEN, MAX_SETTING_KEY_LEN, MAX_SETTING_VALUE_LEN,
};
use std::collections::{BTreeMap, BTreeSet};

/// ローカル環境依存または秘密情報を含むため、ダンプ出力から除外し
/// インポート時もローカル既存値を保持する設定キー（FR-94・FR-97、仕様決定 DX・EC）。
pub(crate) const LOCAL_PROTECTED_SETTINGS: &[&str] = &[
    "ytdlp.po_token",
    "ytdlp.path",
    "mpv.extra_args",
    "setup.deps_shown",
    "youtube.api_key",
];

/// 指定キーがローカル保護対象の設定か判定する（FR-94、仕様決定 DX）。
pub(crate) fn is_local_protected_setting(key: &str) -> bool {
    LOCAL_PROTECTED_SETTINGS.contains(&key)
}

/// プレイリスト項目の `channel_id` を正規化する（FR-94、仕様決定 DX）。
/// 正規 UC 形式（`is_channel_id`）のみ通し、非 UC 値は `None` に倒す。
fn normalize_playlist_item_channel_id(channel_id: Option<&str>) -> Option<String> {
    channel_id.filter(|s| is_channel_id(s)).map(str::to_string)
}

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
                let key: String = row.get(0)?;
                if is_local_protected_setting(&key) {
                    continue;
                }
                out.push(ExportSetting {
                    key,
                    value: row.get(1)?,
                });
            }
            out
        };
        let playlists = {
            let mut stmt = conn.prepare(
                "SELECT id, name, sort_order, source_list_id, synced_at
                 FROM playlists ORDER BY sort_order, id",
            )?;
            let mut rows = stmt.query([])?;
            let mut out = Vec::new();
            while let Some(row) = rows.next()? {
                out.push((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                ));
            }
            let mut item_stmt = conn.prepare(
                "SELECT i.video_id, i.position, v.title,
                        NULLIF(v.channel_id, ''), v.channel_title, v.thumbnail_url
                 FROM playlist_items i LEFT JOIN videos v ON v.video_id = i.video_id
                 WHERE i.playlist_id = ?1 ORDER BY i.position",
            )?;
            let mut playlists = Vec::with_capacity(out.len());
            for (id, name, sort_order, source_list_id, synced_at) in out {
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
                    source_list_id,
                    synced_at,
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

    /// `ExportDump` の内容で対象テーブルを置き換え、変化した非保護設定キーの
    /// `(key, インポート後の Option<String>)` 一覧を返す（FR-62・FR-94、仕様決定 CM・DX）。
    /// 先に内容の検証（形式版・ID 形式・長さ上限・ID 集合・外部キー参照）を行い、
    /// 受理できたダンプだけを 1 トランザクションで書き込む。
    ///
    /// 置き換えの規則:
    /// - `channels`/`categories`/`settings`/`playlists`/`playlist_items` は
    ///   ダンプの内容に一致する状態へ戻す（ダンプに無い行は消える）。
    /// - `LOCAL_PROTECTED_SETTINGS` の 5 キーは削除前にローカル既存値を退避し、
    ///   ダンプ側の同 5 キーを読み飛ばした上で退避値を復元する（FR-94・FR-97、仕様決定 DX・EC）。
    /// - `videos` 台帳は触らず、プレイリスト項目の表示メタだけ
    ///   `video_upsert` と同じ規則で補完する（既存メタは壊さない）。
    ///   `ExportPlaylistItem.channel_id` の非 UC 値は `None` へ正規化する。
    /// - `watch_history` など対象外のデータは保持する。
    /// - `DELETE FROM playlists` に伴い `queue_state` のプレイリストキュー行は
    ///   外部キー `ON DELETE CASCADE` で削除される。
    /// - ダンプ内で `categories.id` / `playlists.id` はエクスポート時の値を
    ///   そのまま使う（別環境で ID が衝突しても参照だけ合えばよい。
    ///   AUTOINCREMENT 採番は最大 ID 以降へ進む）。
    pub fn import_dump(&self, dump: &ExportDump) -> Result<Vec<(String, Option<String>)>, DbError> {
        validate_dump(dump)?;
        let mut conn = self.lock()?;
        let tx = conn.transaction()?;
        // `DELETE FROM settings` の直前に既存設定を全件読み取り、
        // (1) 保護 5 キーの退避・復元 と (2) 非保護キーの前後差分算出 に使う。
        let before_settings: BTreeMap<String, String> = {
            let mut stmt = tx.prepare("SELECT key, value FROM settings ORDER BY key")?;
            let mut rows = stmt.query([])?;
            let mut map = BTreeMap::new();
            while let Some(row) = rows.next()? {
                map.insert(row.get::<_, String>(0)?, row.get::<_, String>(1)?);
            }
            map
        };
        let preserved_protected: Vec<(&String, &String)> = before_settings
            .iter()
            .filter(|(k, _)| is_local_protected_setting(k))
            .collect();
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
        let mut after_settings: BTreeMap<String, String> = BTreeMap::new();
        for s in &dump.settings {
            if is_local_protected_setting(&s.key) {
                continue;
            }
            tx.execute(
                "INSERT INTO settings (key, value) VALUES (?1, ?2)",
                rusqlite::params![s.key, s.value],
            )?;
            after_settings.insert(s.key.clone(), s.value.clone());
        }
        for (k, v) in preserved_protected {
            tx.execute(
                "INSERT INTO settings (key, value) VALUES (?1, ?2)",
                rusqlite::params![k, v],
            )?;
        }
        for p in &dump.playlists {
            tx.execute(
                "INSERT INTO playlists (id, name, sort_order, source_list_id, synced_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![
                    p.id,
                    p.name.trim(),
                    p.sort_order,
                    p.source_list_id,
                    p.synced_at
                ],
            )?;
            for it in &p.items {
                // 表示メタの補完登録（台帳の既存メタは非空の値だけで補強する）。
                // channel_id の非 UC 値のみ None へ正規化する（FR-94、仕様決定 DX）。
                Self::video_upsert(
                    &tx,
                    &crate::model::VideoRef {
                        video_id: it.video_id.clone(),
                        title: it.title.clone().unwrap_or_default(),
                        channel_id: normalize_playlist_item_channel_id(it.channel_id.as_deref()),
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
        let mut all_keys = BTreeSet::new();
        for k in before_settings.keys() {
            if !is_local_protected_setting(k) {
                all_keys.insert(k.as_str());
            }
        }
        for k in after_settings.keys() {
            if !is_local_protected_setting(k) {
                all_keys.insert(k.as_str());
            }
        }
        let mut changed = Vec::new();
        for k in all_keys {
            let before = before_settings.get(k);
            let after = after_settings.get(k);
            if before != after {
                changed.push((k.to_string(), after.cloned()));
            }
        }
        Ok(changed)
    }
}

/// ダンプの受理可否を検証する（書き込み前の全文検査、FR-62・FR-94）。
/// 形式版の一致に加えて、ID 形式・長さ上限・ID の一意性と外部キー参照の有効性を確認する。
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
        if !is_channel_id(&ch.channel_id) {
            return Err(import_error(format!(
                "チャンネル ID {} の形式が不正です",
                ch.channel_id
            )));
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
        if is_local_protected_setting(&s.key) {
            continue;
        }
        if s.key.is_empty() {
            return Err(import_error("設定キーが空の行があります"));
        }
        if s.key.len() > MAX_SETTING_KEY_LEN {
            return Err(import_error(format!(
                "設定キーが長すぎます（{MAX_SETTING_KEY_LEN} バイト上限）"
            )));
        }
        if s.value.len() > MAX_SETTING_VALUE_LEN {
            return Err(import_error(format!(
                "設定キー {} の値が長すぎます（{MAX_SETTING_VALUE_LEN} バイト上限）",
                s.key
            )));
        }
        if !setting_keys.insert(s.key.as_str()) {
            return Err(import_error(format!("設定キー {} が重複しています", s.key)));
        }
    }
    let mut playlist_ids = std::collections::HashSet::new();
    for p in &dump.playlists {
        let trimmed_name = p.name.trim();
        if trimmed_name.is_empty() {
            return Err(import_error("プレイリスト名が空の行があります"));
        }
        if trimmed_name.chars().count() > MAX_PLAYLIST_NAME_LEN {
            return Err(import_error(format!(
                "プレイリスト名が長すぎます（{MAX_PLAYLIST_NAME_LEN} 文字上限）"
            )));
        }
        if !playlist_ids.insert(p.id) {
            return Err(import_error(format!(
                "プレイリスト ID {} が重複しています",
                p.id
            )));
        }
        if let Some(ref src_id) = p.source_list_id {
            if !is_playlist_source_id(src_id) {
                return Err(import_error(format!(
                    "プレイリスト {} の source_list_id 形式が不正です",
                    p.id
                )));
            }
        }
        if let Some(ref ts) = p.synced_at {
            if ts.is_empty() || ts.len() > 64 || crate::util::iso8601_ms(ts).is_none() {
                return Err(import_error(format!(
                    "プレイリスト {} の synced_at 形式が不正です",
                    p.id
                )));
            }
        }
        let mut item_ids = std::collections::HashSet::new();
        for it in &p.items {
            if it.video_id.trim().is_empty() {
                return Err(import_error("動画 ID が空のプレイリスト項目があります"));
            }
            if !is_video_id(&it.video_id) {
                return Err(import_error(format!(
                    "プレイリスト {} の動画 ID {} の形式が不正です",
                    p.id, it.video_id
                )));
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
