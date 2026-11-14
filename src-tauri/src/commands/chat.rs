//! ライブチャット系コマンド（設計書 §3.1、FR-6）。

use std::sync::Arc;

use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder};

use super::parse_video_id;
use crate::db::Db;
use crate::error::UiError;

/// `chat_start`（設計書 §3.1、FR-6）。指定動画のライブチャット取得を
/// バックグラウンドで開始する。見つからない・失敗した場合の通知は
/// `chat://status` イベントに流れる。
/// `instance_id` は埋め込みパネルの起票インスタンスで、利用者登録
/// （`panel:<id>`）とリプレイの同期先固定（FR-24）の両方に使う。
/// 未指定なら同じ動画を再生中のいずれかの位置で同期する
#[tauri::command]
pub fn chat_start(
    video_id: String,
    instance_id: Option<u32>,
    poller: State<'_, Arc<crate::chat::ChatPoller>>,
) -> Result<(), UiError> {
    let id = parse_video_id(&video_id)?;
    poller.acquire(&id, &crate::chat::panel_consumer(instance_id), instance_id);
    Ok(())
}

/// `chat_stop`。埋め込みパネルの利用者登録（`panel:<id>`）を解除する。
/// 同じ動画の利用者（他パネル・ポップアップ）が残っていれば共有ポーラーは
/// 維持され、最後の利用者がいなくなった時点で止まる（FR-27、仕様決定 AT）
#[tauri::command]
pub fn chat_stop(
    video_id: String,
    instance_id: Option<u32>,
    poller: State<'_, Arc<crate::chat::ChatPoller>>,
) -> Result<(), UiError> {
    let id = parse_video_id(&video_id)?;
    poller.release(&id, &crate::chat::panel_consumer(instance_id));
    Ok(())
}

/// `chat_popup_open`（FR-27、仕様決定 AT）。動画単位に 1 窓の
/// チャットポップアップを開く。既に開いていれば既存窓をフォーカスする。
/// ポップアップの利用者登録は窓作成前に行い、窓の破棄イベントで解除する
/// （ポーラーは全利用者がいなくなるまで維持される）。
/// 最前面は設定 `chat.popup_ontop`（未設定・不正は `on`）で決め、
/// `pip.default` と同じ受理集合で解釈する。
/// 位置とサイズは設定 `chat.popup_geometry`（FR-67、仕様決定 CR）の
/// 保存値から復元する。位置はモニターと十分に重なるときだけ適用し、
/// 画面外の保存値は位置だけ既定に戻す（サイズはそのまま復元する）。
/// 移動・リサイズは窓イベントで [`crate::chat::popup::PopupGeomTracker`]
/// へ記録し、間引きながら同じ設定キーへ保存する。
/// async コマンドにするのは、WebView2 の webview 初期化がメッセージ
/// ポンプを必要とするためで、メインスレッドで `build()` を呼ぶと
/// 初期化待ちがイベントループを塞いでデッドロックする（実機で確認）
#[tauri::command]
pub async fn chat_popup_open(
    app: AppHandle,
    db: State<'_, Db>,
    poller: State<'_, Arc<crate::chat::ChatPoller>>,
    video_id: String,
    instance_id: Option<u32>,
) -> Result<(), UiError> {
    let id = parse_video_id(&video_id)?;
    let label = crate::chat::popup_label(&id);
    if let Some(w) = app.get_webview_window(&label) {
        let _ = w.set_focus();
        return Ok(());
    }
    let ontop =
        crate::mpv::pip_default_enabled(db.setting_get(crate::chat::SETTING_CHAT_POPUP_ONTOP)?);
    // 復元対象の geometry。保存値が無い・形式外は既定（位置は OS 任せ）
    let geom = crate::chat::popup::parse_popup_geometry(
        db.setting_get(crate::chat::SETTING_CHAT_POPUP_GEOMETRY)?,
    );
    // 画面外復元のガード（FR-67）。モニター矩形を論理ピクセルへ揃えて
    // 重なり判定する。列挙に失敗・0 面の場合は判定不能として位置も復元する
    let pos_visible = geom.is_some_and(|g| {
        let monitors: Vec<(i32, i32, u32, u32)> = app
            .available_monitors()
            .unwrap_or_default()
            .iter()
            .map(|m| {
                let sf = m.scale_factor();
                let p = m.position().to_logical::<f64>(sf);
                let s = m.size().to_logical::<f64>(sf);
                (
                    p.x.round() as i32,
                    p.y.round() as i32,
                    s.width.round() as u32,
                    s.height.round() as u32,
                )
            })
            .collect();
        crate::chat::popup::popup_geometry_visible(&g, &monitors)
    });
    poller.acquire(&id, crate::chat::POPUP_CONSUMER, instance_id);
    let mut url = format!("chat?v={id}");
    if let Some(i) = instance_id {
        url.push_str(&format!("&i={i}"));
    }
    let mut builder = WebviewWindowBuilder::new(&app, &label, WebviewUrl::App(url.into()))
        .title(format!("ライブチャット - {id}"))
        .always_on_top(ontop);
    builder = match geom {
        Some(g) => {
            let b = builder.inner_size(f64::from(g.w), f64::from(g.h));
            if pos_visible {
                b.position(f64::from(g.x), f64::from(g.y))
            } else {
                b
            }
        }
        None => builder.inner_size(360.0, 560.0),
    };
    match builder.build() {
        Ok(w) => {
            let poller = poller.inner().clone();
            let vid = id.clone();
            let db = db.inner().clone();
            let tracker = crate::chat::popup::PopupGeomTracker::new();
            let w2 = w.clone();
            w.on_window_event(move |ev| {
                match ev {
                    tauri::WindowEvent::Moved(pos) => {
                        let sf = w2.scale_factor().unwrap_or(1.0);
                        let p = pos.to_logical::<f64>(sf);
                        // 位置イベントしか来ていない場合に備えて、
                        // 未記録のサイズは窓の現在値で補う
                        if tracker.needs_size() {
                            if let Ok(s) = w2.inner_size() {
                                let s = s.to_logical::<f64>(sf);
                                tracker.fill_size(s.width.round() as u32, s.height.round() as u32);
                            }
                        }
                        tracker.note_moved(p.x.round() as i32, p.y.round() as i32);
                        save_popup_geometry(&db, &tracker, false);
                    }
                    tauri::WindowEvent::Resized(size) => {
                        let sf = w2.scale_factor().unwrap_or(1.0);
                        let s = size.to_logical::<f64>(sf);
                        if tracker.needs_pos() {
                            if let Ok(p) = w2.outer_position() {
                                let p = p.to_logical::<f64>(sf);
                                tracker.fill_pos(p.x.round() as i32, p.y.round() as i32);
                            }
                        }
                        tracker.note_resized(s.width.round() as u32, s.height.round() as u32);
                        save_popup_geometry(&db, &tracker, false);
                    }
                    tauri::WindowEvent::ScaleFactorChanged { new_inner_size, .. } => {
                        // モニター間移動で scale factor が変わると論理値が
                        // 変わるため、新しい内サイズをリサイズとして記録する
                        let sf = w2.scale_factor().unwrap_or(1.0);
                        let s = new_inner_size.to_logical::<f64>(sf);
                        tracker.note_resized(s.width.round() as u32, s.height.round() as u32);
                        save_popup_geometry(&db, &tracker, false);
                    }
                    tauri::WindowEvent::Destroyed => {
                        // 間引きで残った末尾分を含め、最終 geometry を確定保存する
                        save_popup_geometry(&db, &tracker, true);
                        poller.release(&vid, crate::chat::POPUP_CONSUMER);
                    }
                    _ => {}
                }
            });
            Ok(())
        }
        Err(e) => {
            // 窓が作れなかった場合は登録した利用者を巻き戻す
            poller.release(&id, crate::chat::POPUP_CONSUMER);
            Err(UiError::internal(format!(
                "ポップアップ窓の作成に失敗: {e}"
            )))
        }
    }
}

