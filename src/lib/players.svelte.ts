// プレイヤー状態の共有ストア（設計書 §3.2 のイベント経路）。
// 複数ページ（再生・設定）で同じマップを参照するため、イベント購読は一度だけ初期化する。

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { AbEntry } from "./ab-logic";

export type PlayStatus = "idle" | "playing" | "paused" | "buffering" | "ended";

export type PlayerState = {
  instanceId: number;
  videoId: string;
  pause: boolean;
  position: number;
  duration: number;
  fps: number;
  state: PlayStatus;
  volume: number;
  speed: number;
  mediaTitle: string;
  /// PiP（最前面・枠なしの小窓）表示中かどうか（設計書 §4.5）。
  pip: boolean;
  /// 現在適用中の画質式（`ytdl-format`）。
  format: string;
  /// 現在の音声出力デバイス（mpv `audio-device`、FR-32、仕様決定 BA）。
  /// 既定・自動選択は "auto"。
  audioDevice: string;
  /// mpv `track-list` のうち字幕（`type=sub`）トラックの一覧
  /// （FR-40、仕様決定 BP）。未取得時は空。
  subTracks: SubTrack[];
  /// 現在の字幕選択（mpv `sid`、FR-40）。"no" / "auto" / トラック ID の文字列。
  sid: string;
};

/// 字幕トラック 1 件分（FR-40、仕様決定 BP）。
/// mpv `track-list` の `type=sub` エントリ由来の id・title・lang。
export type SubTrack = {
  /// mpv トラック ID（`sid` へ渡す値）。
  id: number;
  title: string | null;
  lang: string | null;
};

/// `audio_device_list` コマンドの 1 要素（FR-32、仕様決定 BA）。
/// mpv が列挙した音声出力デバイス。name が識別子、description が表示名。
export type AudioDevice = {
  name: string;
  description: string;
};

export type PlayerEnded = {
  instanceId: number;
  videoId: string;
  reason: string;
  /// 連続再生で同一インスタンスが次項目へ進んだとき true（FR-10、仕様決定 S）
  continued: boolean;
  /// 継続先として読み込みを開始した項目（FR-16、仕様決定 AA）。
  /// videoId と同一なら現在項目の繰り返し（ループ）。非継続時は null
  continuedVideoId: string | null;
  /// 継続先項目のユニーク ID（BG-10、仕様決定 BK）。キュー位置は
  /// この値で照合するため、同一動画の連続登録でもずれない。非継続時は null
  continuedUid: number | null;
  /// このイベント時点の武装取り出し世代（仕様決定 AD）。
  /// `player_set_queue` の `baseSeq` に載せて古い置換を拒否させる
  armedSeq: number;
  /// この終端でインスタンスが畳まれたとき true（FR-43、仕様決定 BS）。
  /// 継続時は常に false。非継続でも `player.close_on_eof=off` の eof
  /// 終端では false が届き、mpv 窓とカードは残る。後始末は true のときだけ行う
  closed: boolean;
};

/// `player://restarted` イベント（設計書 §3.2、仕様決定 BE）。
/// IPC 無応答ウォッチドッグによる mpv 自動再起動の開始。インスタンスは消滅しない
export type PlayerRestarted = {
  instanceId: number;
  videoId: string;
  attempt: number;
};

/// `sponsor://skipped` イベント（設計書 §3.2）。action は "skip" | "notify"。
export type SponsorSkipped = {
  instanceId: number;
  videoId: string;
  category: string;
  segment: [number, number];
  action: "skip" | "notify";
};

/// `sponsor://segments` の 1 区間分（FR-36、仕様決定 BI、設計書 §3.2）。
export type SponsorSegment = {
  category: string;
  start: number;
  end: number;
  /// "skip" | "notify" | "off"。off の区間はシークバーへ出さない。
  action: "skip" | "notify" | "off";
};

/// `sponsor://segments` イベント（設計書 §3.2）。
/// 区間取得の完了時にインスタンスごとに届き、区間なし・取得失敗は空列。
export type SponsorSegmentsPayload = {
  instanceId: number;
  videoId: string;
  segments: SponsorSegment[];
};

