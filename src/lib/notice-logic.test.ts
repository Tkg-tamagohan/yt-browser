// 通知履歴の保持ロジックの回帰テスト（FR-83、仕様決定 DH）。
// NH-01: pushNoticeHistory の追加順序と上限超過時の古い順の削除
// NH-02: historyDesc の新しい順ソートと非破壊性
// NH-03: formatNoticeAt の HH:MM:SS 形式
import { describe, expect, test } from "vitest";
import {
  formatNoticeAt,
  historyDesc,
  NOTICE_HISTORY_MAX,
  pushNoticeHistory,
  type NoticeHistoryEntry,
} from "./notice-logic";

function entry(id: number): NoticeHistoryEntry {
  return { id, msg: `m${id}`, at: id };
}

describe("NH-01 pushNoticeHistory の追加と上限", () => {
  test("発行順に末尾へ追加する", () => {
    const h: NoticeHistoryEntry[] = [];
    pushNoticeHistory(h, entry(1));
    pushNoticeHistory(h, entry(2));
    expect(h.map((n) => n.id)).toEqual([1, 2]);
  });

  test("上限を超えると古い方から落ちる（暫定の上限数）", () => {
    const h: NoticeHistoryEntry[] = [];
    for (let i = 1; i <= NOTICE_HISTORY_MAX + 3; i++) {
      pushNoticeHistory(h, entry(i));
    }
    expect(h).toHaveLength(NOTICE_HISTORY_MAX);
    expect(h[0].id).toBe(4);
    expect(h.at(-1)?.id).toBe(NOTICE_HISTORY_MAX + 3);
  });

  test("アクションはラベルのみ保持しコールバックは持たない", () => {
    const h = pushNoticeHistory([], {
      id: 1,
      msg: "m",
      at: 1,
      actionLabel: "フォルダを開く",
    });
    expect(h[0].actionLabel).toBe("フォルダを開く");
    expect("run" in h[0]).toBe(false);
    expect("action" in h[0]).toBe(false);
  });
});

describe("NH-02 historyDesc の表示順", () => {
  test("新しいものを先頭にし、元配列は変更しない", () => {
    const h = [entry(1), entry(2), entry(3)];
    const desc = historyDesc(h);
    expect(desc.map((n) => n.id)).toEqual([3, 2, 1]);
    expect(h.map((n) => n.id)).toEqual([1, 2, 3]);
  });
});

describe("NH-03 formatNoticeAt の時刻表示", () => {
  test("HH:MM:SS の 0 埋め形式になる", () => {
    const d = new Date(2026, 0, 2, 3, 4, 5);
    expect(formatNoticeAt(d.getTime())).toBe("03:04:05");
  });
});
