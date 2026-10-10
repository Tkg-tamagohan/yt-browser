//! 動画ごとのチャットポーリング。利用者（consumer）管理と、
//! ライブ/リプレイの取得ループを `ChatPoller` にまとめる。

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};

use crate::db::Db;
use crate::filter::NgMatcher;
use crate::innertube::InnerTube;
use crate::model::{ChatEvent, ChatKind, ChatReset, ChatStatus};

use super::normalize::{commit_pending, normalize_all};

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
    /// video_id -> チャット表示の利用者キー集合。埋め込みパネルと
    /// ポップアップ窓を同一動画の利用者として数え、空になった時点で
    /// ポーラーを止める（FR-27、仕様決定 AT。別ウィンドウの利用者は
    /// メイン窓のフロントが数えられないためバックエンドで参照保持する）。
    consumers: Mutex<HashMap<String, HashSet<String>>>,
}

impl ChatPoller {
    pub fn new(db: Db, app: AppHandle, innertube: Arc<InnerTube>, ng: Arc<NgMatcher>) -> Self {
        Self {
            db,
            app,
            innertube,
            ng,
            sessions: Mutex::new(HashMap::new()),
            consumers: Mutex::new(HashMap::new()),
        }
    }

    /// 利用者を 1 件登録して取得を開始する。既に動いていれば利用者の追加だけ行う。
    /// `instance_id` はリプレイの同期先を固定するためのパネル起票インスタンス。
    /// 既に同じ動画のセッションがある場合はそちらの同期先が維持される
    /// （セッションは動画 ID ごとに 1 本。仕様決定 AQ の制約として明記）
    pub fn acquire(self: &Arc<Self>, video_id: &str, consumer: &str, instance_id: Option<u32>) {
        self.consumers
            .lock()
            .unwrap()
            .entry(video_id.to_string())
            .or_default()
            .insert(consumer.to_string());
        self.start(video_id, instance_id);
    }

    /// 利用者を 1 件解除し、最後の利用者なら取得を止める。
    pub fn release(&self, video_id: &str, consumer: &str) {
        // consumers ロックを保持したまま止める。こうすると、解除と並行した
        // acquire が consumers ロック待ちになり、新しい利用者を登録した直後に
        // その新セッションがここの abort に巻き込まれる競合を避けられる
        let mut consumers = self.consumers.lock().unwrap();
        let empty = if let Some(set) = consumers.get_mut(video_id) {
            set.remove(consumer);
            if set.is_empty() {
                consumers.remove(video_id);
                true
            } else {
                false
            }
        } else {
            false
        };
        if empty {
            if let Some(h) = self.sessions.lock().unwrap().remove(video_id) {
                h.abort();
            }
        }
    }

    /// 指定動画のチャット取得を開始する。既に動いていれば何もしない。
    /// `instance_id` はリプレイの同期先を固定するためのパネル起票インスタンス。
    /// 既に同じ動画のセッションがある場合はそちらの同期先が維持される
    /// （セッションは動画 ID ごとに 1 本。仕様決定 AQ の制約として明記）
    fn start(self: &Arc<Self>, video_id: &str, instance_id: Option<u32>) {
        // 利用者登録なしでは動かさない。acquire と stop の競合で
        // 利用者なしのセッションが残らないようにするためのガード
        if !self.consumers.lock().unwrap().contains_key(video_id) {
            return;
        }
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

    /// 全セッションを止めて利用者登録も消す（アプリ終了時）。
    pub fn stop_all(&self) {
        let handles: Vec<_> = self
            .sessions
            .lock()
            .unwrap()
            .drain()
            .map(|(_, h)| h)
            .collect();
        self.consumers.lock().unwrap().clear();
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
    /// されていればその位置を優先し、その窓が無くなった場合は同じ動画を
    /// 再生中のいずれかの位置へ移る（固定先の終了で残り窓のリプレイが
    /// 止まらないようにする）。対象が見つからなければ None
    /// （バッファは進めるが送出しない）。
    fn position_ms(&self, video_id: &str, instance_id: Option<u32>) -> Option<i64> {
        let pm = self.app.try_state::<crate::mpv::PlayerManager>()?;
        let pos = match instance_id {
            Some(id) => pm
                .position_of_instance(id, video_id)
                .or_else(|| pm.position_of(video_id)),
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
        // drain で捨てた区間の末尾オフセット。この時点以前に捨てた
        // 発言があり得るため、その範囲への後方シークでだけ限界警告を出す
        let mut dropped_upto: Option<i64> = None;
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
                // drain で捨てた区間の発言は再送できない。シーク先が
                // 実際に捨てた範囲（dropped_upto 以前）に入るときだけ
                // 巻き戻し限界であることを通知する（保持先頭より前でも
                // 最初の発言に達していないだけなら警告しない）
                if dropped_upto.is_some_and(|b| pos <= b) {
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
            // （シーク・早送り）は近接時点の末尾だけ流して中間を飛ばす。
            // ただし先読みが再生位置に追いついていない（未読の継続が
            // 残り、バッファ末尾が pos より手前の）間は、バッファ内の
            // 遠い過去の発言を送出せずスキップだけする。追いついてから
            // 近接時点の発言を流す
            let target = items.partition_point(|(o, _)| *o <= pos);
            let covered = cont.is_none() || items.last().is_some_and(|(o, _)| *o >= pos);
            if !covered {
                // 追い越し中は遠い過去を送出しないが、現在位置直前の
                // 発言は未送出のまま残す。先読みが追いついた時点で
                // （または継続が尽きた時点で）近接分として送出する。
                // emit_idx は後退させない（送出済みを再送しない）
                emit_idx = emit_idx.max(target.saturating_sub(REPLAY_FLUSH_TAIL));
            } else if target > emit_idx {
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
                dropped_upto = items.get(drop_n - 1).map(|(o, _)| *o);
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
pub(crate) fn with_gen(e: &ChatEvent, gen: u32) -> ChatEvent {
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
pub(crate) enum PremiereCorrection {
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
pub(crate) fn premiere_correction_ms(w: &crate::innertube::BroadcastWindow) -> PremiereCorrection {
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
