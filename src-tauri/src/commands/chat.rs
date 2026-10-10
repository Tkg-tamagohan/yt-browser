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
    poller.acquire(&id, crate::chat::POPUP_CONSUMER, instance_id);
    let mut url = format!("chat?v={id}");
    if let Some(i) = instance_id {
        url.push_str(&format!("&i={i}"));
    }
    match WebviewWindowBuilder::new(&app, &label, WebviewUrl::App(url.into()))
        .title(format!("ライブチャット - {id}"))
        .inner_size(360.0, 560.0)
        .always_on_top(ontop)
        .build()
    {
        Ok(w) => {
            let poller = poller.inner().clone();
            let vid = id.clone();
            w.on_window_event(move |ev| {
                if matches!(ev, tauri::WindowEvent::Destroyed) {
                    poller.release(&vid, crate::chat::POPUP_CONSUMER);
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
    // メイン窓のパネル表示は `chat://open-panel` 購読側で開く
    let _ = app.emit_to(
        "main",
        "chat://open-panel",
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

/// `chat_history_search`（FR-6 の保存・検索要件）。本文・投稿者名の FTS5 AND 検索。
#[tauri::command]
pub fn chat_history_search(
    video_id: Option<String>,
    query: String,
    limit: Option<u32>,
    db: State<'_, Db>,
) -> Result<Vec<crate::model::ChatEvent>, UiError> {
    let q = query.trim().to_string();
    if q.is_empty() {
        return Err(UiError::invalid_input("検索語が空です"));
    }
    if q.len() > 256 {
        return Err(UiError::invalid_input("検索語が長すぎます"));
    }
    let vid = match &video_id {
        Some(v) if !v.trim().is_empty() => Some(parse_video_id(v)?),
        _ => None,
    };
    Ok(db.chat_search(vid.as_deref(), &q, limit.unwrap_or(100).min(1000))?)
}
