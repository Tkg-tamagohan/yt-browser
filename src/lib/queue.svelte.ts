//! 連続再生キューとループ（FR-10、仕様決定 S・AA・AD）。
//! キューはフロント側のセッション状態として持つ。今後の項目は
//! `player_set_queue` で順序列入れてバックエンドへ事前登録（武装）し、
//! mpv の終端イベントで同一インスタンスが読み替える。
//! 遷移のたびにフロントへ再登録を要求しない（再登録が終端に間に合わず
//! 途切れる競合への対策、仕様決定 AD）。登録済みプレイリストと実際に流れた
//! 項目がずれる（queue drift）のは仕様上の制約として許容する。
//! ループ状態もインスタンス別にフロントが持ち、武装対象の選択で実現する
//! （1 項目は現在項目のみ、全体は末尾到達で先頭へ戻る回転順、キュー無しは
//! 現在項目を巡回登録）。

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  playerStates,
  type PlayerEnded,
  type VideoRef,
} from "$lib/players.svelte";
import {
  armPlanFor,
  indexAfterMove,
  indexAfterRemove,
  reconcileIndex,
} from "./queue-logic";

/// 稼働中キューの状態。`items` は動画 ID 列、`index` は現在の再生位置。
/// `instanceId` はキューを背負うプレイヤーインスタンス。
/// `playlistId === null` は一時キュー（キュースタック、FR-20・仕様決定 AM）。
/// 未実行で積んでいるだけのときは instanceId も null
export const queue = $state<{
  playlistId: number | null;
  playlistName: string;
  items: string[];
  index: number;
  instanceId: number | null;
}>({ playlistId: null, playlistName: "", items: [], index: 0, instanceId: null });

/// キュー項目の表示メタ（FR-20、仕様決定 AM）。セッション内メモリのみで
/// DB には保存しない。同一動画の重複登録は同じメタを共有する
export type QueueItemMeta = {
  title: string;
  channelTitle: string | null;
  thumbnailUrl: string | null;
};
export const queueMeta = $state<{ list: Map<string, QueueItemMeta> }>({
  list: new Map(),
});

/// キューパネルの開閉。ナビ入口とキュー実行中カードの両方から共有する
export const queuePanel = $state({ open: false });

/// メタを記憶する。追加経路ごとに呼び、重複登録は先のメタを維持する
function rememberMeta(v: VideoRef): void {
  if (queueMeta.list.has(v.videoId)) return;
  const next = new Map(queueMeta.list);
  next.set(v.videoId, {
    title: v.title,
    channelTitle: v.channelTitle,
    thumbnailUrl: v.thumbnailUrl,
  });
  queueMeta.list = next;
}

/// キュー項目の表示メタを引く。未登録は videoId をタイトル代わりに返す
export function queueMetaOf(videoId: string): QueueItemMeta {
  return (
    queueMeta.list.get(videoId) ?? {
      title: videoId,
      channelTitle: null,
      thumbnailUrl: null,
    }
  );
}

/// ループ状態（仕様決定 AA）。「なし」「プレイリスト全体」「1 項目」の 3 状態。
export type LoopMode = "none" | "all" | "one";

/// インスタンス別のループ状態。未登録は "none" とみなす。
/// セッション内のみ有効（キューと同じく永続化しない）
export const loopModes = $state<{ list: Map<number, LoopMode> }>({
  list: new Map(),
});

/// インスタンスの現在のループ状態。未登録は "none"。
export function loopMode(instanceId: number): LoopMode {
  return loopModes.list.get(instanceId) ?? "none";
}

function setLoopMode(instanceId: number, mode: LoopMode): void {
  const next = new Map(loopModes.list);
  if (mode === "none") next.delete(instanceId);
  else next.set(instanceId, mode);
  loopModes.list = next;
}

/// インスタンス終了・クローズ時にループ状態を外す。
export function clearLoop(instanceId: number): void {
  if (!loopModes.list.has(instanceId)) return;
  setLoopMode(instanceId, "none");
}

