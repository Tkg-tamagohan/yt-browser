//! DB・設定・yt-dlp のシステム系コマンド。

use tauri::{AppHandle, Emitter, State};

use crate::db::Db;
use crate::error::UiError;
use crate::model::{
    DbSize, DbStatus, DepStatus, ExportDump, MonitorInfo, SettingChanged, YtDlpStatus,
};
use crate::yt::{self, YtDlpResolver};

const MAX_SETTING_KEY_LEN: usize = 128;
const MAX_SETTING_VALUE_LEN: usize = 16 * 1024;

/// `ui.shortcuts`（FR-79、仕様決定 DD）。アプリ側キーボード
/// ショートカットの有効化。on 系のみ有効で、未設定・その他の値は
/// off（既定）。値の解釈・既定キーマップ・対象操作はすべてフロント側
/// （shortcuts-logic.ts / shortcuts.svelte.ts）が持ち、バックエンドは
/// 汎用の settings_get / settings_set で受け渡すだけのため、ここには
/// キー名の定数だけを置く（設計書の設定キー表との照合対象として
/// scripts/check_docs_consistency.py が要求するため宣言し、
/// Rust 側では参照しないので dead_code を抑止する）
#[allow(dead_code)]
pub const SETTING_UI_SHORTCUTS: &str = "ui.shortcuts";

fn validate_setting_key(key: &str) -> Result<(), UiError> {
    if key.is_empty() {
        return Err(UiError::invalid_input("key が空です"));
    }
    if key.len() > MAX_SETTING_KEY_LEN {
        return Err(UiError::invalid_input(format!(
            "key が長すぎます（{MAX_SETTING_KEY_LEN} 文字上限）"
        )));
    }
    Ok(())
}

/// DB の生存確認。起動直後のヘルスチェック用。
#[tauri::command]
pub fn db_status(db: State<'_, Db>) -> Result<DbStatus, UiError> {
    Ok(DbStatus {
        schema_version: db.schema_version()?,
    })
}

#[tauri::command]
pub fn settings_get(db: State<'_, Db>, key: String) -> Result<Option<String>, UiError> {
    validate_setting_key(&key)?;
    Ok(db.setting_get(&key)?)
}

#[tauri::command]
pub async fn settings_set(
    app: AppHandle,
    db: State<'_, Db>,
    players: State<'_, crate::mpv::PlayerManager>,
    key: String,
    value: String,
) -> Result<(), UiError> {
    validate_setting_key(&key)?;
    if value.len() > MAX_SETTING_VALUE_LEN {
        return Err(UiError::invalid_input(format!(
            "value が長すぎます（{MAX_SETTING_VALUE_LEN} バイト上限）"
        )));
    }
    db.setting_set(&key, &value)?;
    // PiP 追従の切替は稼働中のインスタンスへ即時反映する（仕様決定 AL）。
    // emit より先に配っておくと、画面の再読み取り表示と窓の実態が揃う
    if key == crate::mpv::SETTING_PIP_FIT_ASPECT {
        players
            .set_pip_fit_all(crate::mpv::pip_fit_aspect_enabled(Some(value.clone())))
            .await;
    }
    // コミット済みの値を載せて変更を通知する。設定値を保持する画面は
    // 再読み取りなしで同期できる。購読者がいなくても正常なので失敗は捨てる
    let _ = app.emit("settings://changed", SettingChanged { key, value });
    Ok(())
}

/// `monitor_list`（FR-31、仕様決定 AZ）。再生窓の起動ディスプレイ選択肢として
/// 接続中のモニターを列挙順で返す。列挙自体に失敗した環境では空列を返す。
#[tauri::command]
pub fn monitor_list(app: AppHandle) -> Vec<MonitorInfo> {
    let monitors = app.available_monitors().unwrap_or_default();
    // プライマリ判定は Monitor の等値比較が無いため、名前・位置・サイズの一致で推す
    let primary = app.primary_monitor().ok().flatten();
    monitors
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let pos = m.position();
            let size = m.size();
            let primary = primary
                .as_ref()
                .is_some_and(|p| p.name() == m.name() && p.position() == pos && p.size() == size);
            MonitorInfo {
                name: m.name().map(|n| n.to_string()),
                index: i as u32,
                x: pos.x,
                y: pos.y,
                width: size.width,
                height: size.height,
                primary,
            }
        })
        .collect()
}

/// `windowing_system`（FR-41・FR-42、仕様決定 BQ・BR）。mpv 窓の移動・
/// 前面化が動作するかを UI がボタン可否の判定に使う表示環境の分類を返す。
/// 値は "windows" / "x11" / "wayland" / "other"。Wayland は恒久制限で非対応。
#[tauri::command]
pub fn windowing_system() -> &'static str {
    crate::mpv::windowing_system()
}

