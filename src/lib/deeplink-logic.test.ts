// deep link の URL 解析の回帰テスト（FR-17、仕様決定 AC）。
// DL-01: `yt-browser://open?url=` のスキーム解析（受理形のバリエーション）
// DL-02: 対象 URL の振り分け（動画・プレイリスト・watch+list 複合・対象外）
// DL-03: `yt-browser://queue?url=` のキュー追加解析（動画のみ受理、FR-29）
// DL-04: `yt-browser://playlist_add` のプレイリスト追加解析（FR-50、仕様決定 CA）
import { describe, expect, test } from "vitest";
import { classifyTarget, parseOpenUrl } from "./deeplink.svelte";

const WATCH = "https://www.youtube.com/watch?v=dQw4w9WgXcQ";
const LIST = "https://www.youtube.com/playlist?list=PLtest12345";

describe("DL-01 parseOpenUrl", () => {
  test("open アクションの url パラメータを対象へ分解する", () => {
    const u = `yt-browser://open?url=${encodeURIComponent(WATCH)}`;
    expect(parseOpenUrl(u)).toEqual({
      kind: "video",
      videoId: "dQw4w9WgXcQ",
      startSec: null,
    });
  });

  test("argv 経由のホスト無し形も受理する", () => {
    for (const raw of [
      `yt-browser:open?url=${encodeURIComponent(LIST)}`,
      `yt-browser:///open?url=${encodeURIComponent(LIST)}`,
    ]) {
      expect(parseOpenUrl(raw)).toEqual({ kind: "playlist", url: LIST });
    }
  });

  test("open 以外のアクションと別スキームと壊れた URL は null", () => {
    expect(parseOpenUrl("yt-browser://play?url=x")).toBeNull();
    expect(parseOpenUrl("https://example.com")).toBeNull();
    expect(parseOpenUrl("not a url")).toBeNull();
    expect(parseOpenUrl("yt-browser://open")).toBeNull();
  });
});

describe("DL-02 classifyTarget", () => {
  test("watch は動画。短縮・shorts・live・music も動画", () => {
    expect(classifyTarget(WATCH)).toEqual({
      kind: "video",
      videoId: "dQw4w9WgXcQ",
      startSec: null,
    });
    expect(classifyTarget("https://youtu.be/dQw4w9WgXcQ")).toEqual({
      kind: "video",
      videoId: "dQw4w9WgXcQ",
      startSec: null,
    });
    expect(classifyTarget("https://www.youtube.com/shorts/dQw4w9WgXcQ")).toEqual(
      { kind: "video", videoId: "dQw4w9WgXcQ", startSec: null },
    );
    expect(
      classifyTarget("https://music.youtube.com/watch?v=dQw4w9WgXcQ"),
    ).toEqual({ kind: "video", videoId: "dQw4w9WgXcQ", startSec: null });
  });

  test("watch+list 複合は動画として扱う（仕様決定 AC）", () => {
    expect(
      classifyTarget(`${WATCH}&list=PLtest12345`),
    ).toEqual({ kind: "video", videoId: "dQw4w9WgXcQ", startSec: null });
  });

  test("t=/start= の開始位置を startSec として拾う", () => {
    expect(classifyTarget(`${WATCH}&t=2630s`)).toEqual({
      kind: "video",
      videoId: "dQw4w9WgXcQ",
      startSec: 2630,
    });
    expect(classifyTarget("https://youtu.be/dQw4w9WgXcQ?t=1h2m3s")).toEqual({
      kind: "video",
      videoId: "dQw4w9WgXcQ",
      startSec: 3723,
    });
    expect(classifyTarget(`${WATCH}&start=90`)).toEqual({
      kind: "video",
      videoId: "dQw4w9WgXcQ",
      startSec: 90,
    });
    // 解釈できない t は無視する
    expect(classifyTarget(`${WATCH}&t=abc`)).toEqual({
      kind: "video",
      videoId: "dQw4w9WgXcQ",
      startSec: null,
    });
  });

  test("playlist は取り込み対象", () => {
    expect(classifyTarget(LIST)).toEqual({ kind: "playlist", url: LIST });
  });

  test("対象外は null", () => {
    expect(classifyTarget("https://www.youtube.com/")).toBeNull();
    expect(classifyTarget("https://example.com/watch?v=dQw4w9WgXcQ")).toBeNull();
    expect(classifyTarget("https://www.youtube.com/watch?v=short")).toBeNull();
    expect(classifyTarget("ftp://www.youtube.com/watch?v=dQw4w9WgXcQ")).toBeNull();
  });
});

