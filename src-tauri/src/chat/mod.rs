//! ライブチャットのポーリングと正規化（設計書 §6.2〜§7、FR-6、FR-9）。
//!
//! `chat_start` で動画ごとのポーリングタスクを立て、watch ページの
//! `ytInitialData` から初期継続トークンを取って `get_live_chat` を繰り返す。
//! 応答 1 回分のアクションを `ChatEvent` に正規化し、NG 判定と重複除去を経て
//! 1 トランザクションで `chat_logs` へ保存したうえで `chat://message` に流す。
//! 原文は raw_json として残し、削除アクションや未知 renderer も `other` /
//! `deleted` で記録する（設計書 §6.3 の「保存は別レイヤ、表示は制御する」方針）。

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::Value;
use tauri::{AppHandle, Emitter};

use crate::db::Db;
use crate::error::UiError;
use crate::filter::Matcher;
use crate::innertube::InnerTube;
use crate::model::{ChatEvent, ChatKind, ChatStatus};

/// 1 セッションで保持する既処理 item ID の上限（重複除去用）。
/// 超限の古い ID は捨てる（ごく古いアイテムの再送は稀で、
/// 重複 1 件が残るだけの影響で上限を超えてメモリを増やし続ける方が悪い）。
const SEEN_CAP: usize = 10_000;
/// 連続失敗がこの回数を超えたら `chat://status` に warn を出す。
const WARN_AFTER_FAILURES: u32 = 2;
/// ポーリング間隔の最小値（応答の timeoutMs が 0/欠落でも過剰に回さない）。
const MIN_POLL_MS: u64 = 300;

/// 動画ごとのチャットポーリングを管理する。`tauri::State` に `Arc` で載せる。
pub struct ChatPoller {
    db: Db,
    app: AppHandle,
    innertube: Arc<InnerTube>,
    /// 現在の NG 評価器。filter 変更のたびに丸ごと差し替える。
    matcher: Mutex<Arc<Matcher>>,
    /// `refresh_filters` の直列化用。一覧取得から差し替えまでを 1 つの
    /// 排他区間にし、並行する更新で古いマッチャが後勝ちするのを防ぐ。
    filter_lock: Mutex<()>,
    /// video_id -> 実行中タスクの abort handle。
    sessions: Mutex<HashMap<String, tokio::task::AbortHandle>>,
}

impl ChatPoller {
    pub fn new(db: Db, app: AppHandle, innertube: Arc<InnerTube>) -> Self {
        Self {
            db,
            app,
            innertube,
            matcher: Mutex::new(Arc::new(Matcher::empty())),
            filter_lock: Mutex::new(()),
            sessions: Mutex::new(HashMap::new()),
        }
    }

    /// `filters` テーブルの現在値で NG 評価器を作り直す。
    /// 起動時と `filter_add` / `filter_remove` の直後に呼ぶ。
    pub fn refresh_filters(&self) -> Result<(), UiError> {
        let _serialize = self.filter_lock.lock().unwrap();
        let rows = self.db.filter_list()?;
        let m = Matcher::rebuild(&rows);
        if !m.invalid_patterns().is_empty() {
            self.status(
                None,
                "warn",
                &format!(
                    "コンパイルに失敗したフィルタを除外しました（{} 件）",
                    m.invalid_patterns().len()
                ),
            );
        }
        *self.matcher.lock().unwrap() = Arc::new(m);
        Ok(())
    }

    /// 現在の NG 評価器を共有で取り出す。
    /// フィード・検索・関連動画の表示側フィルタ（動画系 target）に使う。
    pub fn matcher(&self) -> Arc<Matcher> {
        self.matcher.lock().unwrap().clone()
    }

    /// 指定動画のチャット取得を開始する。既に動いていれば何もしない。
    pub fn start(self: &Arc<Self>, video_id: &str) {
        let mut sessions = self.sessions.lock().unwrap();
        if sessions.contains_key(video_id) {
            return;
        }
        let poller = Arc::clone(self);
        let vid = video_id.to_string();
        let handle = tokio::spawn(async move { poller.run(&vid).await });
        sessions.insert(video_id.to_string(), handle.abort_handle());
    }

