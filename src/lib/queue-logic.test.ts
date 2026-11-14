// 連続再生キュー・ループの純粋ロジックの回帰テスト（FR-16、仕様決定 AA・AD・BK）。
// LP-01: 武装キューの選定（1 項目は現在項目のみ、全体は回転順、なしは次項目以降）
// LP-02: 終了イベントの継続項目 uid によるキュー位置の照合（BG-10）
// LP-05: 同一動画の連続登録でも uid 照合で位置がずれないことの回帰（BG-10）
// LP-03: キュースタック編集（削除・移動）に伴う再生位置の調整（FR-20）
// MQ-01: 複数キューの終端処置（eof 消費完了で残す／それ以外で消滅）（FR-26）
// MQ-02: 「連続再生を止める」後のキュー位置（一時は先頭・プレイリストは保存）（FR-26）
// LP-04: 一時キュー永続化の項目変換と復元位置（FR-30、仕様決定 AY）
// QM-01: キュー表示メタのフォールバック判定（FR-29、仕様決定 BD）
// QS-01: 「プレイリストとして保存」の項目列変換（FR-47、仕様決定 BX）
// QS-02: プレイリストキュー復元の保存位置（FR-48、仕様決定 BY）
import { describe, expect, test } from "vitest";
import {
  armPlanFor,
  headPlayingAfterRemove,
  indexAfterMove,
  indexAfterRemove,
  indexAfterStop,
  isFallbackTitle,
  persistItemsOf,
  queueEndOutcome,
  reconcileIndex,
  restoredIndex,
  restoredPlaylistIndex,
  saveItemsOf,
  type QueueEntry,
} from "./queue-logic";

/// テスト用のキュー項目。uid を明示して同一動画の重複も表せる
function e(uid: number, videoId: string): QueueEntry {
  return { uid, videoId };
}

/// videoId 列から連番 uid の項目列を作る短縮形
function es(...videoIds: string[]): QueueEntry[] {
  return videoIds.map((videoId, i) => e(i + 1, videoId));
}

/// QueueEntry 列を videoId 列へ（armed 計画の比較用）
function vids(items: QueueEntry[]): string[] {
  return items.map((i) => i.videoId);
}

describe("LP-01 armPlanFor", () => {
  const items = es("A", "B", "C");

  test("none は次項目以降を順に登録し、末尾では空列（武装解除）", () => {
    expect(armPlanFor("none", items, 0)).toEqual({ items: items.slice(1), loop: false });
    expect(armPlanFor("none", items, 1)).toEqual({ items: items.slice(2), loop: false });
    expect(armPlanFor("none", items, 2)).toEqual({ items: [], loop: false });
  });

  test("all は次項目以降＋先頭〜現在項目の回転順を巡回登録する", () => {
    // [A]再生中: 今後は B,C,A,B,C,... と巡回する（B→C→A→B の回転）
    expect(armPlanFor("all", items, 0)).toEqual({
      items: [items[1], items[2], items[0]],
      loop: true,
    });
    // [C]再生中（末尾）: 今後は A,B,C,A,... と先頭へ巻き戻る
    expect(armPlanFor("all", items, 2)).toEqual({ items, loop: true });
    // 1 項目だけのキューでも回転順＝現在項目のみ（繰り返し）
    expect(armPlanFor("all", es("A"), 0)).toEqual({
      items: [e(1, "A")],
      loop: true,
    });
  });

  test("one は常に現在項目のみを巡回登録する（キュー中も次へ進まない）", () => {
    expect(armPlanFor("one", items, 0)).toEqual({ items: [items[0]], loop: true });
    expect(armPlanFor("one", items, 2)).toEqual({ items: [items[2]], loop: true });
  });

  test("indexIsPlaying=false は index 位置の未再生項目から武装する", () => {
    // [A,B,C] の B(index=1)再生中に B を削除 → [A,C] index=1 は未再生の C。
    // 武装から C を外すと B 終了後に何も再生されず C が飛ばされる（指摘の回帰）
    expect(vids(armPlanFor("none", es("A", "C"), 1, false).items)).toEqual(["C"]);
    // 先頭の再生中を削除 → [B,C] index=0 は未再生の B から
    expect(vids(armPlanFor("none", es("B", "C"), 0, false).items)).toEqual([
      "B",
      "C",
    ]);
    // 全体ループは index 位置を含めた回転順
    expect(vids(armPlanFor("all", es("A", "C"), 1, false).items)).toEqual([
      "C",
      "A",
    ]);
    // one は index 位置（未再生の次項目）を巡回対象にする
    expect(vids(armPlanFor("one", es("A", "C"), 1, false).items)).toEqual([
      "C",
    ]);
  });

  test("武装項目は uid＋videoId の対のまま渡される", () => {
    // backend の QueueArmItem と同じ形で送る（仕様決定 BK）
    expect(armPlanFor("none", items, 0).items).toEqual([
      { uid: 2, videoId: "B" },
      { uid: 3, videoId: "C" },
    ]);
  });
});