export type WatchHistory = {
  videoId: string;
  title: string;
  channelId: string | null;
  channelTitle: string | null;
  positionSec: number;
  durationSec: number | null;
  lastWatchedAt: string;
  completed: boolean;
};

/// `favorite_add` / `playlist_add` の入力（検索・関連・フィード行のメタを渡す）。
export type VideoRef = {
  videoId: string;
  title: string;
  channelId: string | null;
  channelTitle: string | null;
  thumbnailUrl: string | null;
};

/// `favorite_list` の 1 行（FR-7）。
export type FavoriteEntry = {
  videoId: string;
  title: string;
  channelId: string | null;
  channelTitle: string | null;
  thumbnailUrl: string | null;
  addedAt: string;
};

/// `playlist_list` の 1 行（FR-7）。
export type Playlist = {
  id: number;
  name: string;
  sortOrder: number;
  itemCount: number;
};

/// `playlist_items` の 1 行（FR-7）。
export type PlaylistEntry = {
  position: number;
  videoId: string;
  title: string;
  channelId: string | null;
  channelTitle: string | null;
  thumbnailUrl: string | null;
  /// 投稿日時（フィード投入済み項目のみ。未取得は null、ソート時は末尾）
  publishedAt: string | null;
};

export type UiError = { code: string; message: string };

/// invoke の失敗を通知表示用の文字列にする。UiError は message を持つので
/// それを取り出し、それ以外は文字列化する。
export function asErrorMessage(e: unknown): string {
  if (typeof e === "object" && e !== null && "message" in e) {
    return String((e as UiError).message);
  }
  return String(e);
}

export type YtDlpStatus = { path: string | null; version: string | null };

export type DbStatus = { schemaVersion: number };

export type SearchResult = {
  videoId: string;
  title: string;
  channelId: string | null;
  // @handle 形の投稿者 ID。ブロックキーには使えないが購読入力には使える
  uploaderId: string | null;
  channelTitle: string | null;
  durationSec: number | null;
  viewCount: number | null;
  thumbnailUrl: string | null;
};

export type BlockedChannel = {
  channelId: string;
  title: string;
  createdAt: string;
};

/// `chat://message` バッチの 1 要素（設計書 §3.1 の ChatEvent）。
export type ChatEvent = {
  /// InnerTube のアイテム ID。削除イベントはこの値ではなく
  /// `message` に対象の item ID が入る。
  itemId: string;
  videoId: string;
  postedAtUsec: number;
  authorChannelId: string | null;
  authorName: string | null;
  kind: "text" | "superchat" | "membership" | "deleted" | "other";
  message: string;
  amountDisplay: string | null;
  /// NG フィルタで非表示判定されたもの。送信されるが UI は出さない。
  ng: boolean;
  /// リプレイの動画内時刻（ms）。ライブでは未定義（FR-24）。
  videoOffsetMs?: number;
};

/// `chat://status` イベント（設計書 §3.2）。
export type ChatStatus = {
  videoId: string | null;
  level: "info" | "warn" | "error" | string;
  message: string;
};

/// `filters` テーブルの 1 行（NG フィルタ）。
export type Filter = {
  id: number;
  target: string;
  kind: string;
  pattern: string;
  enabled: boolean;
  createdAt: string;
};

export type PlayerAction =
  | { type: "pause"; value: boolean }
  | { type: "seek"; seconds: number }
  | { type: "volume"; value: number }
  | { type: "speed"; value: number }
  | { type: "quality"; format: string }
  | { type: "frame_step" }
  | { type: "frame_back_step" }
  | { type: "pip"; enabled: boolean }
  // PiP 窓の一括整列（FR-41、仕様決定 BQ）。geometry は格子計算済みの
  // `WxH+x+y`（対象ディスプレイ左上原点の相対座標）
  | { type: "pip_tile"; geometry: string }
  // 音声出力先の切替（FR-32、仕様決定 BA）。値は audio_device_list の name
  | { type: "audio_device"; device: string }
  // A-B リピートの 3 状態巡回に対応する mpv 操作（FR-37、仕様決定 BM）
  | { type: "ab_loop_set_a" }
  | { type: "ab_loop_set_b" }
  | { type: "ab_loop_clear" }
  // 字幕トラックの選択（FR-40、仕様決定 BP）。
  // sid は "no" / "auto" / トラック ID の文字列。セッション内のみ有効
  | { type: "sub_track"; sid: string }
  // mpv 窓の前面化（FR-42、仕様決定 BR）。Wayland は恒久制限で非対応
  | { type: "focus_window" };