    /// 指定動画のチャット取得を止める。未起動なら何もしない。
    pub fn stop(&self, video_id: &str) {
        if let Some(h) = self.sessions.lock().unwrap().remove(video_id) {
            h.abort();
        }
    }

    /// 全セッションを止める（アプリ終了時）。
    pub fn stop_all(&self) {
        let handles: Vec<_> = self
            .sessions
            .lock()
            .unwrap()
            .drain()
            .map(|(_, h)| h)
            .collect();
        for h in handles {
            h.abort();
        }
    }

    /// `chat://status` を発行する。
    fn status(&self, video_id: Option<&str>, level: &str, message: &str) {
        let _ = self.app.emit(
            "chat://status",
            ChatStatus {
                video_id: video_id.map(|v| v.to_string()),
                level: level.to_string(),
                message: message.to_string(),
            },
        );
    }

    /// 1 動画のポーリングループ。終了・エラー・中断のいずれでも
    /// `sessions` から自分を外して終わる。
    async fn run(self: &Arc<Self>, video_id: &str) {
        let mut cont = match self.innertube.watch_html(video_id).await {
            Ok(html) => match crate::innertube::extract_initial_continuation(&html) {
                Some(c) => c,
                None => {
                    self.status(
                        Some(video_id),
                        "info",
                        "チャットが見つかりません（ライブ配信以外か、チャットが無効です）",
                    );
                    self.sessions.lock().unwrap().remove(video_id);
                    return;
                }
            },
            Err(e) => {
                self.status(
                    Some(video_id),
                    "error",
                    &format!("watch ページの取得に失敗: {e}"),
                );
                self.sessions.lock().unwrap().remove(video_id);
                return;
            }
        };

        let mut seen: HashSet<String> = HashSet::new();
        let mut order: VecDeque<String> = VecDeque::new();
        let mut failures: u32 = 0;
        loop {
            match self.innertube.get_live_chat(&cont).await {
                Ok(v) => {
                    if failures >= WARN_AFTER_FAILURES {
                        self.status(Some(video_id), "info", "チャット取得が復帰しました");
                    }
                    failures = 0;
                    let lcc = v
                        .get("continuationContents")
                        .and_then(|c| c.get("liveChatContinuation"));
                    // dedup は「保存確定済み seen」と「この応答内の pending」の
                    // 2 段で行う。保存に失敗したバッチは pending を捨てるだけで
                    // seen には入れないため、YouTube が item を再送したときに
                    // 履歴へ拾い直せる（UI 側にも再送されるので UI は item_id で dedup）。
                    // matcher は応答ごとに取り直し、フィルタ変更を走行中にも反映する。
                    let matcher = self.matcher();
                    let mut pending: HashSet<String> = HashSet::new();
                    let events = lcc
                        .and_then(|l| l.get("actions"))
                        .and_then(|a| a.as_array())
                        .map(|acts| normalize_all(&matcher, video_id, acts, &seen, &mut pending))
                        .unwrap_or_default();
                    if !events.is_empty() {
                        match self.db.chat_insert_batch(&events) {
                            Ok(_) => commit_pending(&mut seen, &mut order, &mut pending),
                            Err(e) => {
                                pending.clear();
                                self.status(
                                    Some(video_id),
                                    "warn",
                                    &format!("チャットの保存に失敗: {e}"),
                                );
                            }
                        }
                        let _ = self.app.emit("chat://message", &events);
                    }
                    match lcc.and_then(crate::innertube::next_continuation) {
                        Some((next, timeout)) => {
                            cont = next;
                            tokio::time::sleep(Duration::from_millis(timeout.max(MIN_POLL_MS)))
                                .await;
                        }
                        None => {
                            self.status(
                                Some(video_id),
                                "info",
                                "チャットが終了しました（配信終了またはチャットクローズ）",
                            );
                            break;
                        }
                    }
                }
                Err(e) => {
                    failures += 1;
                    if failures == WARN_AFTER_FAILURES {
                        self.status(
                            Some(video_id),
                            "warn",
                            &format!("チャット取得に連続して失敗しています: {e}"),
                        );
                    }
                    // 1→2→4→8→16→30 秒で待ち直す（上限 30 秒）
                    let backoff = (1u64 << failures.min(5)).min(30);
                    tokio::time::sleep(Duration::from_secs(backoff)).await;
                }
            }
        }
        self.sessions.lock().unwrap().remove(video_id);
    }
}

