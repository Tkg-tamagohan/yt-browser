// ライブ配信の待機（FR-44、仕様決定 BT）の純粋ロジックの回帰テスト。
// LW-01: 放送状態の分類（放送中 > 終了済み > 開始前 > 非放送の優先順位）
// LW-02: ポーリング間隔（直前窓の短間隔・窓外と予定無しの定間隔）
// LW-03: 24 時間失効（予定時刻起点・予定無しは登録時刻起点）
// LW-04: live_wait.action の解釈（notify のみ notify、他は play）
// LW-05: 待機判定と一覧の並び（予定時刻昇順・未定は末尾・登録時刻でたいブレイク）
// LW-06: 登録結果 → 通知キーの写像
import { describe, expect, test } from "vitest";
import {
  classifyWaitBroadcast,
  isWaitExpired,
  isWaiting,
  liveWaitCompare,
  nextWaitPollMs,
  waitActionOf,
  waitAddOutcomeKey,
  WAIT_NEAR_AFTER_MS,
  WAIT_NEAR_BEFORE_MS,
  WAIT_POLL_BASE_MS,
  WAIT_POLL_SHORT_MS,
  type BroadcastStatus,
  type LiveWait,
} from "./livewait-logic";

/// テスト用の放送状態。既定は開始前（予定あり）で個別に上書きする
function st(partial: Partial<BroadcastStatus> = {}): BroadcastStatus {
  return {
    isLiveNow: false,
    startMs: null,
    endMs: null,
    scheduledStartMs: null,
    hasDetails: true,
    isUpcoming: true,
    ...partial,
  };
}

describe("LW-01 classifyWaitBroadcast", () => {
  test("isLiveNow 真は放送中（endTimestamp 混在の矛盾形でも放送中を優先）", () => {
    expect(classifyWaitBroadcast(st({ isLiveNow: true }))).toEqual({
      kind: "live",
    });
    expect(
      classifyWaitBroadcast(st({ isLiveNow: true, endMs: 1_000 })),
    ).toEqual({ kind: "live" });
  });

  test("isLiveNow が偽・未判定で endTimestamp ありは終了済み", () => {
    expect(classifyWaitBroadcast(st({ endMs: 1_000 }))).toEqual({
      kind: "ended",
    });
    expect(
      classifyWaitBroadcast(st({ isLiveNow: null, endMs: 1_000 })),
    ).toEqual({ kind: "ended" });
  });

  test("details あり・終了無しは開始前（予定は scheduledStart 優先）", () => {
    expect(
      classifyWaitBroadcast(
        st({ startMs: 1_000, scheduledStartMs: 2_000 }),
      ),
    ).toEqual({ kind: "upcoming", scheduledMs: 2_000 });
    // 実応答では startTimestamp に予定が入る（scheduledStartTimestamp 無し）
    expect(classifyWaitBroadcast(st({ startMs: 1_000 }))).toEqual({
      kind: "upcoming",
      scheduledMs: 1_000,
    });
    // 予定無しの開始前（details だけある形）
    expect(classifyWaitBroadcast(st({ isUpcoming: false }))).toEqual({
      kind: "upcoming",
      scheduledMs: null,
    });
  });

  test("details 無し・isUpcoming 偽は非放送（通常動画）", () => {
    expect(
      classifyWaitBroadcast(
        st({ hasDetails: false, isUpcoming: false }),
      ),
    ).toEqual({ kind: "notBroadcast" });
  });

  test("details 無しでも isUpcoming 真は開始前（details 取りこぼし形）", () => {
    expect(
      classifyWaitBroadcast(st({ hasDetails: false, isUpcoming: true })),
    ).toEqual({ kind: "upcoming", scheduledMs: null });
  });
});