/// トラッカーが持つ未保存の geometry を `chat.popup_geometry` へ書き出す
/// （FR-67、仕様決定 CR）。`force` は窓の破棄時の最終確定用。
/// 保存失敗は窓の操作を妨げないためログに留める。
fn save_popup_geometry(db: &Db, tracker: &crate::chat::popup::PopupGeomTracker, force: bool) {
    let Some(value) = tracker.take_pending(force) else {
        return;
    };
    if let Err(e) = db.setting_set(crate::chat::SETTING_CHAT_POPUP_GEOMETRY, &value) {
        tracing::warn!("chat.popup_geometry の保存に失敗: {e}");
    }
}

/// `chat_popup_return`（FR-27、仕様決定 AT）。ポップアップ内の
/// 「パネルに戻す」から呼ばれ、ポップアップを閉じて起票元インスタンスの
/// 埋め込みパネルを開き直す。起票元が同じ動画を再生中でなければエラー
/// （ポップアップ側のボタンはこの条件で無効化される）。
/// 先にパネルの利用者を登録してからポップアップを閉じるため、
/// 切替中に共有ポーラーは止まらない。
#[tauri::command]
pub async fn chat_popup_return(
    app: AppHandle,
    poller: State<'_, Arc<crate::chat::ChatPoller>>,
    players: State<'_, crate::mpv::PlayerManager>,
    video_id: String,
    instance_id: u32,
) -> Result<(), UiError> {
    let id = parse_video_id(&video_id)?;
    if players.position_of_instance(instance_id, &id).is_none() {
        return Err(UiError::invalid_input(
            "戻り先のプレイヤーが見つかりません（終了済みか別の動画を再生中です）",
        ));
    }
    poller.acquire(
        &id,
        &crate::chat::panel_consumer(Some(instance_id)),
        Some(instance_id),
    );
    // メイン窓のパネル表示は `chat://open_panel` 購読側で開く
    let _ = app.emit_to(
        "main",
        "chat://open_panel",
        serde_json::json!({ "instanceId": instance_id, "videoId": id }),
    );
    if let Some(w) = app.get_webview_window(&crate::chat::popup_label(&id)) {
        let _ = w.close();
    }
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.set_focus();
    }
    Ok(())
}