/// 応答の actions[] 全件を正規化し、重複を除き、NG 判定を付けて返す。
/// `seen` は保存確定済みの既処理 ID、`pending` はこの応答で処理した ID
/// （呼び出し側が DB 保存の成功時にだけ `commit_pending` で seen へ移す）。
fn normalize_all(
    matcher: &Matcher,
    video_id: &str,
    actions: &[Value],
    seen: &HashSet<String>,
    pending: &mut HashSet<String>,
) -> Vec<ChatEvent> {
    let mut items = Vec::new();
    for a in actions {
        iter_action_items(a, &mut items);
    }
    let mut out = Vec::new();
    for item in items {
        let Some(mut e) = (match item {
            ActionItem::Item(v) => renderer_to_event(video_id, &v),
            ActionItem::Deleted(target, raw) => Some(deleted_to_event(video_id, &target, &raw)),
        }) else {
            continue;
        };
        // item_id の無いイベント（一部 renderer）は dedup 対象外にする
        if !e.item_id.is_empty()
            && (seen.contains(&e.item_id) || !pending.insert(e.item_id.clone()))
        {
            continue;
        }
        e.ng = is_ng(matcher, &e);
        out.push(e);
    }
    out
}

/// 保存が成功したバッチの item ID を既処理集合へ確定し、上限を超えたら古い順に捨てる。
fn commit_pending(
    seen: &mut HashSet<String>,
    order: &mut VecDeque<String>,
    pending: &mut HashSet<String>,
) {
    for id in pending.drain() {
        seen.insert(id.clone());
        order.push_back(id);
    }
    while order.len() > SEEN_CAP {
        if let Some(old) = order.pop_front() {
            seen.remove(&old);
        }
    }
}

/// アクションから取り出した処理対象。`Item` は renderer を含む
/// `{<name>Renderer: {...}}` 形の値オブジェクト、`Deleted` は削除対象 ID と原文。
enum ActionItem {
    Item(Value),
    Deleted(String, Value),
}

/// 1 アクションオブジェクトから処理対象を全て列挙する。
/// `replayChatItemAction`（リプレイ由来）は内側の actions を再帰的に展開する。
/// 未知のアクションキーは `other` として保存するため Item に流す。
fn iter_action_items(action: &Value, out: &mut Vec<ActionItem>) {
    let Some(m) = action.as_object() else {
        return;
    };
    for (k, v) in m {
        match k.as_str() {
            "addChatItemAction" => {
                if let Some(item) = v.get("item") {
                    out.push(ActionItem::Item(item.clone()));
                }
            }
            "replayChatItemAction" => {
                if let Some(acts) = v.get("actions").and_then(|a| a.as_array()) {
                    for a in acts {
                        iter_action_items(a, out);
                    }
                }
            }
            "removeChatItemAction" | "markChatItemAsDeletedAction" => {
                if let Some(id) = v.get("targetItemId").and_then(|t| t.as_str()) {
                    out.push(ActionItem::Deleted(id.to_string(), v.clone()));
                }
            }
            "clickTrackingParams" => {}
            _ => out.push(ActionItem::Item(v.clone())),
        }
    }
}

