// 動画行コンテキストメニューの表示位置の回帰テスト（FR-78、仕様決定 DC）。
// VC-01: クリック位置をそのまま左上へ置く（収まる場合）
// VC-02: 右端・下端のはみ出しは内側へ折り返す
// VC-03: メニューがビューポートより大きい場合は端へ寄せる（負座標にしない）
import { describe, expect, test } from "vitest";
import {
  contextMenuPos,
  CTX_MENU_HEIGHT,
  CTX_MENU_MARGIN,
  CTX_MENU_WIDTH,
} from "./context-menu-logic";

describe("VC-01 contextMenuPos の基本配置", () => {
  test("収まるクリック位置はそのまま左上になる", () => {
    const vw = 1280;
    const vh = 800;
    expect(contextMenuPos(100, 200, vw, vh)).toEqual({ x: 100, y: 200 });
    expect(contextMenuPos(0, 0, vw, vh)).toEqual({ x: 0, y: 0 });
    // ちょうどメニュー＋余白が収まる位置もそのまま
    expect(
      contextMenuPos(vw - CTX_MENU_WIDTH - CTX_MENU_MARGIN, vh - CTX_MENU_HEIGHT - CTX_MENU_MARGIN, vw, vh),
    ).toEqual({
      x: vw - CTX_MENU_WIDTH - CTX_MENU_MARGIN,
      y: vh - CTX_MENU_HEIGHT - CTX_MENU_MARGIN,
    });
  });
});

describe("VC-02 contextMenuPos の端折り返し", () => {
  const vw = 1280;
  const vh = 800;

  test("右端をはみ出すクリックは内側へ寄せる", () => {
    const pos = contextMenuPos(vw - 10, 200, vw, vh);
    expect(pos).toEqual({ x: vw - CTX_MENU_WIDTH - CTX_MENU_MARGIN, y: 200 });
  });

  test("下端をはみ出すクリックは内側へ寄せる", () => {
    const pos = contextMenuPos(100, vh - 5, vw, vh);
    expect(pos).toEqual({ x: 100, y: vh - CTX_MENU_HEIGHT - CTX_MENU_MARGIN });
  });

  test("右下の角では両軸を折り返す", () => {
    const pos = contextMenuPos(vw, vh, vw, vh);
    expect(pos).toEqual({
      x: vw - CTX_MENU_WIDTH - CTX_MENU_MARGIN,
      y: vh - CTX_MENU_HEIGHT - CTX_MENU_MARGIN,
    });
  });
});

describe("VC-03 contextMenuPos の極小ビューポート", () => {
  test("メニューがビューポートより大きい場合は座標を 0 に留める", () => {
    // メニュー（W=260/H=320 既定）より小さい窓では折り返し先が
    // 負にならない
    const pos = contextMenuPos(300, 300, 200, 200);
    expect(pos).toEqual({ x: 0, y: 0 });
    // 片側だけ狭い場合はその軸のみ 0
    const pos2 = contextMenuPos(300, 10, 200, 800);
    expect(pos2).toEqual({ x: 0, y: 10 });
  });
});