// `player_set_queue` の直列化（インスタンスごと）。
// invoke の到着順は保証されないため、武装・解除・再武装を一本の
// Promise チェーンに載せ、各操作は実行時点で対象を評価する。
// これにより最後に予約した意図だけが必ず backend へ届く。
const armRuns = new Map<number, Promise<void>>();

// `player://ended` で観測した取り出し世代（インスタンスごと）。
// 武装要求はこの値を `baseSeq` に載せ、世代がずれていれば backend が
// 拒否する（古いイベントに基づく置換で消費済み項目が復活しないため）
const armedSeqs = new Map<number, number>();

// 世代ずれで backend に拒否されたまま未反映の意図を持つインスタンス。
// 次の ended イベント（最新世代）で再適用する。
const rearmPending = new Set<number>();

/// インスタンスへの `player_set_queue` を直列化して送る。
/// `plan` は実行時点で評価し、送信順序をチェーンで固定する。
function enqueueArm(
  instanceId: number,
  plan: () => { items: string[]; loop: boolean },
): Promise<void> {
  const prev = armRuns.get(instanceId) ?? Promise.resolve();
  const run = prev.then(() => {
    const p = plan();
    // コマンド引数は items / loopAll / baseSeq（snake_case の camelCase）。
    // 計画側のキー名（loop）と混ざらないよう明示的に渡す。
    // baseSeq は計画を評価した時点の観測世代に束ねる
    const sentSeq = armedSeqs.get(instanceId) ?? 0;
    return invoke<boolean>("player_set_queue", {
      instanceId,
      items: p.items,
      loopAll: p.loop,
      baseSeq: sentSeq,
    }).then((applied) => {
      if (applied) {
        rearmPending.delete(instanceId);
        return;
      }
      // 世代ずれで拒否された意図は保留して再適用する。
      // 拒否応答が世代を進めた終端イベントより遅れることがあるため、
      // 観測世代が送信時より新しければ次イベントを待たず即時に
      // 最新意図で再送する（武装が尽きると意図が失われるため）。
      // 未着なら次の ended イベント（最新世代）で再適用する
      rearmPending.add(instanceId);
      if ((armedSeqs.get(instanceId) ?? 0) > sentSeq) {
        void enqueueArm(instanceId, () => latestPlan(instanceId)).catch(
          () => {},
        );
      }
    });
  });
  // 後続のチェーンは失敗に関わらず進める（失敗処理は呼び出し側の catch）
  const stored = run.catch(() => {});
  armRuns.set(instanceId, stored);
  // 収束したチェーンのエントリを掃除する。より新しいチェーンが登録
  // 済みなら残す（閉じたインスタンスのエントリが溜まらないように）
  void stored.then(() => {
    if (armRuns.get(instanceId) === stored) armRuns.delete(instanceId);
  });
  return run;
}

/// 実行時点の最新意図から武装計画を作る（世代ずれ拒否後の即時再適用用）。
/// キュー中は armPlanFor、キュー外はループモードに応じた単独項目または解除
function latestPlan(instanceId: number): { items: string[]; loop: boolean } {
  if (instanceId === queue.instanceId) {
    return armPlanFor(loopMode(instanceId), queue.items, queue.index);
  }
  if (loopMode(instanceId) === "none") return { items: [], loop: false };
  const vid = playerStates.list.get(instanceId)?.videoId;
  return { items: vid ? [vid] : [], loop: true };
}

/// ループ状態を なし → 全体 → 1 項目 の順に切り替える（仕様決定 AA）。
/// 切り替え時点でそのインスタンスの武装を新しいモードの対象へ張り替える。
export async function cycleLoop(instanceId: number): Promise<void> {
  const cur = loopMode(instanceId);
  const next: LoopMode =
    cur === "none" ? "all" : cur === "all" ? "one" : "none";
  setLoopMode(instanceId, next);
  try {
    if (instanceId === queue.instanceId) {
      await enqueueArm(instanceId, () =>
        armPlanFor(loopMode(instanceId), queue.items, queue.index),
      );
      return;
    }
    // キュー外インスタンス: none 以外は現在項目のみの巡回（繰り返し）。none は解除
    await enqueueArm(instanceId, () =>
      loopMode(instanceId) === "none"
        ? { items: [], loop: false }
        : {
            items: [playerStates.list.get(instanceId)?.videoId ?? ""].filter(
              (v) => v !== "",
            ),
            loop: true,
          },
    );
  } catch {
    // インスタンスが既に無い場合はキュー・ループ状態を畳む
    if (instanceId === queue.instanceId) stopQueue();
    clearLoop(instanceId);
  }
}