export const playerStates = $state<{
  list: Map<number, PlayerState>;
}>({ list: new Map() });

/// インスタンスごとの SponsorBlock 区間（FR-36、仕様決定 BI）。
/// 動画 ID を併せて持ち、描画側で現在再生中の動画と照合する。
export const sponsorSegments = $state<{
  list: Map<number, { videoId: string; segments: SponsorSegment[] }>;
}>({ list: new Map() });

/// シークバーへの SponsorBlock 区間帯の表示（`sponsor.show_segments`、FR-36）。
/// 既定・読み取り失敗は on。
export const sponsorShowSegments = $state({ enabled: true });

/// インスタンス別の A-B リピート状態（FR-37、仕様決定 BM）。セッション内のみ
/// 有効で DB には保存しない。エントリは動画 ID に束ね、再生中動画と一致し
/// ないものは表示側が off として扱う（ab-logic.ts の abStateFor）。
/// mpv 側のループ解除は loadfile 経路（load_video）とプロセス差し替えが担う。
export const abLoops = $state<{ list: Map<number, AbEntry> }>({
  list: new Map(),
});

/// インスタンス終了・mpv 再起動・動画切替での A-B ループ状態の片付け。
/// ended では継続の有無に関わらず呼ぶ（連続再生・同一動画の読み替えでも
/// mpv 側は load_video でループを解除するため、残すと次動画へ誤表示が乗る）。
export function dropAbLoop(instanceId: number): void {
  if (!abLoops.list.has(instanceId)) return;
  const next = new Map(abLoops.list);
  next.delete(instanceId);
  abLoops.list = next;
}

/// instanceId の区間エントリを外す（終了・クローズ・動画遷移時の片付け）。
function dropSponsorSegments(instanceId: number): void {
  if (!sponsorSegments.list.has(instanceId)) return;
  const next = new Map(sponsorSegments.list);
  next.delete(instanceId);
  sponsorSegments.list = next;
}

/// インスタンス別画質を指定済みの instanceId 集合（仕様決定 X。セッション内のみ有効）。
/// プレイヤーカードの画質選択が成功した時点で登録し、インスタンス終了で解放する。
/// 設定画面の全体画質の即時適用はこの集合のインスタンスを除外する
/// （個別指定が全体既定で消えないようにする）
export const formatOverrides = new Set<number>();

/// 再生インスタンスの終了・手動クローズ直後に呼び出すページ側フック
/// （resumeHint の再取得など）。PlayerCards はレイアウトで常時マウントのため
/// ページのローカル状態を直接参照できず、+page はマウント中だけここに登録する。
export const playbackHooks = new Set<() => void>();

let initPromise: Promise<void> | null = null;

