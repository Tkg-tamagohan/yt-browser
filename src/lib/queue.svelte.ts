//! 連続再生キューとループ（FR-10・FR-20・FR-26、仕様決定 S・AA・AD・AM・AS）。
//! キューはフロント側のセッション状態として持つ。一時キュー 1 個
//! （`playlistId` なし）とプレイリストごとのキュー（最大 1 個）を
//! 同時に保持でき、各キューは項目列・現在位置・実行インスタンスを
//! 独立に持つ（FR-26、仕様決定 AS）。複数のキューが別々の mpv
//! インスタンスを背負って同時に連続再生できる。
//! 今後の項目は `player_set_queue` で順序列入れてバックエンドへ事前登録
//! （武装）し、mpv の終端イベントで同一インスタンスが読み替える。
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
  headPlayingAfterRemove,
  indexAfterMove,
  indexAfterRemove,
  indexAfterStop,
  queueEndOutcome,
  reconcileIndex,
} from "./queue-logic";

/// キューの識別子。`null` は一時キュー（キュースタック、FR-20）、
/// 数値はそのプレイリストのキュー（FR-26、仕様決定 AS）
export type QueueKey = number | null;

/// キュー 1 個の状態。`items` は動画 ID 列、`index` は現在の再生位置。
/// `instanceId` はキューを背負うプレイヤーインスタンスで、未実行で
/// 積んでいるだけのときと「連続再生を止める」で外したあとは null。
/// `playingCurrent` は実行中に `items[index]` が実際に再生中かどうか。
/// 再生中項目を削除した直後は index が未再生の次項目を指すため false に
/// なり、次の継続項目の読み込みで true に戻る（武装対象の先頭を決める）
export type QueueState = {
  playlistId: number | null;
  playlistName: string;
  items: string[];
  index: number;
  instanceId: number | null;
  playingCurrent: boolean;
};

/// 空のキュー状態を作る。$state プロキシで返し、Map 値として保持する
/// 各キューの項目・位置・インスタンス変更を追跡できるようにする
function newQueue(playlistId: number | null, playlistName: string): QueueState {
  const q = $state<QueueState>({
    playlistId,
    playlistName,
    items: [],
    index: 0,
    instanceId: null,
    playingCurrent: true,
  });
  return q;
}

/// 保持中のキュー群（FR-26、仕様決定 AS）。キーは QueueKey。
/// 一時キュー（キー null）のエントリは常時存在し、中身の空きだけで
/// 「積み上げ中か」を表す。プレイリストキューは作成と消滅でエントリが
/// 増減し、エントリの存在が「そのプレイリストにキューがある」ことを示す
export const queues = $state<{ list: Map<QueueKey, QueueState> }>({
  list: new Map<QueueKey, QueueState>([[null, newQueue(null, "")]]),
});

/// キーでキューを引く。存在しなければ undefined（一時キューは常時存在）
export function queueAt(key: QueueKey): QueueState | undefined {
  return queues.list.get(key);
}

/// 一時キューのエントリ（常時存在）。「キューに追加」「次に再生」の
/// 送り先は実行中キューの有無に関わらず常にこちら（FR-26、仕様決定 AS）
export function tempQueue(): QueueState {
  const q = queues.list.get(null);
  if (q === undefined) throw new Error("一時キューのエントリがありません");
  return q;
}

/// インスタンスを背負っているキューのキー。どのキューにも属さなければ
/// undefined（通常再生・キューから外れたインスタンス）
export function queueKeyOfInstance(instanceId: number): QueueKey | undefined {
  for (const [key, q] of queues.list) {
    if (q.instanceId === instanceId) return key;
  }
  return undefined;
}

/// キュー項目の表示メタ（FR-20、仕様決定 AM）。セッション内メモリのみで
/// DB には保存しない。同一動画の重複登録は同じメタを共有し、
/// メタはキューをまたいで共有する（同じ動画は同じ表示になる）
export type QueueItemMeta = {
  title: string;
  channelTitle: string | null;
  thumbnailUrl: string | null;
};
export const queueMeta = $state<{ list: Map<string, QueueItemMeta> }>({
  list: new Map(),
});

/// キューパネルの開閉と表示対象キュー（FR-26）。ナビ入口とキューを
/// 背負うカードの両方から共有する。`selected` は表示対象キューのキーで、
/// 選択中のキューが消滅したら一時キューへ戻すのはパネル側の責務
export const queuePanel = $state<{ open: boolean; selected: QueueKey }>({
  open: false,
  selected: null,
});

