// NG ユーザー節の行選択ロジックの回帰テスト（FR-70、仕様決定 CU）。
// NU-01: chat_author の NG 系種別（literal / regex）だけを選ぶ
// NU-02: highlight 種別の chat_author 行は NG ユーザーに含めない
import { describe, expect, test } from "vitest";
import { ngAuthorRows } from "./nguser-logic";
import type { Filter } from "$lib/players.svelte";

function f(id: number, target: string, kind: string, pattern = "p"): Filter {
  return {
    id,
    target,
    kind,
    pattern,
    enabled: true,
    createdAt: "2026-01-01 00:00:00",
  };
}

describe("NU-01 ngAuthorRows の対象選択", () => {
  test("chat_author の literal / regex 行だけを返却順のまま返す", () => {
    const rows = ngAuthorRows([
      f(1, "chat_author", "literal", "spam user"),
      f(2, "chat_text", "literal", "NG word"),
      f(3, "chat_author", "regex", "^bot"),
      f(4, "video_title", "literal", "clip"),
      f(5, "chat_author", "literal", "another"),
    ]);
    expect(rows.map((r) => r.id)).toEqual([1, 3, 5]);
  });

  test("chat_author 行が無いときは空になる", () => {
    expect(ngAuthorRows([f(1, "chat_text", "literal")])).toEqual([]);
    expect(ngAuthorRows([])).toEqual([]);
  });
});

describe("NU-02 ngAuthorRows の highlight 除外", () => {
  test("highlight 種別の chat_author 行は NG ユーザーに含めない", () => {
    const rows = ngAuthorRows([
      f(1, "chat_author", "highlight", "推しユーザー"),
      f(2, "chat_author", "literal", "spam"),
    ]);
    expect(rows.map((r) => r.id)).toEqual([2]);
  });
});