/// `player://state` / `player://ended` を購読して共有マップを更新する。
/// ページをまたいで利用するため、最初の onMount で一度だけ登録する。
export function initPlayerEvents(): Promise<void> {
  if (!initPromise) {
    initPromise = (async () => {
      // player_list の応答を待つ間に届いたイベントの instanceId を記録する。
      // スナップショットは取得時点の値なので、飛行中に更新・終了が起きた
      // インスタンスはイベント側を優先し、古い値で上書きしない。
      const inFlight = new Set<number>();
      await listen<PlayerState>("player://state", (ev) => {
        inFlight.add(ev.payload.instanceId);
        const next = new Map(playerStates.list);
        next.set(ev.payload.instanceId, ev.payload);
        playerStates.list = next;
      });
      // SponsorBlock 区間の表示用データ（FR-36、仕様決定 BI）。
      // 前動画の区間が残らないよう ended/closed で削除し、
      // 区間なし・取得失敗は空列で届く
      await listen<SponsorSegmentsPayload>("sponsor://segments", (ev) => {
        const next = new Map(sponsorSegments.list);
        next.set(ev.payload.instanceId, {
          videoId: ev.payload.videoId,
          segments: ev.payload.segments,
        });
        sponsorSegments.list = next;
      });
      await listen<PlayerEnded>("player://ended", (ev) => {
        inFlight.add(ev.payload.instanceId);
        // close_on_eof=off の eof 終端ではインスタンスと動画が残るため
        // カード・区間・AB ループ・画質上書きをいずれも畳まない
        // （FR-43、仕様決定 BS）
        if (!ev.payload.continued && ev.payload.closed === false) {
          return;
        }
        // 継続・畳まれた非継続を問わずこの動画の区間表示は終わる。
        // 継続先の区間は次動画分の sponsor://segments で届くまで無しとする
        dropSponsorSegments(ev.payload.instanceId);
        // A-B ループも動画の切替で mpv 側が解除するため表示状態を畳む
        dropAbLoop(ev.payload.instanceId);
        // 連続再生で次項目へ進んだインスタンスは稼働中のまま残す
        // （FR-10、仕様決定 S。カードが消えずに新しい状態へ移行する）
        // continuedVideoId は実際に読み込みが始まった項目なので、
        // 定期の状態イベントを待たずに再生中項目だけはここで同期する
        // （遷移直後の操作が古い項目を拾わないように）
        if (ev.payload.continued) {
          const cur = playerStates.list.get(ev.payload.instanceId);
          if (cur && ev.payload.continuedVideoId) {
            const next = new Map(playerStates.list);
            next.set(ev.payload.instanceId, {
              ...cur,
              videoId: ev.payload.continuedVideoId,
            });
            playerStates.list = next;
          }
          return;
        }
        const next = new Map(playerStates.list);
        next.delete(ev.payload.instanceId);
        playerStates.list = next;
        formatOverrides.delete(ev.payload.instanceId);
      });
      // 手動 close は emitter が先に止まるため player://ended が来ない。
      // 代わりにバックエンドが出す player://closed で管理表から外す
      // （チャットポップアップ窓は自分で閉じる導線を持たないため、
      // このイベントだけがインスタンス消滅の検知手段になる。FR-27）
      await listen<{ instanceId: number }>("player://closed", (ev) => {
        inFlight.add(ev.payload.instanceId);
        const next = new Map(playerStates.list);
        next.delete(ev.payload.instanceId);
        playerStates.list = next;
        formatOverrides.delete(ev.payload.instanceId);
        dropSponsorSegments(ev.payload.instanceId);
        dropAbLoop(ev.payload.instanceId);
      });
      // sponsor.show_segments（FR-36、仕様決定 BI）。画面外の変更は
      // settings://changed で追従する。未設定・その他の値は on
      let showSegChanged = false;
      await listen<{ key: string; value: string }>(
        "settings://changed",
        (ev) => {
          if (ev.payload.key !== "sponsor.show_segments") return;
          showSegChanged = true;
          sponsorShowSegments.enabled = ev.payload.value !== "off";
        },
      );
      try {
        const raw = await invoke<string | null>("settings_get", {
          key: "sponsor.show_segments",
        });
        if (!showSegChanged) {
          sponsorShowSegments.enabled = raw !== "off";
        }
      } catch {
        // 読み取り失敗は既定 on のままにする
      }
      // リロード後はイベントが来ない一時停止中インスタンスがあるため、
      // 登録直後に一覧を取得してカードを復元する。
      const snapshot = await invoke<PlayerState[]>("player_list");
      const next = new Map(playerStates.list);
      for (const s of snapshot) {
        if (!inFlight.has(s.instanceId)) next.set(s.instanceId, s);
      }
      playerStates.list = next;
    })();
  }
  return initPromise;
}
