// 表示設定（ui.theme / ui.density / ui.language）の適用層
// （FR-76・FR-77・FR-81、仕様決定 DA・DB・DF）。
// 保存値を `<html>` の data-theme / data-density / lang 属性へ載せ、
// 実際の配色・寸法は src/app.css の CSS 変数が、UI 言語の本文は
// src/lib/i18n.ts のリソースが決める（setLocale で現行ロケールへ反映し、
// t() が $state 経由で画面を即時に再評価する）。
// system 指定時は OS の prefers-color-scheme の変化にも追従する。

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { setLocale } from "./i18n";
import {
  resolveTheme,
  uiDensity,
  uiLanguage,
  uiTheme,
  type UiDensity,
  type UiLanguage,
  type UiTheme,
} from "./ui-logic";

let theme: UiTheme = "dark";
let density: UiDensity = "comfortable";
let language: UiLanguage = "ja";
let media: MediaQueryList | null = null;

/// 現在の設定値を `<html>` の data 属性・lang 属性へ反映する
function applyAttrs(): void {
  const root = document.documentElement;
  root.dataset.theme = resolveTheme(theme, media?.matches ?? false);
  root.dataset.density = density;
  root.lang = language;
}

/// data 属性・ロケールの反映を開始する。`settings://changed` を先に購読してから
/// settings_get で初期値を読み、OS の配色変更も監視する
/// （届いたイベントの値を正とする。+page.svelte と同じ型）。
/// 返した関数は購読解除用（$effect のクリーンアップ）。
/// 本体窓とチャットポップアップ窓のどちらでもレイアウトから呼ぶ
export function initUiPrefs(): () => void {
  media = window.matchMedia("(prefers-color-scheme: light)");
  media.addEventListener("change", applyAttrs);
  // 初期読み取りと設定変更の競合を避けるため、コミット済みの値を持つ
  // settings://changed を先に購読する（届いた以降はイベント側を正とする。
  // キーごとに追跡し、イベント未受信のキーは初期読み取りの値を使う）
  const seen = new Set<string>();
  const unlistenPromise = listen<{ key: string; value: string }>(
    "settings://changed",
    (ev) => {
      const { key, value } = ev.payload;
      if (key === "ui.theme") {
        seen.add(key);
        theme = uiTheme(value);
        applyAttrs();
      } else if (key === "ui.density") {
        seen.add(key);
        density = uiDensity(value);
        applyAttrs();
      } else if (key === "ui.language") {
        seen.add(key);
        language = uiLanguage(value);
        setLocale(language);
        applyAttrs();
      }
    },
  );
  void (async () => {
    try {
      const [themeRaw, densityRaw, languageRaw] = await Promise.all([
        invoke<string | null>("settings_get", { key: "ui.theme" }),
        invoke<string | null>("settings_get", { key: "ui.density" }),
        invoke<string | null>("settings_get", { key: "ui.language" }),
      ]);
      // 購読中に届いたイベントの値を正とし、初期読み取りの遅延解決で戻さない
      if (!seen.has("ui.theme")) theme = uiTheme(themeRaw);
      if (!seen.has("ui.density")) density = uiDensity(densityRaw);
      if (!seen.has("ui.language")) {
        language = uiLanguage(languageRaw);
        setLocale(language);
      }
      applyAttrs();
    } catch {
      // 読み取り失敗は既定（dark / comfortable / ja）のままにする
    }
  })();
  return () => {
    media?.removeEventListener("change", applyAttrs);
    void unlistenPromise.then((u) => u()).catch(() => {});
  };
}
