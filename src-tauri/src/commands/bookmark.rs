//! 動画内ブックマークのコマンド（FR-49、仕様決定 BZ、設計書 §3.1）。

use tauri::State;

use super::parse_video_id;
use crate::db::{Db, DbError};
use crate::error::UiError;
use crate::model::VideoBookmark;

/// ラベルの文字数上限（livewait の MAX_WAIT_TEXT_LEN と同じ扱い）。
const MAX_LABEL_LEN: usize = 1024;

/// `bookmark_add`（FR-49）。mpv の再生位置を整数秒へ切り捨てて保存する
/// （シークバーの位置表示と同じ規則）。既定ラベルは位置表記
/// （"h:mm:ss" / "m:ss"）。
/// 同一動画の同一位置の重複は `None` を返して抑止する
/// （暫定解釈: UNIQUE (video_id, position_sec)。既存行のラベルは維持）。
#[tauri::command]
pub fn bookmark_add(
    video_id: String,
    position_sec: f64,
    db: State<'_, Db>,
) -> Result<Option<VideoBookmark>, UiError> {
    let id = parse_video_id(&video_id)?;
    // NaN は max(0.0) が 0.0 を返して吸収する。負値も 0 に揃える
    let pos = position_sec.max(0.0).floor() as i64;
    let label = fmt_position_label(pos);
    Ok(db.bookmark_insert(&id, pos, &label)?)
}

/// `bookmark_list`。表示用タイトルは `videos` 台帳 → `watch_history` の順で
/// 解決する。並びは保存の新しい順（暫定解釈）。
#[tauri::command]
pub fn bookmark_list(db: State<'_, Db>) -> Result<Vec<VideoBookmark>, UiError> {
    Ok(db.bookmark_list()?)
}

/// `bookmark_remove`。未登録の行 ID でも正常終了とする（冪等）。
#[tauri::command]
pub fn bookmark_remove(id: i64, db: State<'_, Db>) -> Result<(), UiError> {
    db.bookmark_remove(id)?;
    Ok(())
}

/// `bookmark_set_label`。空・空白のみのラベルは位置表記へ戻す
/// （暫定解釈: 空ラベルの行は残さず既定表示に復帰させる）。
/// 対象が存在しなければ `DbError::NotFound`。
#[tauri::command]
pub fn bookmark_set_label(
    id: i64,
    label: String,
    db: State<'_, Db>,
) -> Result<VideoBookmark, UiError> {
    if label.chars().count() > MAX_LABEL_LEN {
        return Err(UiError::invalid_input("ラベルが長すぎます"));
    }
    let label = label.trim().to_string();
    let cur = db.bookmark_get(id)?.ok_or(DbError::NotFound)?;
    let label = if label.is_empty() {
        fmt_position_label(cur.position_sec)
    } else {
        label
    };
    db.bookmark_update_label(id, &label)?;
    db.bookmark_get(id)?.ok_or_else(|| DbError::NotFound.into())
}

/// 位置表記（"h:mm:ss" / "m:ss"）。既定ラベルと空ラベルの復帰値に使う。
/// `fmtDuration`（src/lib/format.ts）と同じ表記規則。負値は 0 に揃える。
fn fmt_position_label(sec: i64) -> String {
    let s = sec.max(0);
    let h = s / 3600;
    let m = (s % 3600) / 60;
    let r = s % 60;
    if h > 0 {
        format!("{h}:{m:02}:{r:02}")
    } else {
        format!("{m}:{r:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::fmt_position_label;

    /// FR-49/BZ: 既定ラベルの位置表記。m:ss 未満1時間、h:mm:ss それ以上。
    /// 負値・境界値は 0:00 / 繰り上がり側に揃える。
    #[test]
    fn fmt_position_label_formats() {
        assert_eq!(fmt_position_label(0), "0:00");
        assert_eq!(fmt_position_label(5), "0:05");
        assert_eq!(fmt_position_label(59), "0:59");
        assert_eq!(fmt_position_label(60), "1:00");
        assert_eq!(fmt_position_label(754), "12:34");
        assert_eq!(fmt_position_label(3599), "59:59");
        assert_eq!(fmt_position_label(3600), "1:00:00");
        assert_eq!(fmt_position_label(3661), "1:01:01");
        assert_eq!(fmt_position_label(-3), "0:00");
    }
}
