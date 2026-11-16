// 拡張側の URL・位置ロジックの回帰テスト（FR-17・FR-51、仕様決定 AC・CB・DN）
// CB-01: withTimeParam — t= の付与・既存 t= の上書き・他クエリの温存
// CB-02: hasUsablePosition — t= を付ける意味がある再生位置の判定
// CB-03: classifyKind / classify / schemePrefix — URL 種別判定と末尾境界
// CB-04: manifest.json・popup.html/js・shared.js 間の構文・DOM ID・export 整合
import { existsSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { afterEach, describe, expect, test, vi } from "vitest";
import * as sharedExports from "./shared.js";
import {
  classify,
  classifyKind,
  hasUsablePosition,
  runWithTimeout,
  schemePrefix,
  withTimeParam,
} from "./shared.js";

const WATCH = "https://www.youtube.com/watch?v=dQw4w9WgXcQ";

describe("CB-01 withTimeParam", () => {
  test("watch URL に t=<秒> を付ける", () => {
    expect(withTimeParam(WATCH, 2630)).toBe(`${WATCH}&t=2630`);
  });

  test("小数秒は整数へ切り捨てる", () => {
    expect(withTimeParam(WATCH, 12.9)).toBe(`${WATCH}&t=12`);
  });

  test("既存の t= は上書きし、list= など他のクエリは温存する", () => {
    const withList = `${WATCH}&list=PLtest12345&t=10`;
    expect(withTimeParam(withList, 42)).toBe(`${WATCH}&list=PLtest12345&t=42`);
  });

  test("youtu.be の t= も書き換える", () => {
    expect(withTimeParam("https://youtu.be/dQw4w9WgXcQ?t=1h2m3s", 90)).toBe(
      "https://youtu.be/dQw4w9WgXcQ?t=90",
    );
  });
});

describe("CB-02 hasUsablePosition", () => {
  test("1 秒以上の有限数だけを受理する", () => {
    expect(hasUsablePosition(90)).toBe(true);
    expect(hasUsablePosition(1)).toBe(true);
    expect(hasUsablePosition(0.5)).toBe(false);
    expect(hasUsablePosition(0)).toBe(false);
    expect(hasUsablePosition(-3)).toBe(false);
  });

  test("非数・非数値は受理しない", () => {
    expect(hasUsablePosition(Number.NaN)).toBe(false);
    expect(hasUsablePosition(Number.POSITIVE_INFINITY)).toBe(false);
    expect(hasUsablePosition(null)).toBe(false);
    expect(hasUsablePosition(undefined)).toBe(false);
    expect(hasUsablePosition("90")).toBe(false);
  });
});

describe("CB-03 classifyKind / classify / schemePrefix", () => {
  test("動画 URL を video と判定する（watch / youtu.be / shorts / live / music / m / 複合）", () => {
    for (const url of [
      WATCH,
      `${WATCH}&list=PLtest12345`,
      "https://youtu.be/dQw4w9WgXcQ",
      "https://www.youtube.com/shorts/dQw4w9WgXcQ",
      "https://www.youtube.com/shorts/dQw4w9WgXcQ/",
      "https://www.youtube.com/live/dQw4w9WgXcQ",
      "https://www.youtube.com/live/dQw4w9WgXcQ/",
      "https://music.youtube.com/watch?v=dQw4w9WgXcQ",
      "https://m.youtube.com/watch?v=dQw4w9WgXcQ",
    ]) {
      expect(classifyKind(url), url).toBe("video");
      expect(classify(url), url).toBe(true);
    }
  });

  test("プレイリスト URL を playlist と判定する", () => {
    for (const url of [
      "https://www.youtube.com/playlist?list=PLtest12345",
      "https://music.youtube.com/playlist?list=PLtest12345",
    ]) {
      expect(classifyKind(url), url).toBe("playlist");
      expect(classify(url), url).toBe(true);
    }
  });

  test("対象外・不正 ID・12 文字以上の shorts/live を null とする", () => {
    for (const url of [
      "https://www.youtube.com/",
      "https://example.com/watch?v=dQw4w9WgXcQ",
      "https://www.youtube.com/watch?v=short",
      "https://www.youtube.com/shorts/dQw4w9WgXcQextra",
      "https://www.youtube.com/live/dQw4w9WgXcQextra",
      "https://youtu.be/dQw4w9WgXcQextra",
      "https://www.youtube.com/playlist",
      "ftp://www.youtube.com/watch?v=dQw4w9WgXcQ",
      "not a url",
    ]) {
      expect(classifyKind(url), url).toBeNull();
      expect(classify(url), url).toBe(false);
    }
  });

  test("schemePrefix が action ごとのスキームプレフィックスを返す", () => {
    expect(schemePrefix("open")).toBe("yt-browser://open?url=");
    expect(schemePrefix("queue")).toBe("yt-browser://queue?url=");
    expect(schemePrefix("playlist_add")).toBe("yt-browser://playlist_add?url=");
  });
});

describe("runWithTimeout", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  test("期限前に完了した場合は応答を 1 回返し、その後のタイムアウトを発火させない", () => {
    vi.useFakeTimers();
    const calls = [];
    runWithTimeout(
      (done) => {
        setTimeout(() => done({ ok: true }, null), 100);
      },
      5000,
      (err, res) => calls.push({ err, res }),
    );
    vi.advanceTimersByTime(100);
    expect(calls).toEqual([{ err: null, res: { ok: true } }]);
    vi.advanceTimersByTime(5000);
    expect(calls).toHaveLength(1);
  });

  test("期限内に応答しない場合はタイムアウトで確定し、遅延到着した応答を無視する", () => {
    vi.useFakeTimers();
    const calls = [];
    let lateDone = null;
    runWithTimeout(
      (done) => {
        lateDone = done;
      },
      5000,
      (err, res) => calls.push({ err, res }),
    );
    vi.advanceTimersByTime(5000);
    expect(calls).toHaveLength(1);
    expect(calls[0].err).toBeInstanceOf(Error);
    expect(calls[0].res).toBeNull();

    lateDone?.({ ok: true }, null);
    expect(calls).toHaveLength(1);
  });
});