describe("DL-03 queue アクション（FR-29、仕様決定 AX）", () => {
  test("動画 URL をキュー対象として返す。watch+list 複合も動画扱い", () => {
    const q = (u: string) =>
      `yt-browser://queue?url=${encodeURIComponent(u)}`;
    expect(parseOpenUrl(q(WATCH))).toEqual({
      kind: "queue",
      videoId: "dQw4w9WgXcQ",
    });
    expect(parseOpenUrl(q(`${WATCH}&list=PLtest12345`))).toEqual({
      kind: "queue",
      videoId: "dQw4w9WgXcQ",
    });
    expect(parseOpenUrl(q("https://youtu.be/dQw4w9WgXcQ"))).toEqual({
      kind: "queue",
      videoId: "dQw4w9WgXcQ",
    });
  });

  test("プレイリスト単体と対象外 URL は null（unsupported 通知へ）", () => {
    const q = (u: string) =>
      `yt-browser://queue?url=${encodeURIComponent(u)}`;
    expect(parseOpenUrl(q(LIST))).toBeNull();
    expect(parseOpenUrl(q("https://example.com/watch?v=dQw4w9WgXcQ"))).toBeNull();
    expect(parseOpenUrl("yt-browser://queue")).toBeNull();
  });

  test("t= の開始位置はキュー項目へ持ち越さない（仕様決定 AX）", () => {
    const q = (u: string) =>
      `yt-browser://queue?url=${encodeURIComponent(u)}`;
    expect(parseOpenUrl(q(`${WATCH}&t=2630s`))).toEqual({
      kind: "queue",
      videoId: "dQw4w9WgXcQ",
    });
  });
});

describe("DL-04 playlist_add アクション（FR-50、仕様決定 CA）", () => {
  const q = (u: string, extra = "") =>
    `yt-browser://playlist_add?url=${encodeURIComponent(u)}${extra}`;

  test("動画 URL と playlist_id をプレイリスト追加対象として返す", () => {
    expect(parseOpenUrl(q(WATCH, "&playlist_id=7"))).toEqual({
      kind: "playlist_add",
      videoId: "dQw4w9WgXcQ",
      playlistId: 7,
    });
    // watch+list 複合も動画扱い（対象動画をプレイリストへ入れる）
    expect(
      parseOpenUrl(q(`${WATCH}&list=PLtest12345`, "&playlist_id=3")),
    ).toEqual({ kind: "playlist_add", videoId: "dQw4w9WgXcQ", playlistId: 3 });
    expect(parseOpenUrl(q("https://youtu.be/dQw4w9WgXcQ", "&playlist_id=1"))).toEqual(
      { kind: "playlist_add", videoId: "dQw4w9WgXcQ", playlistId: 1 },
    );
  });

  test("playlist_id が無い・非整数・0 以下なら null（unsupported 通知へ）", () => {
    for (const extra of [
      "",
      "&playlist_id=",
      "&playlist_id=abc",
      "&playlist_id=0",
      "&playlist_id=-2",
      "&playlist_id=1.5",
    ]) {
      expect(parseOpenUrl(q(WATCH, extra)), `extra=${extra}`).toBeNull();
    }
  });

  test("プレイリスト単体と対象外 URL は null（動画のみ受理）", () => {
    expect(parseOpenUrl(q(LIST, "&playlist_id=7"))).toBeNull();
    expect(
      parseOpenUrl(q("https://example.com/watch?v=dQw4w9WgXcQ", "&playlist_id=7")),
    ).toBeNull();
  });
});