describe("LP-02 reconcileIndex", () => {
  const items = es("A", "B", "C");

  test("継続項目が現在項目なら繰り返し（位置据え置き）", () => {
    expect(reconcileIndex(items, 0, 1)).toBe(0);
    expect(reconcileIndex(items, 2, 3)).toBe(2);
  });

  test("継続項目が次項目なら 1 つ進める", () => {
    expect(reconcileIndex(items, 0, 2)).toBe(1);
    expect(reconcileIndex(items, 1, 3)).toBe(2);
  });

  test("末尾で先頭項目が来たら全体ループの巻き戻り（0 へ）", () => {
    expect(reconcileIndex(items, 2, 1)).toBe(0);
  });

  test("遷移中のモード変更で続きが繰り返しでも位置を飛ばさない", () => {
    // one → all に遷移と同時に切り替わった場合: 実際に読まれたのは A
    // （モードではなく continuedUid で照合する回帰）
    expect(reconcileIndex(items, 0, 1)).toBe(0);
    // 逆に all → one 中に次項目が読まれたら前進する
    expect(reconcileIndex(items, 0, 2)).toBe(1);
  });

  test("一覧に無い uid（編集で消えた武装項目）では位置を変えない", () => {
    expect(reconcileIndex(items, 1, 99)).toBe(1);
    expect(reconcileIndex(items, 1, null)).toBe(1);
  });
});

describe("LP-05 uid 照合の回帰（BG-10、仕様決定 BK）", () => {
  test("同一動画の連続登録で継続先が区別できる", () => {
    // [A,A,B] の 1 番目の A(index=0) から 2 番目の A(uid=2) へ進む。
    // video_id 基準では両方 A で区別できず位置がずれていた旧来の制約
    const items = [e(11, "A"), e(12, "A"), e(13, "B")];
    expect(reconcileIndex(items, 0, 12)).toBe(1);
    // 逆方向（先に後ろ側の項目が読まれた場合）も位置を飛ばさない
    expect(reconcileIndex(items, 0, 11)).toBe(0);
  });

  test("同一動画が離れて並んでいても uid の項目そのものへ照合する", () => {
    // [A,B,A] index=1(B) から末尾の A(uid=13) へ進む遷移。
    // video_id 照合では先頭の A に誤照合して巻き戻り扱いになっていた
    const items = [e(11, "A"), e(12, "B"), e(13, "A")];
    expect(reconcileIndex(items, 1, 13)).toBe(2);
    // 先頭の A(uid=11) が読まれた全体ループ巻き戻りも正しく 0 へ
    expect(reconcileIndex(items, 2, 11)).toBe(0);
  });

  test("削除済み項目の uid が届いても残った同 videoId 項目に誤照合しない", () => {
    // [A,A] の 1 番目を削除して [A(uid=12)] だけ残った状態で、
    // 削除側 uid=11 の終了が届く。video_id 照合なら位置が
    // ずれていたが、uid 照合では見つからず据え置きになる
    const items = [e(12, "A")];
    expect(reconcileIndex(items, 0, 11)).toBe(0);
    expect(reconcileIndex(items, 0, 12)).toBe(0);
  });
});

