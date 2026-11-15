// 動画行コンテキストメニューの表示位置の回帰テスト（FR-78、仕様決定 DC）。
// VC-01: クリック位置をそのまま左上へ置く（収まる場合）
// VC-02: 右端・下端のはみ出しは内側へ折り返す
// VC-03: メニューがビューポートより大きい場合は端へ寄せる（負座標にしない）
// VC-04: メニュー終了時にリアクティブな状態が null 化されても追加後コールバックを落とさない
// VC-05: 上端位置 y とビューポート高さから下端余白込みの最大高さを算出する
import { describe, expect, test, vi } from "vitest";
import {
  contextMenuMaxHeight,
  contextMenuPos,
  CTX_MENU_HEIGHT,
  CTX_MENU_MARGIN,
  CTX_MENU_WIDTH,
  finishPlaylistAdd,
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

describe("VC-04 finishPlaylistAdd のコールバック維持", () => {
  test("onclose 実行後にリアクティブ参照が null になっても onplaylistadd を呼び出す", () => {
    const order: string[] = [];
    const onAdd = vi.fn((playlistId: number) => {
      order.push(`add:${playlistId}`);
    });
    const state: {
      current: {
        onclose: () => void;
        onplaylistadd: (playlistId: number) => void;
      } | null;
    } = {
      current: {
        onclose: () => {
          order.push("close");
          state.current = null;
        },
        onplaylistadd: onAdd,
      },
    };

    // Svelte 5 のリアクティブな props ゲッターを模したオブジェクトを渡す
    finishPlaylistAdd(42, {
      get onclose() {
        return state.current?.onclose;
      },
      get onplaylistadd() {
        return state.current?.onplaylistadd;
      },
    });

    expect(state.current).toBeNull();
    expect(onAdd).toHaveBeenCalledTimes(1);
    expect(onAdd).toHaveBeenCalledWith(42);
    expect(order).toEqual(["close", "add:42"]);
  });
});

describe("VC-05 contextMenuMaxHeight の最大高さ算出", () => {
  test("下端折り返し後・上端付近・極小ビューポートで下端余白を確保する", () => {
    const vw = 1280;
    const vh = 800;

    // 通常の折り返し位置でも下端余白が CTX_MENU_MARGIN 以上残る
    const bottomPos = contextMenuPos(100, vh - 5, vw, vh);
    const bottomMax = contextMenuMaxHeight(bottomPos.y, vh);
    expect(bottomMax).toBe(CTX_MENU_HEIGHT);
    expect(bottomPos.y + bottomMax).toBeLessThanOrEqual(vh - CTX_MENU_MARGIN);

    // 上端付近で項目が多い場合は画面下端までに収める
    expect(contextMenuMaxHeight(50, vh)).toBe(vh - 50 - CTX_MENU_MARGIN);

    // 極小ビューポートでも負にならない
    const tinyPos = contextMenuPos(300, 300, 200, 200);
    expect(tinyPos.y).toBe(0);
    expect(contextMenuMaxHeight(tinyPos.y, 200)).toBe(200 - CTX_MENU_MARGIN);
    expect(contextMenuMaxHeight(10, 4)).toBe(0);
  });
});
