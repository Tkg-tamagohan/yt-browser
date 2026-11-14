// 表示設定（ui.theme / ui.density）の適用層（FR-76・FR-77、仕様決定 DA・DB）。
// 保存値を `<html>` の data-theme / data-density 属性へ載せ、実際の配色・
// 寸法は src/app.css の CSS 変数が決める。
// system 指定時は OS の prefers-color-scheme の変化にも追従する。

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  resolveTheme,
  uiDensity,
  uiTheme,
  type UiDensity,
  type UiTheme,
} from "./ui-logic";

let theme: UiTheme = "dark";
let density: UiDensity = "comfortable";
let media: MediaQueryList | null = null;

/// 現在の設定値を `<html>` の data 属性へ反映する
function applyAttrs(): void {
  const root = document.documentElement;
  root.dataset.theme = resolveTheme(theme, media?.matches ?? false);
  root.dataset.density = density;
}

/// data 属性の反映を開始する。`settings://changed` を先に購読してから
/// settings_get で初期値を読み、OS の配色変更も監視する
/// （届いたイベントの値を正とする。+page.svelte と同じ型）。
/// 返した関数は購読解除用（$effect のクリーンアップ）。
/// 本体窓とチャットポップアップ窓のどちらでもレイアウトから呼ぶ
export function initUiPrefs(): () => void {
  media = window.matchMedia("(prefers-color-scheme: light)");
  media.addEventListener("change", applyAttrs);
  // 初期読み取りと設定変更の競合を避けるため、コミット済みの値を持つ
  // settings://changed を先に購読する（届いた以降はイベント側を正とする）
  let changed = false;
  const unlistenPromise = listen<{ key: string; value: string }>(
    "settings://changed",
    (ev) => {
      const { key, value } = ev.payload;
      if (key === "ui.theme") {
        changed = true;
        theme = uiTheme(value);
        applyAttrs();
      } else if (key === "ui.density") {
        changed = true;
        density = uiDensity(value);
        applyAttrs();
      }
    },
  );
  void (async () => {
    try {
      const [themeRaw, densityRaw] = await Promise.all([
        invoke<string | null>("settings_get", { key: "ui.theme" }),
        invoke<string | null>("settings_get", { key: "ui.density" }),
      ]);
      // 購読中に届いたイベントの値を正とし、初期読み取りの遅延解決で戻さない
      if (!changed) {
        theme = uiTheme(themeRaw);
        density = uiDensity(densityRaw);
      }
      applyAttrs();
    } catch {
      // 読み取り失敗は既定（dark / comfortable）のままにする
    }
  })();
  return () => {
    media?.removeEventListener("change", applyAttrs);
    void unlistenPromise.then((u) => u()).catch(() => {});
  };
}