describe("LP-03 キュースタック編集の位置調整（FR-20）", () => {
  test("indexAfterRemove: 再生中より前を消したら 1 つ前へ", () => {
    expect(indexAfterRemove(2, 0, 3)).toBe(1);
    expect(indexAfterRemove(2, 1, 3)).toBe(1);
  });

  test("indexAfterRemove: 再生中・より後ろを消しても据え置き", () => {
    expect(indexAfterRemove(1, 1, 3)).toBe(1);
    expect(indexAfterRemove(1, 2, 3)).toBe(1);
  });

  test("indexAfterRemove: 末尾の再生中を消したら新末尾へ収める", () => {
    expect(indexAfterRemove(2, 2, 2)).toBe(1);
    expect(indexAfterRemove(0, 0, 0)).toBe(0);
  });

  test("indexAfterMove: 再生中項目の移動は位置も追従する", () => {
    expect(indexAfterMove(1, 1, 3)).toBe(3);
    expect(indexAfterMove(3, 3, 0)).toBe(0);
  });

  test("indexAfterMove: 再生中をまたぐ移動は位置をずらす", () => {
    // [A,B,C,D] index=2(C) で A→2: [B,C,A,D] → C は 1 へ
    expect(indexAfterMove(2, 0, 2)).toBe(1);
    // 同じく D→0: [D,A,B,C] → C は 3 へ
    expect(indexAfterMove(2, 3, 0)).toBe(3);
    // 再生中をまたがない移動は据え置き
    expect(indexAfterMove(2, 0, 1)).toBe(2);
    expect(indexAfterMove(2, 3, 3)).toBe(2);
  });

  test("headPlayingAfterRemove: 現位置削除で index は未再生の次項目を指す", () => {
    // 再生中(index=1)を削除 → 次項目が残るので未再生扱い（false）
    expect(headPlayingAfterRemove(true, 1, 1, 2)).toBe(false);
    // 末尾の再生中(index=2)を削除 → 次項目は無く index は消費済みへ戻る
    expect(headPlayingAfterRemove(true, 2, 2, 2)).toBe(true);
    // 全項目の消去（newLen=0）も再生中扱いのまま（武装は空になる）
    expect(headPlayingAfterRemove(true, 0, 0, 0)).toBe(true);
  });

  test("headPlayingAfterRemove: 別位置の削除は状態を維持する", () => {
    // 再生中より前・後ろを消しても再生中のまま
    expect(headPlayingAfterRemove(true, 2, 0, 3)).toBe(true);
    expect(headPlayingAfterRemove(true, 1, 2, 3)).toBe(true);
    // 未再生の次項目（index 位置）以外を消しても未再生扱いのまま
    expect(headPlayingAfterRemove(false, 1, 0, 2)).toBe(false);
  });
});

describe("MQ-01 queueEndOutcome（FR-26、仕様決定 AS）", () => {
  const items = es("A", "B", "C");

  test("一時キューは終端理由に関わらず消滅する", () => {
    // 消費完了（末尾の eof）でも消滅するのが一時キューの現行仕様
    expect(queueEndOutcome(null, "eof", items, 2, "C")).toBe("destroy");
    expect(queueEndOutcome(null, "stop", items, 1, "B")).toBe("destroy");
    expect(queueEndOutcome(null, "error", items, 0, "A")).toBe("destroy");
  });

  test("プレイリストキューは末尾項目の eof 終了で残す（消費完了）", () => {
    expect(queueEndOutcome(7, "eof", items, 2, "C")).toBe("reset");
  });

  test("プレイリストキューは eof 以外の終端で消滅する（インスタンス消失）", () => {
    for (const reason of ["quit", "stop", "error", "process_exit"]) {
      // index が末尾項目でも理由が eof でなければ消失として畳む
      expect(queueEndOutcome(7, reason, items, 2, "C")).toBe("destroy");
    }
  });

  test("eof でも途中項目の終了は消費完了でなく消失として畳む", () => {
    // [A,B,C] の B(index=1)終了後に継続先 C の読み込みが失敗すると
    // 同じ eof＋非継続で届く。末尾まで進んでいないためキューは消滅させる
    expect(queueEndOutcome(7, "eof", items, 1, "B")).toBe("destroy");
    expect(queueEndOutcome(7, "eof", items, 0, "A")).toBe("destroy");
  });

  test("位置が末尾でも終了項目が末尾でなければ消失として畳む", () => {
    // 再生中の末尾項目をキューから消した直後に旧項目が終わり、
    // 継続先の読み込みに失敗した経路。終了した項目自体は既に一覧に無い
    expect(queueEndOutcome(7, "eof", es("A", "C"), 1, "B")).toBe("destroy");
  });
});

describe("MQ-02 indexAfterStop（FR-26、仕様決定 AS）", () => {
  test("一時キューは項目だけ残して位置を先頭に戻す", () => {
    expect(indexAfterStop(null, 3)).toBe(0);
    expect(indexAfterStop(null, 0)).toBe(0);
  });

  test("プレイリストキューは現在位置を保つ（パネルの再生で保存位置から再開）", () => {
    expect(indexAfterStop(7, 3)).toBe(3);
    expect(indexAfterStop(7, 0)).toBe(0);
  });
});