describe("CB-04 拡張マニフェスト・DOM ID・共有モジュールの整合", () => {
  const extDir = dirname(fileURLToPath(import.meta.url));

  test("manifest.json が参照するファイルがすべて存在する", () => {
    const manifest = JSON.parse(
      readFileSync(join(extDir, "manifest.json"), "utf-8"),
    );
    expect(manifest.manifest_version).toBe(3);

    const refs = [];
    if (manifest.background?.service_worker) {
      refs.push(manifest.background.service_worker);
    }
    if (manifest.action?.default_popup) {
      refs.push(manifest.action.default_popup);
    }
    for (const cs of manifest.content_scripts ?? []) {
      for (const jsFile of cs.js ?? []) {
        refs.push(jsFile);
      }
      for (const cssFile of cs.css ?? []) {
        refs.push(cssFile);
      }
    }
    expect(refs.length).toBeGreaterThan(0);
    for (const rel of refs) {
      expect(existsSync(join(extDir, rel)), `${rel} が存在しない`).toBe(true);
    }
  });

  test("popup.js が参照する DOM ID と popup.html の script 参照が一致する", () => {
    const popupHtml = readFileSync(join(extDir, "popup.html"), "utf-8");
    const popupJs = readFileSync(join(extDir, "popup.js"), "utf-8");

    const htmlIds = new Set(
      [...popupHtml.matchAll(/\bid\s*=\s*["']([^"']+)["']/g)].map((m) => m[1]),
    );
    const jsIds = [
      ...popupJs.matchAll(/document\.getElementById\(\s*["']([^"']+)["']\s*\)/g),
    ].map((m) => m[1]);
    expect(jsIds.length).toBeGreaterThan(0);
    for (const id of jsIds) {
      expect(htmlIds.has(id), `popup.html に #${id} が無い`).toBe(true);
    }

    const scripts = [
      ...popupHtml.matchAll(/<script[^>]*\bsrc\s*=\s*["']([^"']+)["']/gi),
    ].map((m) => m[1]);
    for (const src of scripts) {
      expect(
        existsSync(join(extDir, src.replace(/^\.\//, ""))),
        `popup.html が参照する ${src} が存在しない`,
      ).toBe(true);
    }
  });

  test("extension JS の ./shared.js import がすべて export されている", () => {
    const importRe = /import\s*\{([^}]+)\}\s*from\s*["']\.\/shared\.js["']/g;
    let totalChecked = 0;
    for (const file of ["background.js", "popup.js", "content.js"]) {
      const text = readFileSync(join(extDir, file), "utf-8");
      for (const m of text.matchAll(importRe)) {
        const specs = m[1]
          .split(",")
          .map((s) => s.trim())
          .filter(Boolean);
        for (const spec of specs) {
          const exportedName = spec.split(/\s+as\s+/)[0].trim();
          expect(
            Object.hasOwn(sharedExports, exportedName),
            `${file} が import する shared.js の ${exportedName} が存在しない`,
          ).toBe(true);
          totalChecked += 1;
        }
      }
    }
    expect(totalChecked).toBeGreaterThan(0);
  });
});
