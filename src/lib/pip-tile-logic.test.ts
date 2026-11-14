// PT-01: 整列先モニターの解決（window.screen 指定・未設定/未接続はプライマリ・全欠落は列挙先頭）
// PT-02: 格子の分割（台数→行列数）とマス目 geometry の生成（座標・隙間なく敷き詰める）
import { describe, expect, it } from "vitest";
import {
  pipTileGeometry,
  pipTileGrid,
  pipTileMonitor,
} from "./pip-tile-logic";
import type { MonitorInfo } from "./monitors";

function mon(
  partial: Partial<MonitorInfo> & { name: string | null },
): MonitorInfo {
  return {
    index: 0,
    x: 0,
    y: 0,
    width: 1920,
    height: 1080,
    primary: false,
    ...partial,
  };
}

describe("PT-01 pipTileMonitor の整列先解決", () => {
  const primary = mon({ name: "\\\\.\\DISPLAY1", primary: true });
  const secondary = mon({ name: "\\\\.\\DISPLAY2", index: 1, x: 1920 });

  it("window.screen の保存名に一致するモニターを選ぶ", () => {
    expect(pipTileMonitor([primary, secondary], "\\\\.\\DISPLAY2")).toBe(
      secondary,
    );
  });

  it("未設定（空・null）はプライマリへ退く", () => {
    expect(pipTileMonitor([primary, secondary], "")).toBe(primary);
    expect(pipTileMonitor([primary, secondary], null)).toBe(primary);
  });

  it("未接続の保存名はプライマリへ退く", () => {
    expect(pipTileMonitor([primary, secondary], "\\\\.\\DISPLAY9")).toBe(
      primary,
    );
  });

  it("プライマリが無ければ列挙先頭、モニター 0 台は null", () => {
    const a = mon({ name: "A" });
    const b = mon({ name: "B", index: 1 });
    expect(pipTileMonitor([a, b], null)).toBe(a);
    expect(pipTileMonitor([], null)).toBeNull();
  });
});

describe("PT-02 格子分割と geometry 生成", () => {
  it("台数から列 x 行を決める", () => {
    expect(pipTileGrid(1)).toEqual({ cols: 1, rows: 1 });
    expect(pipTileGrid(2)).toEqual({ cols: 2, rows: 1 });
    expect(pipTileGrid(3)).toEqual({ cols: 2, rows: 2 });
    expect(pipTileGrid(4)).toEqual({ cols: 2, rows: 2 });
    expect(pipTileGrid(6)).toEqual({ cols: 3, rows: 2 });
    expect(pipTileGrid(9)).toEqual({ cols: 3, rows: 3 });
    expect(pipTileGrid(10)).toEqual({ cols: 4, rows: 3 });
  });

  it("マス目 geometry は対象ディスプレイ左上原点の WxH+x+y", () => {
    // 1920x1080 に 4 台 → 2x2 の各 960x540
    const { cols, rows } = pipTileGrid(4);
    expect(pipTileGeometry(0, cols, rows, 1920, 1080)).toBe("960x540+0+0");
    expect(pipTileGeometry(1, cols, rows, 1920, 1080)).toBe("960x540+960+0");
    expect(pipTileGeometry(2, cols, rows, 1920, 1080)).toBe("960x540+0+540");
    expect(pipTileGeometry(3, cols, rows, 1920, 1080)).toBe(
      "960x540+960+540",
    );
    // 1 台ならディスプレイ全面
    expect(pipTileGeometry(0, 1, 1, 1920, 1080)).toBe("1920x1080+0+0");
  });

  it("割り切れない寸法でも隙間・重なりなく敷き詰める", () => {
    // 1366x768 に 3 台 → 2x2（右列が余り 1px を拾う形ではなく均等割り）
    const { cols, rows } = pipTileGrid(3);
    const cells = [0, 1, 2].map((i) =>
      pipTileGeometry(i, cols, rows, 1366, 768),
    );
    expect(cells).toEqual([
      "683x384+0+0",
      "683x384+683+0",
      "683x384+0+384",
    ]);
  });
});
