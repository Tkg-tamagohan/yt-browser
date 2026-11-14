// 履歴一覧のページング・検索絞り込み（FR-52・FR-61、仕様決定 CC・CL）の
// 純粋ロジックの回帰テスト。
// HL-01: 次ページの追記（末尾への追加と既存順の維持）
// HL-02: 追記時の video_id 重複除去
import { describe, expect, it } from "vitest";
import { mergeHistoryPage } from "./history-logic";

const item = (videoId: string) => ({ videoId });

describe("mergeHistoryPage", () => {
  it("HL-01 次ページを末尾へ追記し、既存の並びを維持する", () => {
    const current = [item("a"), item("b"), item("c")];
    const page = [item("d"), item("e")];
    expect(
      mergeHistoryPage(current, page).map((h) => h.videoId),
    ).toEqual(["a", "b", "c", "d", "e"]);
  });

  it("HL-02 追记分に既表示と同じ video_id があっても重複させない", () => {
    const current = [item("a"), item("b")];
    const page = [item("b"), item("c")];
    expect(
      mergeHistoryPage(current, page).map((h) => h.videoId),
    ).toEqual(["a", "b", "c"]);
  });
});
