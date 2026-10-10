// チャット表示行併合の回帰テスト（設計書 §6.2〜6.3、FR-27 で共通化）。
// CM-01: 表示フィルタと dedup（ng / other の除外、item_id の重複除外）
// CM-02: 削除イベントによる既表示行の打消し
// CM-03: 表示件数の上限（直近 500 件）
import { describe, expect, test } from "vitest";
import { mergeChatItems, type ChatItem } from "./chat-logic";
import type { ChatEvent } from "$lib/players.svelte";

function ev(partial: Partial<ChatEvent> & Pick<ChatEvent, "itemId">): ChatEvent {
  return {
    videoId: "v",
    postedAtUsec: 0,
    authorChannelId: null,
    authorName: null,
    kind: "text",
    message: "msg",
    amountDisplay: null,
    ng: false,
    ...partial,
  };
}

describe("CM-01 mergeChatItems の表示フィルタと dedup", () => {
  test("ng と other を除外し、同一 item_id の再送は捨てる", () => {
    const merged = mergeChatItems(
      [],
      [
        ev({ itemId: "a", message: "one" }),
        ev({ itemId: "b", ng: true, message: "ng" }),
        ev({ itemId: "c", kind: "other", message: "other" }),
        ev({ itemId: "a", message: "one" }),
        ev({ itemId: "d", message: "two" }),
      ],
    );
    expect(merged.map((i) => i.itemId)).toEqual(["a", "d"]);
  });

  test("既表示分と同一の item_id も追加しない", () => {
    const base = [ev({ itemId: "a" })];
    const merged = mergeChatItems(base, [ev({ itemId: "a" }), ev({ itemId: "b" })]);
    expect(merged.map((i) => i.itemId)).toEqual(["a", "b"]);
  });
});

describe("CM-02 mergeChatItems の削除打消し", () => {
  test("deleted イベントは対象行を消さず deleted フラグを立てる", () => {
    const base: ChatItem[] = [ev({ itemId: "a", message: "hi" })];
    const merged = mergeChatItems(base, [
      ev({ itemId: "x", kind: "deleted", message: "a" }),
    ]);
    expect(merged).toHaveLength(1);
    expect(merged[0].deleted).toBe(true);
  });

  test("対象が見つからない deleted は行を増やさない", () => {
    const merged = mergeChatItems(
      [ev({ itemId: "a" })],
      [ev({ itemId: "x", kind: "deleted", message: "missing" })],
    );
    expect(merged).toHaveLength(1);
    expect(merged[0].deleted).toBeUndefined();
  });
});

describe("CM-03 mergeChatItems の表示上限", () => {
  test("直近 500 件だけを残す", () => {
    const base = Array.from({ length: 500 }, (_, i) =>
      ev({ itemId: `old${i}` }),
    );
    const merged = mergeChatItems(base, [ev({ itemId: "new" })]);
    expect(merged).toHaveLength(500);
    expect(merged[0].itemId).toBe("old1");
    expect(merged[merged.length - 1].itemId).toBe("new");
  });
});