/// `{<rendererName>: {...}}` 形のアイテムを `ChatEvent` に正規化する。
/// 未知 renderer は `other` として原文だけ残す。
fn renderer_to_event(video_id: &str, item: &Value) -> Option<ChatEvent> {
    let m = item.as_object()?;
    for (name, r) in m {
        if !name.ends_with("Renderer") {
            continue;
        }
        let (kind, message, amount) = match name.as_str() {
            "liveChatTextMessageRenderer" => (ChatKind::Text, message_text(r.get("message")), None),
            "liveChatPaidMessageRenderer" => (
                ChatKind::Superchat,
                message_text(r.get("message")),
                simple_text(r.get("purchaseAmountText")),
            ),
            "liveChatPaidStickerRenderer" => {
                let amt = simple_text(r.get("purchaseAmountText"));
                (
                    ChatKind::Superchat,
                    match &amt {
                        Some(a) => format!("[スタンプ {a}]"),
                        None => "[スタンプ]".to_string(),
                    },
                    amt,
                )
            }
            "liveChatMembershipItemRenderer"
            | "liveChatSponsorshipsGiftPurchaseAnnouncementRenderer" => {
                let msg = {
                    let t = message_text(r.get("message"));
                    if t.is_empty() {
                        message_text(r.get("headerSubtext"))
                    } else {
                        t
                    }
                };
                (ChatKind::Membership, msg, None)
            }
            _ => (ChatKind::Other, String::new(), None),
        };
        return Some(ChatEvent {
            item_id: r
                .get("id")
                .and_then(|i| i.as_str())
                .unwrap_or_default()
                .to_string(),
            video_id: video_id.to_string(),
            posted_at_usec: r
                .get("timestampUsec")
                .and_then(|t| t.as_str())
                .and_then(|s| s.parse::<i64>().ok())
                .unwrap_or_else(now_usec),
            author_channel_id: r
                .get("authorExternalChannelId")
                .and_then(|c| c.as_str())
                .map(|s| s.to_string()),
            author_name: r
                .get("authorName")
                .and_then(|n| n.get("simpleText"))
                .and_then(|s| s.as_str())
                .map(|s| s.to_string())
                .or_else(|| r.get("authorName").map(message_text_opt)),
            kind,
            message,
            amount_display: amount,
            ng: false,
            raw_json: serde_json::to_string(item).unwrap_or_default(),
        });
    }
    None
}

/// 削除アクションを `deleted` イベントとして正規化する。
/// `message` に削除対象の item ID を入れ、UI 側で該当行の打消しに使う。
fn deleted_to_event(video_id: &str, target_id: &str, raw: &Value) -> ChatEvent {
    ChatEvent {
        item_id: format!("del:{target_id}"),
        video_id: video_id.to_string(),
        posted_at_usec: now_usec(),
        author_channel_id: None,
        author_name: None,
        kind: ChatKind::Deleted,
        message: target_id.to_string(),
        amount_display: None,
        ng: false,
        raw_json: serde_json::to_string(raw).unwrap_or_default(),
    }
}

