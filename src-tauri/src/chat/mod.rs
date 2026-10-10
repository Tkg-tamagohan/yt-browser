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
use tauri::{AppHandle, Emitter, Manager};

use crate::db::Db;
use crate::filter::{Matcher, NgMatcher};
use crate::innertube::InnerTube;
use crate::model::{ChatEvent, ChatKind, ChatReset, ChatStatus};

/// 1 セッションで保持する既処理 item ID の上限（重複除去用）。
/// 超限の古い ID は捨てる（ごく古いアイテムの再送は稀で、
/// 重複 1 件が残るだけの影響で上限を超えてメモリを増やし続ける方が悪い）。
const SEEN_CAP: usize = 10_000;
/// 連続失敗がこの回数を超えたら `chat://status` に warn を出す。
const WARN_AFTER_FAILURES: u32 = 2;
/// ポーリング間隔の最小値（応答の timeoutMs が 0/欠落でも過剰に回さない）。
const MIN_POLL_MS: u64 = 300;
/// リプレイの送出ティック（再生位置との照合周期）。
const REPLAY_TICK_MS: u64 = 250;
/// リプレイの先読み幅（再生位置 + この先までバッファする）。
const REPLAY_LOOKAHEAD_MS: i64 = 120_000;
/// 1 ティックに流すメッセージの上限（通常再生の追従分には十分な幅）。
const REPLAY_BATCH_MAX: usize = 200;
/// 前方シーク等で一気に追い越したとき、近接時点の末尾だけ流す件数。
const REPLAY_FLUSH_TAIL: usize = 50;
/// 再生位置がこの幅より戻ったらシークとみなし再アンカーする。
const REPLAY_SEEK_BACK_MS: i64 = 2_000;
/// リプレイ取得の連続失敗上限。リプレイ未処理・無効な継続など
/// 恒久的な失敗で無限にポーリングしないための上限。
const REPLAY_MAX_FAILURES: u32 = 8;
/// リプレイの送出済み保持数（後方シークの巻き戻し窓）。超過分は
/// 古い順に捨て、長時間配信で items と raw_json が肥大しないようにする。
/// 捨てた区間への後方シークではその区間のチャットは再送されない
/// （設計書 §6.2 の制約として明記）
const REPLAY_REWIND_KEEP: usize = 1_000;
/// リプレイの未送出先読み保持数。高頻度チャットでは LOOKAHEAD 窓だけでは
/// 収まらないため、超過時は送出が進むまで追加取得を休止する上限
const REPLAY_BUF_MAX: usize = 2_000;

/// 動画ごとのチャットポーリングを管理する。`tauri::State` に `Arc` で載せる。
pub struct ChatPoller {
    db: Db,
    app: AppHandle,
    innertube: Arc<InnerTube>,
    /// 共有の NG 評価器。所有者は `filter::NgMatcher`（chat 以外の
    /// コマンドも使うためここでは参照だけ持つ）。
    ng: Arc<NgMatcher>,
    /// video_id -> 実行中タスクの JoinHandle（停止は `abort()`）。
    sessions: Mutex<HashMap<String, tauri::async_runtime::JoinHandle<()>>>,
}

impl ChatPoller {
    pub fn new(db: Db, app: AppHandle, innertube: Arc<InnerTube>, ng: Arc<NgMatcher>) -> Self {
        Self {
            db,
            app,
            innertube,
            ng,
            sessions: Mutex::new(HashMap::new()),
        }
    }