/// 今後の項目列をバックエンドへ武装する。ループモードに応じて順序を選ぶ
/// （対象の決定は armPlanFor、LP-NN 系回帰テストで検証）。
/// 即時の継続はバックエンドのキューが担う。遷移イベント後の呼び出しは
/// 遷移中に変わったモードやキューを次周回以降の意図へ直すためのもので、
/// この登録が遅れても継続自体は保たれる（仕様決定 AD）。
async function armNext(): Promise<void> {
  if (queue.instanceId === null) return;
  const id = queue.instanceId;
  try {
    await enqueueArm(id, () =>
      armPlanFor(loopMode(id), queue.items, queue.index),
    );
  } catch {
    // インスタンスが既に無い場合はキューを畳む
    stopQueue();
  }
}

/// キューを開始する。呼び出し側は `items[index]` の再生を別途起動済みで、
/// その instanceId を渡す。終了後は自動で次項目へ進む。
/// `playlistId` が null のときは一時キュー（キュースタック）として扱う
export async function startQueue(
  playlistId: number | null,
  playlistName: string,
  items: string[],
  index: number,
  instanceId: number,
): Promise<void> {
  queue.playlistId = playlistId;
  queue.playlistName = playlistName;
  queue.items = items;
  queue.index = index;
  queue.instanceId = instanceId;
  await armNext();
}

/// キューを畳む。ループモードが残るインスタンスは単独ループへ移行する
/// （キュー無しの全体/1 項目は現在項目の繰り返し。仕様決定 AA）。
/// なしなら武装を解除して終端で自然に終了する。
export function stopQueue(): void {
  if (queue.instanceId !== null) {
    const id = queue.instanceId;
    // 実際に再生中の項目は player state を優先し、無ければキュー位置。
    // 進行済みの遷移が未着の場合は、到着する ended イベントの再武装が
    // この武装をチェーン上で上書きする（直列化で後着が必ず勝つ）
    const vid =
      playerStates.list.get(id)?.videoId ??
      queue.items[queue.index] ??
      null;
    queue.instanceId = null;
    void enqueueArm(id, () =>
      loopMode(id) === "none"
        ? { items: [], loop: false }
        : { items: vid === null ? [] : [vid], loop: true },
    ).catch(() => {});
  }
  queue.playlistId = null;
  queue.playlistName = "";
  queue.items = [];
  queue.index = 0;
}

/// キューがこのプレイリストを背負っているか。
export function queueActive(playlistId: number): boolean {
  return queue.playlistId === playlistId;
}

/// 一時キュー（積み上げ中・実行中の双方）を背負っているか（FR-20）。
export function tempQueueActive(): boolean {
  return queue.playlistId === null && queue.items.length > 0;
}

/// 「キューに追加」（FR-20、仕様決定 AM）。末尾へ積む。
/// 実行中（一時・プレイリストどちらのキューでも）はそのキューへの追記として
/// 武装を張り替え、未実行なら積むだけで再生は開始しない。
/// 同一動画の重複登録は許す
export function queueAdd(v: VideoRef): void {
  rememberMeta(v);
  queue.items = [...queue.items, v.videoId];
  if (queue.instanceId !== null) void armNext();
}

/// 「次に再生」（FR-20、仕様決定 AM）。実行中は現在項目の直後、
/// 未実行は先頭へ挿入する。武装の張り替えは queueAdd と同じ
export function queuePlayNext(v: VideoRef): void {
  rememberMeta(v);
  const at = queue.instanceId === null ? 0 : queue.index + 1;
  const items = [...queue.items];
  items.splice(at, 0, v.videoId);
  queue.items = items;
  if (queue.instanceId !== null) void armNext();
}