describe("LW-02 nextWaitPollMs", () => {
  const scheduled = 10_000_000;

  test("直前窓内は短間隔（手前側と後側の両端を含む）", () => {
    expect(nextWaitPollMs(scheduled - WAIT_NEAR_BEFORE_MS, scheduled)).toBe(
      WAIT_POLL_SHORT_MS,
    );
    expect(nextWaitPollMs(scheduled, scheduled)).toBe(WAIT_POLL_SHORT_MS);
    expect(nextWaitPollMs(scheduled + WAIT_NEAR_AFTER_MS, scheduled)).toBe(
      WAIT_POLL_SHORT_MS,
    );
  });

  test("直前窓の外と予定無しは定間隔（仕様決定 BT「60 秒程度」）", () => {
    expect(
      nextWaitPollMs(scheduled - WAIT_NEAR_BEFORE_MS - 1, scheduled),
    ).toBe(WAIT_POLL_BASE_MS);
    expect(
      nextWaitPollMs(scheduled + WAIT_NEAR_AFTER_MS + 1, scheduled),
    ).toBe(WAIT_POLL_BASE_MS);
    expect(nextWaitPollMs(scheduled, null)).toBe(WAIT_POLL_BASE_MS);
  });
});

describe("LW-03 isWaitExpired", () => {
  const DAY = 24 * 60 * 60_000;

  test("予定時刻から 24 時間超過で失効（境界の等号は残す）", () => {
    const scheduled = 10_000_000;
    expect(isWaitExpired(scheduled + DAY, scheduled, 0)).toBe(false);
    expect(isWaitExpired(scheduled + DAY + 1, scheduled, 0)).toBe(true);
  });

  test("予定無しは登録時刻を起点にする（暫定解釈）", () => {
    const registered = 5_000_000;
    expect(isWaitExpired(registered + DAY, null, registered)).toBe(false);
    expect(isWaitExpired(registered + DAY + 1, null, registered)).toBe(true);
  });
});

describe("LW-04 waitActionOf", () => {
  test("notify のみ notify、未設定・その他・play は play（既定）", () => {
    expect(waitActionOf("notify")).toBe("notify");
    expect(waitActionOf("play")).toBe("play");
    expect(waitActionOf(null)).toBe("play");
    expect(waitActionOf("")).toBe("play");
    expect(waitActionOf("bogus")).toBe("play");
  });
});

describe("LW-05 isWaiting / liveWaitCompare", () => {
  function w(
    videoId: string,
    scheduledStartMs: number | null,
    registeredMs: number,
  ): LiveWait {
    return {
      videoId,
      title: videoId,
      channelId: null,
      channelTitle: null,
      thumbnailUrl: null,
      scheduledStartMs,
      registeredMs,
    };
  }

  test("video_id の一致で待機中を判定する（重複抑止の表示側）", () => {
    const list = [w("a", 1, 1), w("b", null, 1)];
    expect(isWaiting(list, "a")).toBe(true);
    expect(isWaiting(list, "b")).toBe(true);
    expect(isWaiting(list, "c")).toBe(false);
  });

  test("予定時刻の昇順・未定は末尾・同時刻は登録時刻昇順", () => {
    const sorted = [
      w("late", 3_000, 1),
      w("none", null, 1),
      w("early", 1_000, 2),
      w("same-new", 2_000, 2),
      w("same-old", 2_000, 1),
    ].sort(liveWaitCompare);
    expect(sorted.map((x) => x.videoId)).toEqual([
      "early",
      "same-old",
      "same-new",
      "late",
      "none",
    ]);
  });
});

describe("LW-06 waitAddOutcomeKey", () => {
  test("結果区分ごとの通知キー", () => {
    expect(waitAddOutcomeKey("added")).toBe("livewait.added");
    expect(waitAddOutcomeKey("duplicate")).toBe("livewait.duplicate");
    expect(waitAddOutcomeKey("live")).toBe("livewait.live");
    expect(waitAddOutcomeKey("ended")).toBe("livewait.ended");
    expect(waitAddOutcomeKey("notBroadcast")).toBe("livewait.notBroadcast");
  });
});