    /// 指定動画のチャット取得を開始する。既に動いていれば何もしない。
    /// `instance_id` はリプレイの同期先を固定するためのパネル起票インスタンス。
    /// 既に同じ動画のセッションがある場合はそちらの同期先が維持される
    /// （セッションは動画 ID ごとに 1 本。仕様決定 AQ の制約として明記）
    pub fn start(self: &Arc<Self>, video_id: &str, instance_id: Option<u32>) {
        let mut sessions = self.sessions.lock().unwrap();
        if sessions.contains_key(video_id) {
            return;
        }
        let poller = Arc::clone(self);
        let vid = video_id.to_string();
        // chat_start は同期コマンド（ランタイムコンテキスト外）から呼ばれるため
        // tokio::spawn ではなく Tauri のランタイムに乗せる
        let handle =
            tauri::async_runtime::spawn(async move { poller.run(&vid, instance_id).await });
        sessions.insert(video_id.to_string(), handle);
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
    /// 初期継続トークンの種別でライブ/リプレイを自動判定する（仕様決定 AQ）。
    async fn run(self: &Arc<Self>, video_id: &str, instance_id: Option<u32>) {
        let html = match self.innertube.watch_html(video_id).await {
            Ok(html) => html,
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
        // 放送窓はリプレイ経路の判定（is_ended）とプレミア補正の両方で使う
        let window = crate::innertube::extract_broadcast_window(&html);
        match crate::innertube::extract_initial_continuation(&html) {
            Some((cont, crate::innertube::ContinuationKind::Live)) if !window.is_ended() => {
                self.run_live(video_id, cont).await;
            }
            Some((cont, _)) => {
                // 初段がリプレイ継続、または放送窓から終了済みと判定できる
                // （watch の `reloadContinuationData` はライブ系キーだが、
                // 終了済みでは `get_live_chat_replay` へ投げるとリプレイが
                // 返る。実機確認 2026-10、仕様決定 AQ）
                let correction = match premiere_correction_ms(&window) {
                    PremiereCorrection::Apply(ms) => {
                        self.status(
                            Some(video_id),
                            "info",
                            &format!(
                                "チャットリプレイを開始します（プレミアずれ {:.1} 秒を補正）",
                                ms as f64 / 1000.0
                            ),
                        );
                        ms
                    }
                    PremiereCorrection::Unestimable => {
                        self.status(
                            Some(video_id),
                            "warn",
                            "プレミアずれの補正量を推定できないため、未補正でリプレイします",
                        );
                        0
                    }
                    PremiereCorrection::None => 0,
                };
                self.run_replay(video_id, cont, correction, instance_id)
                    .await;
            }
            None => {
                self.status(
                    Some(video_id),
                    "info",
                    "チャットが見つかりません（ライブ配信以外か、チャットが無効です）",
                );
            }
        }
        self.sessions.lock().unwrap().remove(video_id);
    }

    /// リプレイ同期用の再生位置（ms）。パネル起票のインスタンスが指定
    /// されていればその位置、無ければ同じ動画を再生中のいずれかの位置。
    /// 対象が見つからなければ None（バッファは進めるが送出しない）。
    fn position_ms(&self, video_id: &str, instance_id: Option<u32>) -> Option<i64> {
        let pm = self.app.try_state::<crate::mpv::PlayerManager>()?;
        let pos = match instance_id {
            Some(id) => pm.position_of_instance(id, video_id),
            None => pm.position_of(video_id),
        };
        pos.map(|p| (p * 1000.0) as i64)
    }

    /// ライブ配信のポーリングループ（FR-6）。
    /// 応答のアクションを保存したうえで `chat://message` に流し、
    /// timeoutMs 間隔で継続トークンを辿る。
    async fn run_live(self: &Arc<Self>, video_id: &str, mut cont: String) {
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
                    let matcher = self.ng.get();
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
                        Some((next, timeout, _)) => {
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
    }

    /// 終了済み配信のリプレイ（FR-24、仕様決定 AQ）。
    /// 継続トークンを先読みしてオフセット付きバッファを作り、mpv の再生位置を
    /// 照合して `chat://message` へ流す。リプレイ分は `chat_logs` に保存しない。
    /// シーク（位置の後退・大きな前進）は近接時点へ再アンカーする。
    /// 末尾を通過してもポーラーは維持し、後方シークでの再送に備える
    /// （停止はパネル閉・プレイヤー終了時の `chat_stop` が担う）。
    /// `instance_id` 指定時はそのインスタンスの位置のみで同期する
    /// （同一動画の複数窓で位置がずれないよう固定。仕様決定 AQ）
    async fn run_replay(
        self: &Arc<Self>,
        video_id: &str,
        cont: String,
        correction_ms: i64,
        instance_id: Option<u32>,
    ) {
        let mut cont = Some(cont);
        // オフセット昇順のバッファ。`emit_idx` は未送出の先頭。
        let mut items: Vec<(i64, ChatEvent)> = Vec::new();
        let mut emit_idx = 0usize;
        let mut seen: HashSet<String> = HashSet::new();
        let mut order: VecDeque<String> = VecDeque::new();
        // 後方シークで再アンカーした世代。世代毎に item_id へ接尾辞を付け、
        // 表示側の dedup を避けて同じメッセージを再度流せるようにする。
        let mut gen = 0u32;
        let mut last_pos: Option<i64> = None;
        let mut failures: u32 = 0;
        // 末尾通過の終了通知は 1 回だけ出す
        let mut end_notified = false;
        loop {
            let pos = self
                .position_ms(video_id, instance_id)
                .or(last_pos)
                .unwrap_or(0);
            // 位置 + LOOKAHEAD までバッファが無ければ継続を先読みする
            while let Some(c) = cont.clone() {
                // 未送出の先読みが上限なら、送出が進むまで追加取得を休む。
                // 高頻度チャットの LOOKAHEAD 窓でも items が肥大しないようにする
                if items.len() - emit_idx >= REPLAY_BUF_MAX {
                    break;
                }
                let tail = items.last().map(|(o, _)| *o).unwrap_or(i64::MIN);
                if tail >= pos + REPLAY_LOOKAHEAD_MS {
                    break;
                }
                match self.innertube.get_live_chat_replay(&c).await {
                    Ok(v) => {
                        if failures >= WARN_AFTER_FAILURES {
                            self.status(Some(video_id), "info", "チャット取得が復帰しました");
                        }
                        failures = 0;
                        let lcc = v
                            .get("continuationContents")
                            .and_then(|x| x.get("liveChatContinuation"));
                        let matcher = self.ng.get();
                        let mut pending: HashSet<String> = HashSet::new();
                        let events = lcc
                            .and_then(|l| l.get("actions"))
                            .and_then(|a| a.as_array())
                            .map(|acts| {
                                normalize_all(&matcher, video_id, acts, &seen, &mut pending)
                            })
                            .unwrap_or_default();
                        commit_pending(&mut seen, &mut order, &mut pending);
                        let mut appended = false;
                        for mut e in events {
                            // リプレイ分は chat_logs に保存しないため生 JSON は
                            // 保持しない（長時間配信でのメモリ肥大対策）
                            e.raw_json = String::new();
                            // 補正後の動画内時刻。オフセットを持たない項目は 0 扱い
                            let off = e.video_offset_ms.unwrap_or(0) - correction_ms;
                            items.push((off, e));
                            appended = true;
                        }
                        if appended && items.len() > 1 {
                            items.sort_unstable_by_key(|(o, _)| *o);
                        }
                        // リプレイ経路では `liveChatReplayContinuationData` のみを
                        // 辿る。併存する `playerSeekContinuationData`（シーク用）は
                        // バッファ方式では使わない（仕様決定 AQ）
                        cont = lcc
                            .and_then(crate::innertube::next_continuation)
                            .filter(|(_, _, k)| *k == crate::innertube::ContinuationKind::Replay)
                            .map(|(next, _, _)| next);
                        if cont.is_none() {
                            break;
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
                        if failures >= REPLAY_MAX_FAILURES {
                            self.status(
                                Some(video_id),
                                "error",
                                "チャットリプレイを取得できませんでした（リプレイ未公開の可能性があります）",
                            );
                            return;
                        }
                        let backoff = (1u64 << failures.min(5)).min(30);
                        tokio::time::sleep(Duration::from_secs(backoff)).await;
                        break;
                    }
                }
            }
            // 後方シークの検知 → 未送出カーソルを近接時点へ戻して再アンカー。
            // 世代を進めるので、その時点より先の既送出分は新しい接尾辞で
            // 再度流れ、表示側では新しい行として再生される
            if last_pos.is_some_and(|lp| pos < lp - REPLAY_SEEK_BACK_MS) {
                emit_idx = items.partition_point(|(o, _)| *o <= pos);
                gen += 1;
                // 保持窓から捨てた区間は再送できない。シーク先が保持分の
                // 先頭より前なら巻き戻し限界であることを通知する
                if items.first().is_some_and(|(o, _)| *o > pos) {
                    self.status(
                        Some(video_id),
                        "warn",
                        "巻き戻し可能な範囲を超えたため、それより前のチャットは再表示されません",
                    );
                }
                // パネルの既表示行を消してから再送する
                // （シーク先より未来の発言が残り続けないよう）
                let _ = self.app.emit(
                    "chat://reset",
                    ChatReset {
                        video_id: video_id.to_string(),
                    },
                );
            }
            // 再生位置までの未送出分を流す。前方への大きな追い越し
            // （シーク・早送り）は近接時点の末尾だけ流して中間を飛ばす
            let target = items.partition_point(|(o, _)| *o <= pos);
            if target > emit_idx {
                let start = if target - emit_idx > REPLAY_BATCH_MAX {
                    target.saturating_sub(REPLAY_FLUSH_TAIL)
                } else {
                    emit_idx
                };
                let batch: Vec<ChatEvent> = items[start..target]
                    .iter()
                    .map(|(_, e)| with_gen(e, gen))
                    .collect();
                if !batch.is_empty() {
                    let _ = self.app.emit("chat://message", &batch);
                }
                emit_idx = target;
            }
            last_pos = Some(pos);
            if cont.is_none() && emit_idx >= items.len() && !end_notified {
                self.status(Some(video_id), "info", "チャットリプレイが終了しました");
                end_notified = true;
            }
            // 送出済みの保持は巻き戻し窓に限る。それより古い分は捨てて
            // 長時間配信でメモリが増え続けないようにする（捨てた区間への
            // 後方シークではその区間のチャットは再送されない）
            if emit_idx > REPLAY_REWIND_KEEP {
                let drop_n = emit_idx - REPLAY_REWIND_KEEP;
                items.drain(..drop_n);
                emit_idx -= drop_n;
            }
            tokio::time::sleep(Duration::from_millis(REPLAY_TICK_MS)).await;
        }
    }
}

/// 再アンカー世代の接尾辞を付けたイベント複製。
/// `gen=0`（初回通過）はそのまま返す。削除イベントは `message` の
/// 対象 ID にも同じ接尾辞を付け、再表示分の行と対応付ける。
fn with_gen(e: &ChatEvent, gen: u32) -> ChatEvent {
    if gen == 0 {
        return e.clone();
    }
    let mut out = e.clone();
    out.item_id = format!("{}#g{gen}", out.item_id);
    if out.kind == ChatKind::Deleted {
        out.message = format!("{}#g{gen}", out.message);
    }
    out
}

/// プレミアずれの補正量の推定結果（仕様決定 AQ の暫定方式）。
enum PremiereCorrection {
    /// この補正量（ms）を videoOffsetTimeMsec から引く。
    Apply(i64),
    /// 補正不要（差が小さい、または放送が動画より短い）。
    None,
    /// 推定に必要な情報が無い、または差が大きすぎて前置きとは見做せない。
    Unestimable,
}

/// 放送窓と動画長からプレミアずれの補正量（ms）を推定する（暫定方式）。
/// 放送時間が動画長より長い分を前置き（カウントダウン等）とみなし、
/// 30 秒超・6 時間以内のときだけ補正に使う。通常のライブアーカイブは
/// 放送時間 ≒ 動画長なので自然に補正不要になる。
fn premiere_correction_ms(w: &crate::innertube::BroadcastWindow) -> PremiereCorrection {
    let (Some(start), Some(end), Some(len_secs)) = (w.start_ms, w.end_ms, w.length_secs) else {
        return PremiereCorrection::Unestimable;
    };
    let diff = end - start - len_secs * 1000;
    if diff > 30_000 && diff <= 6 * 3_600_000 {
        PremiereCorrection::Apply(diff)
    } else if diff > 6 * 3_600_000 {
        PremiereCorrection::Unestimable
    } else {
        PremiereCorrection::None
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
        iter_action_items(a, None, &mut items);
    }
    let mut out = Vec::new();
    for item in items {
        let Some(mut e) = (match item {
            ActionItem::Item { v, offset_ms } => renderer_to_event(video_id, &v, offset_ms),
            ActionItem::Deleted {
                target,
                raw,
                offset_ms,
            } => Some(deleted_to_event(video_id, &target, &raw, offset_ms)),
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
/// `offset_ms` はリプレイの動画内時刻（`videoOffsetTimeMsec`）、ライブでは None。
enum ActionItem {
    Item {
        v: Value,
        offset_ms: Option<i64>,
    },
    Deleted {
        target: String,
        raw: Value,
        offset_ms: Option<i64>,
    },
}

/// 1 アクションオブジェクトから処理対象を全て列挙する。
/// `replayChatItemAction`（リプレイ由来）は内側の actions を再帰的に展開し、
/// アクションレベルの `videoOffsetTimeMsec` を内側アイテムへ伝播する。
/// 未知のアクションキーは `other` として保存するため Item に流す。
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
                        raw: v.clone(),
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
/// 未知 renderer は `other` として原文だけ残す。
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
            raw_json: serde_json::to_string(item).unwrap_or_default(),
        });
    }
    None
}

/// 削除アクションを `deleted` イベントとして正規化する。
/// `message` に削除対象の item ID を入れ、UI 側で該当行の打消しに使う。
fn deleted_to_event(
    video_id: &str,
    target_id: &str,
    raw: &Value,
    offset_ms: Option<i64>,
) -> ChatEvent {
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
                ActionItem::Deleted {
                    target,
                    raw,
                    offset_ms,
                } => deleted_to_event("v", target, raw, *offset_ms),
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

        let del = deleted_to_event("v", "x1", &serde_json::json!({}), Some(500));
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
                ActionItem::Deleted {
                    target,
                    raw,
                    offset_ms,
                } => Some(deleted_to_event("live1", target, raw, *offset_ms)),
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