/// yt-dlp の解決パスとバージョン（設計書 §5 の運用確認用）。
#[tauri::command]
pub async fn ytdlp_status(
    db: State<'_, Db>,
    resolver: State<'_, YtDlpResolver>,
) -> Result<YtDlpStatus, UiError> {
    let (path, version) = resolver.status(&db).await;
    Ok(YtDlpStatus { path, version })
}

/// `yt-dlp -U` による更新（設計書 §5 の更新機構）。
/// システム管理パスでは権限不足で失敗し得る。その場合もエラーをそのまま返す。
#[tauri::command]
pub async fn ytdlp_update(
    db: State<'_, Db>,
    resolver: State<'_, YtDlpResolver>,
) -> Result<String, UiError> {
    let path = resolver.resolve(&db).await.ok_or(yt::YtError::NotFound)?;
    Ok(yt::update(&path).await?)
}

/// `deps_status`（FR-74、仕様決定 CY）。mpv・yt-dlp・deno の検出状況・
/// バージョン・解決パスを返す。設定画面の診断節と初回起動の依存検出画面
/// （FR-75）が同じ一覧を表示する。
#[tauri::command]
pub async fn deps_status(
    db: State<'_, Db>,
    resolver: State<'_, YtDlpResolver>,
) -> Result<Vec<DepStatus>, UiError> {
    Ok(crate::deps::collect_deps_status(&db, &resolver).await)
}

/// `setup_deps_pending`（FR-75、仕様決定 CZ）。初回起動の依存検出画面を
/// まだ表示していないか（`setup.deps_shown` が on 系でないか）を返す。
#[tauri::command]
pub fn setup_deps_pending(db: State<'_, Db>) -> Result<bool, UiError> {
    let raw = db.setting_get(crate::deps::SETTING_SETUP_DEPS_SHOWN)?;
    Ok(!crate::deps::setup_deps_shown_enabled(raw))
}

/// `data_export`（FR-62、仕様決定 CM）。`channels`/`categories`/`settings`/
/// `playlists`（＋項目）を JSON ダンプして `path` へ書き出す。
/// `path` はフロント側の保存ダイアログで選ばせた値をそのまま受け取る。
#[tauri::command]
pub fn data_export(path: String, db: State<'_, Db>) -> Result<(), UiError> {
    let path = path.trim().to_string();
    if path.is_empty() {
        return Err(UiError::invalid_input("保存先パスが空です"));
    }
    let dump = db.export_dump()?;
    let json = serde_json::to_string_pretty(&dump)
        .map_err(|e| UiError::internal(format!("ダンプの直列化に失敗: {e}")))?;
    std::fs::write(&path, json)
        .map_err(|e| UiError::internal(format!("エクスポートファイルの書き込みに失敗: {e}")))?;
    Ok(())
}

/// `data_import`（FR-62、仕様決定 CM）。`data_export` が書き出した JSON を
/// 読み込み、対象テーブルをダンプ内容で置き換える。
/// ファイルの読み取り・形式検証に失敗した場合は DB を変更しない。
#[tauri::command]
pub fn data_import(path: String, db: State<'_, Db>) -> Result<(), UiError> {
    let path = path.trim().to_string();
    if path.is_empty() {
        return Err(UiError::invalid_input("読み込み元パスが空です"));
    }
    let text = std::fs::read_to_string(&path)
        .map_err(|e| UiError::invalid_input(format!("インポートファイルを読めません: {e}")))?;
    let dump: ExportDump = serde_json::from_str(&text)
        .map_err(|e| UiError::invalid_input(format!("インポート形式が不正です: {e}")))?;
    db.import_dump(&dump)?;
    Ok(())
}

/// `db_size`（FR-66、仕様決定 CQ）。DB 本体と WAL/SHM サイドファイルの
/// 合計バイト数を返す（設定画面の運用節の表示用）。
#[tauri::command]
pub fn db_size(db: State<'_, Db>) -> Result<DbSize, UiError> {
    Ok(db.db_size()?)
}

/// `db_vacuum`（FR-66、仕様決定 CQ）。`VACUUM` で DB ファイルを再構築する。
#[tauri::command]
pub fn db_vacuum(db: State<'_, Db>) -> Result<(), UiError> {
    db.db_vacuum()?;
    Ok(())
}

/// `db_cleanup_orphans`（FR-66、仕様決定 CQ）。孤児行（参照切れの
/// videos / playlist_items / favorites / watch_later）を削除する。
/// 戻り値は削除した行数。
#[tauri::command]
pub fn db_cleanup_orphans(db: State<'_, Db>) -> Result<u64, UiError> {
    Ok(db.db_cleanup_orphans()?)
}
