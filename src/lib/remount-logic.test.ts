// 常時マウント化パネルの取り直しキー（FR-80、仕様決定 DE）の回帰テスト。
// RM-01: 表示中はキーを返し、非表示では null を返す
// RM-02: 開くたび・開いたままのキー変化でだけ取り直しが走り、
//        閉じている間のキー変化では取り直さない（前回値比較による駆動）
// RM-03: 固定キーなら開くたびだけ取り直す（開いたままでは再取得しない）
import { describe, expect, test } from "vitest";
import { panelFetchKey } from "./remount-logic";

// $effect での使い方と同じく、戻り値が前回と変わったときだけ取得する
// 駆動関数を組んで、開閉とキー変化の並びに対する取り直しを検証する
function driver() {
  const fetches: string[] = [];
  let last: string | null = null;
  return {
    fetches,
    step(visible: boolean, key: string): void {
      const k = panelFetchKey(visible, key);
      if (k === last) return;
      last = k;
      if (k !== null) fetches.push(k);
    },
  };
}

describe("RM-01 panelFetchKey の解決", () => {
  test("表示中はキーをそのまま返す", () => {
    expect(panelFetchKey(true, "vid1")).toBe("vid1");
  });

  test("非表示では null を返す", () => {
    expect(panelFetchKey(false, "vid1")).toBeNull();
  });
});

describe("RM-02 開閉とキー変化に連動した取り直し", () => {
  test("開くたび・開いたままのキー変化でだけ取得する", () => {
    const d = driver();
    d.step(false, "v1"); // 閉じたまま: 取らない
    d.step(true, "v1"); // 開く: v1 を取る
    d.step(true, "v1"); // 変化なし: 取らない
    d.step(true, "v2"); // 開いたままのキー変化: v2 を取る
    d.step(false, "v2"); // 閉じる
    d.step(false, "v3"); // 閉じている間のキー変化は無視
    d.step(true, "v3"); // 開き直し: v3 を取る
    d.step(false, "v3");
    d.step(true, "v3"); // 同じキーでも開き直せば取り直す
    expect(d.fetches).toEqual(["v1", "v2", "v3", "v3"]);
  });
});

describe("RM-03 固定キーの場合", () => {
  test("開くたびだけ取り直し、開いたままでは再取得しない", () => {
    const d = driver();
    d.step(true, "open"); // 開く: 取る
    d.step(true, "open"); // 開いたまま: 取らない
    d.step(false, "open"); // 閉じる
    d.step(true, "open"); // 開き直し: 取り直す
    expect(d.fetches).toEqual(["open", "open"]);
  });
});