describe("LP-04 一時キュー永続化の項目変換と復元位置（FR-30、仕様決定 AY）", () => {
  const META = new Map([
    ["A", { title: "タイトルA", channelTitle: "ch", thumbnailUrl: "t" }],
    ["B", { title: "", channelTitle: null, thumbnailUrl: null }],
  ]);
  const metaOf = (id: string) =>
    META.get(id) ?? { title: id, channelTitle: null, thumbnailUrl: null };

  test("items からメタ付き項目列を作る。メタ無し・空タイトルは videoId に潰す", () => {
    expect(persistItemsOf(es("A", "B", "C"), metaOf)).toEqual([
      { uid: 1, videoId: "A", title: "タイトルA", channelTitle: "ch", thumbnailUrl: "t" },
      { uid: 2, videoId: "B", title: "B", channelTitle: null, thumbnailUrl: null },
      { uid: 3, videoId: "C", title: "C", channelTitle: null, thumbnailUrl: null },
    ]);
    expect(persistItemsOf([], metaOf)).toEqual([]);
  });

  test("保存位置は項目数に収める。空は 0、末尾超過は末尾、負数は 0", () => {
    expect(restoredIndex(0, 3)).toBe(0);
    expect(restoredIndex(3, 5)).toBe(2);
    expect(restoredIndex(3, -1)).toBe(0);
    expect(restoredIndex(3, 1)).toBe(1);
  });
});

describe("QM-01 isFallbackTitle（FR-29、仕様決定 BD）", () => {
  test("タイトルが動画 ID と同一または空ならフォールバック", () => {
    expect(isFallbackTitle("abcdefghijk", "abcdefghijk")).toBe(true);
    expect(isFallbackTitle("", "abcdefghijk")).toBe(true);
  });

  test("実タイトルはフォールバックでない（先勝ちで維持する）", () => {
    expect(isFallbackTitle("動画タイトル", "abcdefghijk")).toBe(false);
    // 別動画の ID はこの項目にとっては実タイトル扱い（上書きしない）
    expect(isFallbackTitle("xyz98765432", "abcdefghijk")).toBe(false);
  });
});

describe("QS-01 saveItemsOf（FR-47、仕様決定 BX）", () => {
  // キュー表示メタの形に合わせた解決関数（queueMetaOf と同じ形）
  const metaOf = (map: Record<string, { title: string; channelTitle: string | null; thumbnailUrl: string | null }>) =>
    (videoId: string) =>
      map[videoId] ?? { title: videoId, channelTitle: null, thumbnailUrl: null };

  test("順序・重複・メタをそのまま VideoRef 列へ写す", () => {
    const items = es("A", "B", "A", "C");
    const out = saveItemsOf(
      items,
      metaOf({
        A: { title: "動画A", channelTitle: "CH-A", thumbnailUrl: "thumb-a" },
        C: { title: "動画C", channelTitle: null, thumbnailUrl: null },
      }),
    );
    // 順序と同一動画の重複は保持する（重複可否はバックエンドの主キー側の判断）
    expect(out.map((i) => i.videoId)).toEqual(["A", "B", "A", "C"]);
    expect(out[0]).toEqual({
      videoId: "A",
      title: "動画A",
      channelId: null,
      channelTitle: "CH-A",
      thumbnailUrl: "thumb-a",
    });
    // メタ未登録は videoId をタイトルに潰し、残りは null
    expect(out[1]).toEqual({
      videoId: "B",
      title: "B",
      channelId: null,
      channelTitle: null,
      thumbnailUrl: null,
    });
    // キュー項目の uid は複製へ持ち越さない（新規採番されるもの。BX(2)）
    expect(out[2]).toEqual(out[0]);
    // キューの表示メタは channel_id を持たないため常に null
    expect(out.every((i) => i.channelId === null)).toBe(true);
  });
});

describe("QS-02 restoredPlaylistIndex（FR-48、仕様決定 BY）", () => {
  test("保存位置は項目数に収める（一時キュー復元と同じ規則）", () => {
    expect(restoredPlaylistIndex(3, 5)).toBe(3);
    expect(restoredPlaylistIndex(9, 5)).toBe(4);
    expect(restoredPlaylistIndex(-1, 5)).toBe(0);
    expect(restoredPlaylistIndex(0, 1)).toBe(0);
  });

  test("対象プレイリストが空（0 件）なら null（復元しない）", () => {
    expect(restoredPlaylistIndex(0, 0)).toBeNull();
    expect(restoredPlaylistIndex(3, 0)).toBeNull();
  });
});