/// `{ "simpleText": "..." }` / `{ "runs": [{"text": ...} | {"emoji": ...}] }`
/// のどちらからも表示文字列を取る。
fn message_text(v: Option<&Value>) -> String {
    let Some(v) = v else {
        return String::new();
    };
    if let Some(t) = v.get("simpleText").and_then(|s| s.as_str()) {
        return t.to_string();
    }
    v.get("runs")
        .and_then(|r| r.as_array())
        .map(|runs| {
            runs.iter()
                .map(|r| {
                    if let Some(t) = r.get("text").and_then(|t| t.as_str()) {
                        t.to_string()
                    } else if let Some(e) = r.get("emoji") {
                        emoji_text(e)
                    } else {
                        String::new()
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

/// `message_text` の Option 版。
fn message_text_opt(v: &Value) -> String {
    message_text(Some(v))
}

fn simple_text(v: Option<&Value>) -> Option<String> {
    v.and_then(|v| v.get("simpleText"))
        .and_then(|s| s.as_str())
        .map(|s| s.to_string())
}

/// 絵文字 run の表示文字列。標準絵文字の `emojiId` は絵文字自体なのでそのまま使い、
/// カスタム絵文字（内部 ID）はアクセシビリティラベルかプレースホルダに落とす。
fn emoji_text(e: &Value) -> String {
    if let Some(id) = e.get("emojiId").and_then(|i| i.as_str()) {
        let looks_like_emoji = id.chars().all(|c| !c.is_ascii_alphanumeric() && c != '/');
        if looks_like_emoji {
            return id.to_string();
        }
    }
    e.get("image")
        .and_then(|i| i.get("accessibility"))
        .and_then(|a| a.get("accessibilityData"))
        .and_then(|d| d.get("label"))
        .and_then(|l| l.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "[絵文字]".to_string())
}

fn now_usec() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_micros() as i64)
        .unwrap_or(0)
}

/// ChatEvent への NG 判定。本文と投稿者（名前・チャンネル ID 双方）を対象にする。
fn is_ng(matcher: &Matcher, e: &ChatEvent) -> bool {
    if matcher.is_blocked("chat_text", &e.message) {
        return true;
    }
    for text in [e.author_name.as_deref(), e.author_channel_id.as_deref()]
        .into_iter()
        .flatten()
    {
        if matcher.is_blocked("chat_author", text) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn text_action(id: &str, author: &str, ch: &str, usec: &str, msg: &str) -> Value {
        json!({
            "addChatItemAction": {"item": {"liveChatTextMessageRenderer": {
                "id": id, "timestampUsec": usec,
                "authorName": {"simpleText": author},
                "authorExternalChannelId": ch,
                "message": {"runs": [{"text": msg}]}
            }}}
        })
    }

    /// CH-01: テキストアクションの正規化（id・時刻・投稿者・本文）。
    #[test]
    fn normalize_text_action() {
        let a = text_action("id1", "@alice", "UCxxxx", "1700000000000000", "hello");
        let mut items = Vec::new();
        iter_action_items(&a, &mut items);
        assert_eq!(items.len(), 1);
        let ev = match &items[0] {
            ActionItem::Item(v) => renderer_to_event("vid123", v).unwrap(),
            _ => panic!("expected item"),
        };
        assert_eq!(ev.kind, ChatKind::Text);
        assert_eq!(ev.item_id, "id1");
        assert_eq!(ev.author_name.as_deref(), Some("@alice"));
        assert_eq!(ev.author_channel_id.as_deref(), Some("UCxxxx"));
        assert_eq!(ev.posted_at_usec, 1700000000000000);
        assert_eq!(ev.message, "hello");
        assert!(!ev.raw_json.is_empty());
    }

    /// CH-02: スパチャは金額と色分類を持つ。
    #[test]
    fn normalize_superchat() {
        let a = json!({"addChatItemAction": {"item": {"liveChatPaidMessageRenderer": {
            "id": "sc1", "timestampUsec": "1700000000000001",
            "authorName": {"simpleText": "@bob"},
            "message": {"runs": [{"text": "nice!"}]},
            "purchaseAmountText": {"simpleText": "¥500"}
        }}}});
        let mut items = Vec::new();
        iter_action_items(&a, &mut items);
        let ev = match &items[0] {
            ActionItem::Item(v) => renderer_to_event("v", v).unwrap(),
            _ => panic!(),
        };
        assert_eq!(ev.kind, ChatKind::Superchat);
        assert_eq!(ev.amount_display.as_deref(), Some("¥500"));
        assert_eq!(ev.message, "nice!");
    }

    /// CH-03: 削除アクション（実測の removeChatItemAction と旧名の両方）を
    /// deleted イベントにし、対象 ID を message に載せる。
    #[test]
    fn normalize_delete_actions() {
        for key in ["removeChatItemAction", "markChatItemAsDeletedAction"] {
            let a = json!({key: {"targetItemId": "TGT123"}});
            let mut items = Vec::new();
            iter_action_items(&a, &mut items);
            assert_eq!(items.len(), 1, "{key}");
            let ev = match &items[0] {
                ActionItem::Deleted(t, raw) => deleted_to_event("v", t, raw),
                _ => panic!("{key} should be deleted"),
            };
            assert_eq!(ev.kind, ChatKind::Deleted);
            assert_eq!(ev.message, "TGT123");
        }
    }

    /// CH-04: 未知の renderer / アクションは other として保存に回る。
    #[test]
    fn unknown_renderers_become_other() {
        let a = json!({"addChatItemAction": {"item": {"liveChatPollRenderer": {
            "id": "p1", "timestampUsec": "1700000000000002"
        }}}});
        let mut items = Vec::new();
        iter_action_items(&a, &mut items);
        let ev = match &items[0] {
            ActionItem::Item(v) => renderer_to_event("v", v).unwrap(),
            _ => panic!(),
        };
        assert_eq!(ev.kind, ChatKind::Other);

        // addBannerToLiveChatCommand のような未知アクションも Item として拾う
        let b = json!({"addBannerToLiveChatCommand": {"bannerRenderer": {"liveChatBannerRenderer": {}}}});
        let mut items = Vec::new();
        iter_action_items(&b, &mut items);
        assert_eq!(items.len(), 1);
        // {"bannerRenderer": {...}} — bannerRenderer も Renderer 名なので other イベントになる
        let ev = match &items[0] {
            ActionItem::Item(v) => renderer_to_event("v", v),
            _ => panic!(),
        };
        assert_eq!(ev.unwrap().kind, ChatKind::Other);
    }

    /// CH-05: replayChatItemAction は内側のアクションを展開する。
    #[test]
    fn replay_actions_are_flattened() {
        let inner = text_action("r1", "@r", "UCr", "1700000000000003", "replay msg");
        let a = json!({"replayChatItemAction": {"actions": [inner]}});
        let mut items = Vec::new();
        iter_action_items(&a, &mut items);
        assert_eq!(items.len(), 1);
        match &items[0] {
            ActionItem::Item(v) => {
                let ev = renderer_to_event("v", v).unwrap();
                assert_eq!(ev.message, "replay msg");
            }
            _ => panic!(),
        }
    }

    /// CH-06: golden fixture — 実際の get_live_chat 応答（匿名化済み）を
    /// 正規化できる。テキスト 2 件＋削除 1 件、継続トークンを含む。
    #[test]
    fn golden_fixture_normalizes() {
        let v: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/youtube_live_chat.json"))
                .unwrap();
        let lcc = v["continuationContents"]["liveChatContinuation"].clone();
        let (next, timeout) = crate::innertube::next_continuation(&lcc).unwrap();
        assert!(!next.is_empty());
        assert!(timeout > 0);

        let actions = lcc["actions"].as_array().unwrap();
        let mut items = Vec::new();
        for a in actions {
            iter_action_items(a, &mut items);
        }
        let events: Vec<ChatEvent> = items
            .iter()
            .filter_map(|i| match i {
                ActionItem::Item(v) => renderer_to_event("live1", v),
                ActionItem::Deleted(t, raw) => Some(deleted_to_event("live1", t, raw)),
            })
            .collect();
        assert_eq!(events.len(), 3);
        assert!(events
            .iter()
            .any(|e| e.kind == ChatKind::Text && !e.message.is_empty()));
        let del = events.iter().find(|e| e.kind == ChatKind::Deleted).unwrap();
        // 削除対象は InnerTube の item ID 形（"Chw..." 相当）の文字列を指す
        assert!(!del.message.is_empty());
        assert!(del.item_id.starts_with("del:"));
    }

    /// CH-08: dedup は保存確定済み seen と応答内 pending の 2 段。
    /// 保存失敗（pending が捨てられる）後の再送は取り直せる。
    #[test]
    fn dedup_pending_only_commits_on_save() {
        let matcher = Matcher::empty();
        let act = text_action("m1", "@a", "UCa", "1700000000000004", "hi");
        let actions = vec![act.clone()];

        let mut seen = HashSet::new();
        let mut order = VecDeque::new();

        // 1 回目: pending に入ってイベントは返る
        let mut pending = HashSet::new();
        let evs = normalize_all(&matcher, "v", &actions, &seen, &mut pending);
        assert_eq!(evs.len(), 1);
        assert!(pending.contains("m1"));

        // 保存失敗を想定して pending を捨てたまま同じ応答が再送されると、
        // seen に無いので再度取れる（履歴に残る側を優先）
        pending.clear();
        let evs = normalize_all(&matcher, "v", &actions, &seen, &mut pending);
        assert_eq!(evs.len(), 1);

        // 保存成功（commit）後の再送は dedup される
        commit_pending(&mut seen, &mut order, &mut pending);
        let mut pending2 = HashSet::new();
        let evs = normalize_all(&matcher, "v", &actions, &seen, &mut pending2);
        assert!(evs.is_empty());
    }

    /// CH-07: 絵文字 run は絵文字自体またはラベルに変換する。
    #[test]
    fn emoji_run_text() {
        let e = json!({"emojiId": "😀"});
        assert_eq!(emoji_text(&e), "😀");
        let custom = json!({
            "emojiId": "UCabc/def",
            "image": {"accessibility": {"accessibilityData": {"label": ":party:"}}}
        });
        assert_eq!(emoji_text(&custom), ":party:");
        let bare = json!({"emojiId": "UCabc/def"});
        assert_eq!(emoji_text(&bare), "[絵文字]");
    }
}