/// 項目の個別削除。再生中項目を消しても再生は止めない
/// （index は配列上で次項目を指す）。実行中は武装を張り替える
export function queueRemoveAt(i: number): void {
  if (i < 0 || i >= queue.items.length) return;
  const items = [...queue.items];
  items.splice(i, 1);
  queue.items = items;
  queue.index = indexAfterRemove(queue.index, i, items.length);
  if (queue.instanceId !== null) void armNext();
}

/// 項目の移動（from の項目を to の位置へ挿入）。再生中項目は
/// 位置ではなく項目そのものを追従させる。実行中は武装を張り替える
export function queueMove(from: number, to: number): void {
  if (from === to || from < 0 || from >= queue.items.length) return;
  const to2 = Math.max(0, Math.min(to, queue.items.length - 1));
  const items = [...queue.items];
  const [m] = items.splice(from, 1);
  items.splice(to2, 0, m);
  queue.index = indexAfterMove(queue.index, from, to2);
  queue.items = items;
  if (queue.instanceId !== null) void armNext();
}

/// 全消去（FR-20）。実行中はキューを畳んで継続再生を解除する
/// （再生そのものは止めない）。積み上げ中なら項目を捨てるだけ
export function queueClear(): void {
  stopQueue();
  queueMeta.list = new Map();
}

/// キューの先頭項目から連続再生を開始する（FR-20、仕様決定 AM）。
/// 既にキュー実行中のときは何もしない（パネル側で無効化する）。
/// 失敗時はエラーをそのまま投げ、呼び出し側が通知する
export async function queuePlayStart(): Promise<void> {
  const vid = queue.items[0];
  if (vid === undefined || queue.instanceId !== null) return;
  const instanceId = await invoke<number>("play_video", {
    videoId: vid,
    resume: true,
  });
  await startQueue(null, "", queue.items, 0, instanceId);
}

/// 現在の再生位置（その項目が再生中なら true）。
export function queuePlayingAt(videoId: string): boolean {
  return (
    queue.instanceId !== null && queue.items[queue.index] === videoId
  );
}

let initPromise: Promise<void> | null = null;

/// `player://ended` を購読してキューを進め、キュー外インスタンスの
/// ループ繰り返しを再武装する。最初の onMount で一度だけ登録する。
export function initQueueEvents(): Promise<void> {
  if (!initPromise) {
    initPromise = (async () => {
      await listen<PlayerEnded>("player://ended", (ev) => {
        const p = ev.payload;
        // 観測世代を更新してから各分岐で使う
        armedSeqs.set(p.instanceId, p.armedSeq);
        if (!p.continued) {
          // 終端（末尾・途中失敗・手動停止）: キューとループ状態を畳む
          if (p.instanceId === queue.instanceId) stopQueue();
          clearLoop(p.instanceId);
          armedSeqs.delete(p.instanceId);
          rearmPending.delete(p.instanceId);
          return;
        }
        if (p.instanceId === queue.instanceId) {
          // 実際に読み込みが始まった項目で位置を照合する
          queue.index = reconcileIndex(
            queue.items,
            queue.index,
            p.continuedVideoId,
          );
          // 意図の変更が世代ずれで backend に拒否されていたときだけ
          // 最新世代で再適用する。通常の遷移では deque を置き換えない
          // （古い置換が消費済み項目を復活させるのを防ぐ、仕様決定 AD）
          if (rearmPending.has(p.instanceId)) void armNext();
          return;
        }
        // キュー外インスタンス。継続は backend の巡回が担うので、
        // 未反映の意図があるときだけ再適用する（遷移中の stopQueue や
        // モード変更が世代ずれで省かれたケース。実際に読まれた項目で張り直す）
        if (rearmPending.has(p.instanceId)) {
          const vid = p.continuedVideoId ?? p.videoId;
          void enqueueArm(p.instanceId, () =>
            loopMode(p.instanceId) === "none"
              ? { items: [], loop: false }
              : { items: [vid], loop: true },
          ).catch(() => clearLoop(p.instanceId));
        }
      });
    })();
  }
  return initPromise;
}