/// メタを記憶する。キューへの投入経路（キューに追加・次に再生・
/// プレイリストキューのスナップショット作成）ごとに呼び、
/// 重複登録は先のメタを維持する。タイトルが空のときは動画 ID に
/// 潰して、一覧表示が空白にならないようにする
export function rememberMeta(v: VideoRef): void {
  if (queueMeta.list.has(v.videoId)) return;
  const next = new Map(queueMeta.list);
  next.set(v.videoId, {
    title: v.title || v.videoId,
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

/// どのキューにも残っていない動画の表示メタを解放する。個別削除やキュー
/// 消滅後も長時間のセッションでメタが溜まり続けないようにする
function pruneMeta(): void {
  const used = new Set<string>();
  for (const q of queues.list.values()) {
    for (const id of q.items) used.add(id);
  }
  let next: Map<string, QueueItemMeta> | null = null;
  for (const id of queueMeta.list.keys()) {
    if (!used.has(id)) {
      if (!next) next = new Map(queueMeta.list);
      next.delete(id);
    }
  }
  if (next) queueMeta.list = next;
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
  const key = queueKeyOfInstance(instanceId);
  const q = key !== undefined ? queues.list.get(key) : undefined;
  if (q !== undefined) {
    return armPlanFor(
      loopMode(instanceId),
      q.items,
      q.index,
      q.playingCurrent,
    );
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
  const key = queueKeyOfInstance(instanceId);
  try {
    if (key !== undefined) {
      // キューを背負うインスタンス: そのキューの内容で武装を張り替える。
      // 計画の評価時点でキューが外れていれば現況の最新意図で計画する
      await enqueueArm(instanceId, () => {
        const q = queues.list.get(key);
        if (q === undefined || q.instanceId !== instanceId) {
          return latestPlan(instanceId);
        }
        return armPlanFor(
          loopMode(instanceId),
          q.items,
          q.index,
          q.playingCurrent,
        );
      });
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
    // インスタンスが既に無い場合はキュー・ループ状態を畳む。
    // reject 時点でキューが別インスタンスへ張り替わっている世代は
    // 巻き込まないよう、今もこのインスタンスを背負っているか確かめる
    const cur = key !== undefined ? queues.list.get(key) : undefined;
    if (
      key !== undefined &&
      cur !== undefined &&
      cur.instanceId === instanceId
    ) {
      discardQueue(key);
    }
    clearLoop(instanceId);
  }
}

/// キュー 1 個の今後の項目列をバックエンドへ武装する。ループモードに応じて
/// 順序を選ぶ（対象の決定は armPlanFor、LP-NN 系回帰テストで検証）。
/// 即時の継続はバックエンドのキューが担う。遷移イベント後の呼び出しは
/// 遷移中に変わったモードやキューを次周回以降の意図へ直すためのもので、
/// この登録が遅れても継続自体は保たれる（仕様決定 AD）。
/// 計画は実行時点で評価し、キューが外れていれば現況の最新意図へ逃がす
async function armQueue(q: QueueState): Promise<void> {
  if (q.instanceId === null) return;
  const id = q.instanceId;
  try {
    await enqueueArm(id, () =>
      q.instanceId === id
        ? armPlanFor(loopMode(id), q.items, q.index, q.playingCurrent)
        : latestPlan(id),
    );
  } catch {
    // インスタンスが既に無い場合はキューを畳む。
    // reject 時点で別インスタンスへ張り替わっている、あるいは
    // detach 済みの世代は巻き込まない（q.instanceId は張り替えで
    // 新しい id、detach・畳み込みで null に変わる）
    if (q.instanceId === id) discardQueue(q.playlistId);
  }
}

/// キューを開始する。呼び出し側は `items[index]` の再生を別途起動済みで、
/// その instanceId を渡す。終了後は自動で次項目へ進む。
/// 同じキーの既存キューは捨てて新しいスナップショットで上書きする
/// （「ここから連続再生」による置き換え、FR-26）。実行中キューへの
/// 再開始では呼び出し側が同じインスタンスへ読み替えたうえで同じ
/// instanceId を渡す（セッション引き継ぎ）
export async function startQueue(
  playlistId: number | null,
  playlistName: string,
  items: string[],
  index: number,
  instanceId: number,
): Promise<void> {
  let q = queues.list.get(playlistId);
  if (q !== undefined) {
    // 別インスタンスへ置き換える場合は、既存キューのインスタンスを
    // キューから外してから上書きする（外れたインスタンスはループ状態に
    // 応じて継続または終端へ向かう）。セッション引き継ぎ（同じ
    // インスタンスへの再開始）では外さない
    if (q.instanceId !== instanceId) detachInstance(q);
    q.playlistName = playlistName;
    q.items = items;
    q.index = index;
    q.instanceId = instanceId;
    q.playingCurrent = true;
  } else {
    q = newQueue(playlistId, playlistName);
    q.items = items;
    q.index = index;
    q.instanceId = instanceId;
    const next = new Map(queues.list);
    next.set(playlistId, q);
    queues.list = next;
  }
  await armQueue(q);
}

/// インスタンスをキューから外し、残っているループモードに応じた武装へ
/// 張り替える（キュー停止系の共通処理）。ループモードが残るインスタンスは
/// 単独ループへ移行し、なしなら武装を解除して終端で自然に終了する
/// （キュー無しの全体/1 項目は現在項目の繰り返し。仕様決定 AA）
function detachInstance(q: QueueState): void {
  if (q.instanceId === null) return;
  const id = q.instanceId;
  // 実際に再生中の項目は player state を優先し、無ければキュー位置。
  // 進行済みの遷移が未着の場合は、到着する ended イベントの再武装が
  // この武装をチェーン上で上書きする（直列化で後着が必ず勝つ）
  const vid = playerStates.list.get(id)?.videoId ?? q.items[q.index] ?? null;
  q.instanceId = null;
  void enqueueArm(id, () =>
    loopMode(id) === "none"
      ? { items: [], loop: false }
      : { items: vid === null ? [] : [vid], loop: true },
  ).catch(() => {});
}

/// キューを畳む（キュー消費完了・インスタンス消失・全消去・新しいキュー
/// による上書き・対象プレイリスト削除の共通経路、FR-26）。
/// 一覧と表示メタを捨て、再生中のインスタンスは detachInstance の
/// ループ状態に応じた武装で継続または終端へ向かわせる。
/// 一時キューはエントリを残して中身を空にし、プレイリストキューは
/// エントリごと消す
export function discardQueue(key: QueueKey): void {
  const q = queues.list.get(key);
  if (q === undefined) return;
  detachInstance(q);
  if (key === null) {
    q.items = [];
    q.index = 0;
    q.playingCurrent = true;
  } else {
    const next = new Map(queues.list);
    next.delete(key);
    queues.list = next;
  }
  pruneMeta();
}

/// 「連続再生を止める」（FR-20・FR-26、仕様決定 AM・AS）。
/// インスタンスをキューから外して継続再生を止めるが、一覧は残す
/// （一覧まで捨てるのは全消去 discardQueue の役割）。
/// 一時キューは位置を先頭に戻し、プレイリストキューは現在位置を保つ
/// （キューパネルの「再生」で保存位置から再開する。indexAfterStop）
export function queueStop(key: QueueKey): void {
  const q = queues.list.get(key);
  if (q === undefined || q.instanceId === null) return;
  detachInstance(q);
  q.index = indexAfterStop(q.playlistId, q.index);
  q.playingCurrent = true;
}

/// 「キューに追加」（FR-20、仕様決定 AM・AS）。一時キューの末尾へ積む。
/// 実行中キューがプレイリストキューでも送り先は常に一時キューで、
/// 一時キュー実行中はそのキューへの追記として武装を張り替える。
/// 同一動画の重複登録は許す
export function queueAdd(v: VideoRef): void {
  const q = tempQueue();
  rememberMeta(v);
  q.items = [...q.items, v.videoId];
  if (q.instanceId !== null) void armQueue(q);
}

/// 「次に再生」（FR-20、仕様決定 AM・AS）。一時キュー実行中は現在項目の
/// 直後、未実行は先頭へ挿入する。武装の張り替えは queueAdd と同じ
export function queuePlayNext(v: VideoRef): void {
  const q = tempQueue();
  rememberMeta(v);
  const at = q.instanceId === null ? 0 : q.index + 1;
  const items = [...q.items];
  items.splice(at, 0, v.videoId);
  q.items = items;
  if (q.instanceId !== null) void armQueue(q);
}

/// 項目の個別削除。再生中項目を消しても再生は止めない
/// （index は配列上で次項目を指す）。実行中は武装を張り替える。
/// 再生中項目を消したとき index が指すのは未再生の次項目なので、
/// 武装対象にそれを含めるよう playingCurrent を false にする
/// （消さないとその項目が再生されずに飛ばされる）
export function queueRemoveAt(key: QueueKey, i: number): void {
  const q = queues.list.get(key);
  if (q === undefined || i < 0 || i >= q.items.length) return;
  const items = [...q.items];
  items.splice(i, 1);
  q.playingCurrent = headPlayingAfterRemove(
    q.playingCurrent,
    q.index,
    i,
    items.length,
  );
  q.items = items;
  q.index = indexAfterRemove(q.index, i, items.length);
  pruneMeta();
  if (q.instanceId !== null) void armQueue(q);
}

/// 項目の移動（from の項目を to の位置へ挿入）。再生中項目は
/// 位置ではなく項目そのものを追従させる。実行中は武装を張り替える
export function queueMove(key: QueueKey, from: number, to: number): void {
  const q = queues.list.get(key);
  if (q === undefined || from === to || from < 0 || from >= q.items.length) {
    return;
  }
  const to2 = Math.max(0, Math.min(to, q.items.length - 1));
  const items = [...q.items];
  const [m] = items.splice(from, 1);
  items.splice(to2, 0, m);
  q.index = indexAfterMove(q.index, from, to2);
  q.items = items;
  if (q.instanceId !== null) void armQueue(q);
}

/// 全消去（FR-20・FR-26）。実行中はキューを畳んで継続再生を解除する
/// （再生そのものは止めない）。積み上げ中・停止中なら項目を捨てるだけ。
/// 表示メタは discardQueue 側の pruneMeta で一覧と一緒に解放する
export function queueClear(key: QueueKey): void {
  discardQueue(key);
}

/// キューの現在位置から連続再生を開始する（FR-20・FR-26、仕様決定 AM・AS）。
/// 一時キューの位置は常に先頭（停止時に先頭へ戻るため）。
/// 停止中のプレイリストキューは保存された位置から再開する。
/// 既に実行中のときは何もしない（パネル側で無効化する）。
/// 失敗時はエラーをそのまま投げ、呼び出し側が通知する
export async function queuePlayStart(key: QueueKey): Promise<void> {
  const q = queues.list.get(key);
  if (q === undefined || q.instanceId !== null) return;
  const vid = q.items[q.index];
  if (vid === undefined) return;
  const instanceId = await invoke<number>("play_video", {
    videoId: vid,
    resume: true,
  });
  await startQueue(q.playlistId, q.playlistName, q.items, q.index, instanceId);
}

/// そのプレイリストのキューで現在の再生位置がこの項目か
/// （その項目が再生中なら true）。
export function queuePlayingAt(
  playlistId: number | null,
  videoId: string,
): boolean {
  const q = playlistId === null ? undefined : queues.list.get(playlistId);
  return (
    q !== undefined &&
    q.instanceId !== null &&
    q.items[q.index] === videoId
  );
}

let initPromise: Promise<void> | null = null;

/// `player://ended` を購読して各キューを進め、キュー外インスタンスの
/// ループ繰り返しを再武装する。最初の onMount で一度だけ登録する。
/// 終端（非継続）の処置は FR-26 の寿命ルールに従う:
/// プレイリストキューで末尾項目の eof 終了は消費完了として位置を
/// 先頭に戻してキューを残し、それ以外の消失ではキューを畳む
/// （queueEndOutcome）。一時キューは理由に関わらず畳む
export function initQueueEvents(): Promise<void> {
  if (!initPromise) {
    initPromise = (async () => {
      await listen<PlayerEnded>("player://ended", (ev) => {
        const p = ev.payload;
        // 観測世代を更新してから各分岐で使う
        armedSeqs.set(p.instanceId, p.armedSeq);
        const key = queueKeyOfInstance(p.instanceId);
        const q = key !== undefined ? queues.list.get(key) : undefined;
        if (!p.continued) {
          // 終端（末尾・途中失敗・手動停止・プロセス消失）: 寿命ルールに
          // 従ってキューを残すか畳むかを決め、ループ状態を外す
          if (q !== undefined && key !== undefined) {
            if (
              queueEndOutcome(
                q.playlistId,
                p.reason,
                q.items,
                q.index,
                p.videoId,
              ) === "reset"
            ) {
              // 消費完了: インスタンスは既に無い。位置を先頭に戻して残す
              q.instanceId = null;
              q.index = 0;
              q.playingCurrent = true;
            } else {
              discardQueue(key);
            }
          }
          clearLoop(p.instanceId);
          armedSeqs.delete(p.instanceId);
          rearmPending.delete(p.instanceId);
          return;
        }
        if (q !== undefined) {
          // 実際に読み込みが始まった項目で位置を照合する
          q.index = reconcileIndex(q.items, q.index, p.continuedVideoId);
          // 読み込まれた項目が index 位置と一致すれば再生中扱いに戻す。
          // drift（照合失敗）では現状を維持する: この時点で index の
          // 項目は消費済みかもしれず、一律 false にすると再武装で
          // 再生済みの項目を意図せず再登録してしまうため。未再生扱い
          // への遷移は queueRemoveAt（削除経路）だけが行う
          if (
            p.continuedVideoId !== null &&
            q.items[q.index] === p.continuedVideoId
          ) {
            q.playingCurrent = true;
          }
          // 意図の変更が世代ずれで backend に拒否されていたときだけ
          // 最新世代で再適用する。通常の遷移では deque を置き換えない
          // （古い置換が消費済み項目を復活させるのを防ぐ、仕様決定 AD）
          if (rearmPending.has(p.instanceId)) void armQueue(q);
          return;
        }
        // キュー外インスタンス。継続は backend の巡回が担うので、
        // 未反映の意図があるときだけ再適用する（遷移中のキュー畳みや
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
