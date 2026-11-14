// A-B リピート状態機械の回帰テスト（FR-37、仕様決定 BM）。
// AB-01: ボタン押下の 3 状態巡回と送出アクションの対応
// AB-02: 動画 ID 束ね — 別動画のエントリは off として解決する
import { describe, expect, test } from "vitest";
import { abAdvance, abStateFor, type AbEntry } from "./ab-logic";

describe("AB-01 abAdvance の状態巡回", () => {
  test("off → a → loop → off の順に遷移とアクションが対応する", () => {
    const s1 = abAdvance("off");
    expect(s1).toEqual({ next: "a", action: { type: "ab_loop_set_a" } });

    const s2 = abAdvance("a");
    expect(s2).toEqual({ next: "loop", action: { type: "ab_loop_set_b" } });

    const s3 = abAdvance("loop");
    expect(s3).toEqual({ next: "off", action: { type: "ab_loop_clear" } });
  });
});

describe("AB-02 abStateFor の動画 ID 束ね", () => {
  const looping: AbEntry = { videoId: "aaa", state: "loop" };

  test("同じ動画ならエントリの状態をそのまま返す", () => {
    expect(abStateFor(looping, "aaa")).toBe("loop");
    expect(abStateFor({ videoId: "aaa", state: "a" }, "aaa")).toBe("a");
  });

  test("別動画へ切り替わると off として扱う", () => {
    expect(abStateFor(looping, "bbb")).toBe("off");
  });

  test("エントリ未設定は off", () => {
    expect(abStateFor(undefined, "aaa")).toBe("off");
  });
});
