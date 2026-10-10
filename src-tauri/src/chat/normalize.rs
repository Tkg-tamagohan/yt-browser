//! `get_live_chat` 応答の actions[] を `ChatEvent` へ正規化し、
//! 重複除去と NG 判定を行う（設計書 §6.3、FR-9）。

use std::collections::{HashSet, VecDeque};

use serde_json::Value;

use crate::filter::Matcher;
use crate::model::{ChatEvent, ChatKind};

/// 1 セッションで保持する既処理 item ID の上限（重複除去用）。
/// 超限の古い ID は捨てる（ごく古いアイテムの再送は稀で、
/// 重複 1 件が残るだけの影響で上限を超えてメモリを増やし続ける方が悪い）。
const SEEN_CAP: usize = 10_000;

/// 応答の actions[] 全件を正規化し、重複を除き、NG 判定を付けて返す。
/// `seen` は確定済みの既処理 ID、`pending` はこの応答で処理した ID
/// （呼び出し側が `commit_pending` で seen へ移す）。
pub(crate) fn normalize_all(
    matcher: &Matcher,
    video_id: &str,
    actions: &[Value],
    seen: &HashSet<String>,
    pending: &mut HashSet<String>,
) -> Vec<ChatEvent> {
    let mut items = Vec::new();
    for a in actions {
        iter_action_items(a, None, &mut items);
    }
    let mut out = Vec::new();
    for item in items {
        let Some(mut e) = (match item {
            ActionItem::Item { v, offset_ms } => renderer_to_event(video_id, &v, offset_ms),
            ActionItem::Deleted { target, offset_ms } => {
                Some(deleted_to_event(video_id, &target, offset_ms))
            }
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

/// 処理したバッチの item ID を既処理集合へ確定し、上限を超えたら古い順に捨てる。
pub(crate) fn commit_pending(
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
/// `{<name>Renderer: {...}}` 形の値オブジェクト、`Deleted` は削除対象 ID。
/// `offset_ms` はリプレイの動画内時刻（`videoOffsetTimeMsec`）、ライブでは None。
enum ActionItem {
    Item {
        v: Value,
        offset_ms: Option<i64>,
    },
    Deleted {
        target: String,
        offset_ms: Option<i64>,
    },
}

/// 1 アクションオブジェクトから処理対象を全て列挙する。
/// `replayChatItemAction`（リプレイ由来）は内側の actions を再帰的に展開し、
/// アクションレベルの `videoOffsetTimeMsec` を内側アイテムへ伝播する。
/// 未知のアクションキーは `other` イベントにするため Item に流す。
fn iter_action_items(action: &Value, offset_ms: Option<i64>, out: &mut Vec<ActionItem>) {
    let Some(m) = action.as_object() else {
        return;
    };
    for (k, v) in m {
        match k.as_str() {
            "addChatItemAction" => {
                if let Some(item) = v.get("item") {
                    out.push(ActionItem::Item {
                        v: item.clone(),
                        offset_ms,
                    });
                }
            }
            "replayChatItemAction" => {
                // オフセットは replayChatItemAction レベルに付く。ネストした
                // 内側の値を優先し、無ければ外側を引き継ぐ
                let inner = v
                    .get("videoOffsetTimeMsec")
                    .and_then(|t| t.as_str())
                    .and_then(|s| s.parse::<i64>().ok())
                    .or(offset_ms);
                if let Some(acts) = v.get("actions").and_then(|a| a.as_array()) {
                    for a in acts {
                        iter_action_items(a, inner, out);
                    }
                }
            }
            "removeChatItemAction" | "markChatItemAsDeletedAction" => {
                if let Some(id) = v.get("targetItemId").and_then(|t| t.as_str()) {
                    out.push(ActionItem::Deleted {
                        target: id.to_string(),
                        offset_ms,
                    });
                }
            }
            "clickTrackingParams" => {}
            _ => out.push(ActionItem::Item {
                v: v.clone(),
                offset_ms,
            }),
        }
    }
}

/// `{<rendererName>: {...}}` 形のアイテムを `ChatEvent` に正規化する。
/// `offset_ms` はリプレイの動画内時刻（ライブでは None）。
/// 未知 renderer は `other` イベントにする。
fn renderer_to_event(video_id: &str, item: &Value, offset_ms: Option<i64>) -> Option<ChatEvent> {
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
            video_offset_ms: offset_ms,
        });
    }
    None
}

/// 削除アクションを `deleted` イベントとして正規化する。
/// `message` に削除対象の item ID を入れ、UI 側で該当行の打消しに使う。
fn deleted_to_event(video_id: &str, target_id: &str, offset_ms: Option<i64>) -> ChatEvent {
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
        video_offset_ms: offset_ms,
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
    use crate::chat::poller::{premiere_correction_ms, with_gen, PremiereCorrection};
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
        iter_action_items(&a, None, &mut items);
        assert_eq!(items.len(), 1);
        let ev = match &items[0] {
            ActionItem::Item { v, offset_ms } => {
                renderer_to_event("vid123", v, *offset_ms).unwrap()
            }
            _ => panic!("expected item"),
        };
        assert_eq!(ev.kind, ChatKind::Text);
        assert_eq!(ev.item_id, "id1");
        assert_eq!(ev.author_name.as_deref(), Some("@alice"));
        assert_eq!(ev.author_channel_id.as_deref(), Some("UCxxxx"));
        assert_eq!(ev.posted_at_usec, 1700000000000000);
        assert_eq!(ev.message, "hello");
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
        iter_action_items(&a, None, &mut items);
        let ev = match &items[0] {
            ActionItem::Item { v, offset_ms } => renderer_to_event("v", v, *offset_ms).unwrap(),
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
            iter_action_items(&a, None, &mut items);
            assert_eq!(items.len(), 1, "{key}");
            let ev = match &items[0] {
                ActionItem::Deleted { target, offset_ms } => {
                    deleted_to_event("v", target, *offset_ms)
                }
                _ => panic!("{key} should be deleted"),
            };
            assert_eq!(ev.kind, ChatKind::Deleted);
            assert_eq!(ev.message, "TGT123");
        }
    }

    /// CH-04: 未知の renderer / アクションは other イベントになる。
    #[test]
    fn unknown_renderers_become_other() {
        let a = json!({"addChatItemAction": {"item": {"liveChatPollRenderer": {
            "id": "p1", "timestampUsec": "1700000000000002"
        }}}});
        let mut items = Vec::new();
        iter_action_items(&a, None, &mut items);
        let ev = match &items[0] {
            ActionItem::Item { v, offset_ms } => renderer_to_event("v", v, *offset_ms).unwrap(),
            _ => panic!(),
        };
        assert_eq!(ev.kind, ChatKind::Other);

        // addBannerToLiveChatCommand のような未知アクションも Item として拾う
        let b = json!({"addBannerToLiveChatCommand": {"bannerRenderer": {"liveChatBannerRenderer": {}}}});
        let mut items = Vec::new();
        iter_action_items(&b, None, &mut items);
        assert_eq!(items.len(), 1);
        // {"bannerRenderer": {...}} — bannerRenderer も Renderer 名なので other イベントになる
        let ev = match &items[0] {
            ActionItem::Item { v, offset_ms } => renderer_to_event("v", v, *offset_ms),
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
        iter_action_items(&a, None, &mut items);
        assert_eq!(items.len(), 1);
        match &items[0] {
            ActionItem::Item { v, offset_ms } => {
                let ev = renderer_to_event("v", v, *offset_ms).unwrap();
                assert_eq!(ev.message, "replay msg");
            }
            _ => panic!(),
        }
    }

    /// FR-24: replayChatItemAction の videoOffsetTimeMsec が
    /// 内側のアイテムへ伝播して video_offset_ms に乗る（AQ）。
    /// 削除アクションも同じオフセットを持つ。
    #[test]
    fn replay_offset_propagates_to_items() {
        let inner = text_action("r1", "@r", "UCr", "1700000000000003", "at 10s");
        let a = json!({"replayChatItemAction": {
            "actions": [inner],
            "videoOffsetTimeMsec": "10000"
        }});
        let mut items = Vec::new();
        iter_action_items(&a, None, &mut items);
        assert_eq!(items.len(), 1);
        match &items[0] {
            ActionItem::Item { v, offset_ms } => {
                assert_eq!(*offset_ms, Some(10_000));
                let ev = renderer_to_event("v", v, *offset_ms).unwrap();
                assert_eq!(ev.video_offset_ms, Some(10_000));
            }
            _ => panic!(),
        }

        let del = json!({"replayChatItemAction": {
            "actions": [{"removeChatItemAction": {"targetItemId": "T1"}}],
            "videoOffsetTimeMsec": "2500"
        }});
        let mut items = Vec::new();
        iter_action_items(&del, None, &mut items);
        match &items[0] {
            ActionItem::Deleted {
                target, offset_ms, ..
            } => {
                assert_eq!(target, "T1");
                assert_eq!(*offset_ms, Some(2_500));
            }
            _ => panic!(),
        }

        // ライブのトップレベルアクションはオフセットを持たない
        let live = text_action("l1", "@a", "UCa", "1700000000000004", "live");
        let mut items = Vec::new();
        iter_action_items(&live, None, &mut items);
        match &items[0] {
            ActionItem::Item { offset_ms, .. } => assert_eq!(*offset_ms, None),
            _ => panic!(),
        }
    }

    /// FR-24: 再アンカー世代の接尾辞。gen=0 は不変、gen>0 は item_id に
    /// 接尾辞を付け、削除イベントは対象 ID にも付ける。
    #[test]
    fn with_gen_suffixes_for_reanchor() {
        let a = text_action("x1", "@r", "UCr", "1700000000000005", "m");
        let mut items = Vec::new();
        iter_action_items(&a, Some(500), &mut items);
        let ev = match &items[0] {
            ActionItem::Item { v, offset_ms } => renderer_to_event("v", v, *offset_ms).unwrap(),
            _ => panic!(),
        };
        let same = with_gen(&ev, 0);
        assert_eq!(same.item_id, "x1");
        let re = with_gen(&ev, 3);
        assert_eq!(re.item_id, "x1#g3");

        let del = deleted_to_event("v", "x1", Some(500));
        let re = with_gen(&del, 3);
        assert_eq!(re.item_id, "del:x1#g3");
        assert_eq!(re.message, "x1#g3");
    }

    /// FR-24: プレミアずれ補正量の推定（暫定方式）。
    /// 放送窓 − 動画長が前置き分。30 秒超〜6 時間以内だけ適用。
    #[test]
    fn premiere_correction_estimates() {
        use crate::innertube::BroadcastWindow;
        let w = |start_ms: i64, end_ms: i64, len_secs: i64| BroadcastWindow {
            start_ms: Some(start_ms),
            end_ms: Some(end_ms),
            length_secs: Some(len_secs),
            is_live_now: Some(false),
        };
        // 前置き 5 分のプレミア: 300 秒補正
        assert!(matches!(
            premiere_correction_ms(&w(0, 300_000 + 600_000, 600)),
            PremiereCorrection::Apply(300_000)
        ));
        // 通常アーカイブ（放送≒動画長、誤差 5 秒）: 補正なし
        assert!(matches!(
            premiere_correction_ms(&w(0, 605_000, 600)),
            PremiereCorrection::None
        ));
        // アーカイブの方が長い（トリミング等）: 補正なし
        assert!(matches!(
            premiere_correction_ms(&w(0, 500_000, 600)),
            PremiereCorrection::None
        ));
        // 前置き 6 時間超は前置きと見做せない: 推定不能
        assert!(matches!(
            premiere_correction_ms(&w(0, 7 * 3_600_000 + 600_000, 600)),
            PremiereCorrection::Unestimable
        ));
        // 情報欠落: 推定不能
        assert!(matches!(
            premiere_correction_ms(&BroadcastWindow::default()),
            PremiereCorrection::Unestimable
        ));
    }

    /// CH-06: golden fixture — 実際の get_live_chat 応答（匿名化済み）を
    /// 正規化できる。テキスト 2 件＋削除 1 件、継続トークンを含む。
    #[test]
    fn golden_fixture_normalizes() {
        let v: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/youtube_live_chat.json"))
                .unwrap();
        let lcc = v["continuationContents"]["liveChatContinuation"].clone();
        let (next, timeout, _) = crate::innertube::next_continuation(&lcc).unwrap();
        assert!(!next.is_empty());
        assert!(timeout > 0);

        let actions = lcc["actions"].as_array().unwrap();
        let mut items = Vec::new();
        for a in actions {
            iter_action_items(a, None, &mut items);
        }
        let events: Vec<ChatEvent> = items
            .iter()
            .filter_map(|i| match i {
                ActionItem::Item { v, offset_ms } => renderer_to_event("live1", v, *offset_ms),
                ActionItem::Deleted { target, offset_ms } => {
                    Some(deleted_to_event("live1", target, *offset_ms))
                }
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

    /// CH-08: dedup は確定済み seen と応答内 pending の 2 段。
    /// 未確定（pending が捨てられた）バッチの再送は取り直せる。
    #[test]
    fn dedup_pending_and_seen_two_tier() {
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

        // pending を捨てたまま同じ応答が再送されると、
        // seen に無いので再度取れる
        pending.clear();
        let evs = normalize_all(&matcher, "v", &actions, &seen, &mut pending);
        assert_eq!(evs.len(), 1);

        // commit 後の再送は dedup される
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
