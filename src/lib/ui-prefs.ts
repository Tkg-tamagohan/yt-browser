// 表示設定（ui.theme / ui.density / ui.language）および
// 再生開始時の画面遷移設定（player.navigate_on_play）の適用層
// （FR-76・FR-77・FR-81・FR-92、仕様決定 DA・DB・DF・DU）。
// 保存値を `<html>` の data-theme / data-density / lang 属性へ載せ、
// 実際の配色・寸法は src/app.css の CSS 変数が、UI 言語の本文は
// src/lib/i18n.ts のリソースが決める（setLocale で現行ロケールへ反映し、
// t() が $state 経由で画面を即時に再評価する）。
// system 指定時は OS の prefers-color-scheme の変化にも追従する。

import { goto } from "$app/navigation";
import { page } from "$app/state";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { setLocale } from "./i18n";
import {
  playerNavigateOnPlay,
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
let navigateOnPlayPref = true;
let media: MediaQueryList | null = null;

/// `player.navigate_on_play`（FR-92、仕様決定 DU）に従い、
/// 有効時かつ現在パスが `/` 以外のときのみ再生画面へ遷移する。
export function navigateOnPlay(): void {
  if (!navigateOnPlayPref) return;
  if (page.url.pathname !== "/") {
    void goto("/");
  }
}

/// 現在の設定値を `<html>` の data 属性・lang 属性へ反映する
function applyAttrs(): void {
  const root = document.documentElement;
  root.dataset.theme = resolveTheme(theme, media?.matches ?? false);
  root.dataset.density = density;
  root.lang = language;
}

async function fetchUiPrefs(): Promise<
  [string | null, string | null, string | null, string | null]
> {
  return Promise.all([
    invoke<string | null>("settings_get", { key: "ui.theme" }),
    invoke<string | null>("settings_get", { key: "ui.density" }),
    invoke<string | null>("settings_get", { key: "ui.language" }),
    invoke<string | null>("settings_get", {
      key: "player.navigate_on_play",
    }),
  ]);
}

/// DB から表示設定と再生時遷移設定を再読込して即時反映する
/// （データインポート直後など `settings://changed` が出ない変更後の同期に使う）
export async function refreshUiPrefs(): Promise<void> {
  if (typeof window !== "undefined" && media === null) {
    media = window.matchMedia("(prefers-color-scheme: light)");
  }
  try {
    const [themeRaw, densityRaw, languageRaw, navOnPlayRaw] =
      await fetchUiPrefs();
    theme = uiTheme(themeRaw);
    density = uiDensity(densityRaw);
    language = uiLanguage(languageRaw);
    setLocale(language);
    navigateOnPlayPref = playerNavigateOnPlay(navOnPlayRaw);
    applyAttrs();
  } catch {
    // 読み取り失敗時は現在値を維持する
  }
}

/// data 属性・ロケールの反映を開始する。`settings://changed` を先に購読してから
/// settings_get で初期値を読み、OS の配色変更も監視する
/// （届いたイベントの値を正とする。+page.svelte と同じ型）。
/// 返した関数は購読解除用（$effect のクリーンアップ）。
/// 本体窓とチャットポップアップ窓のどちらでもレイアウトから呼ぶ
export function initUiPrefs(): () => void {
  let disposed = false;
  media = window.matchMedia("(prefers-color-scheme: light)");
  media.addEventListener("change", applyAttrs);
  // 初期読み取りと設定変更の競合を避けるため、コミット済みの値を持つ
  // settings://changed を先に購読する（届いた以降はイベント側を正とする。
  // キーごとに追跡し、イベント未受信のキーは初期読み取りの値を使う）
  const seen = new Set<string>();
  const unlistenPromise = listen<{ key: string; value: string }>(
    "settings://changed",
    (ev) => {
      if (disposed) return;
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
      } else if (key === "player.navigate_on_play") {
        seen.add(key);
        navigateOnPlayPref = playerNavigateOnPlay(value);
      }
    },
  );
  void (async () => {
    try {
      const [themeRaw, densityRaw, languageRaw, navOnPlayRaw] =
        await fetchUiPrefs();
      if (disposed) return;
      // 購読中に届いたイベントの値を正とし、初期読み取りの遅延解決で戻さない
      if (!seen.has("ui.theme")) theme = uiTheme(themeRaw);
      if (!seen.has("ui.density")) density = uiDensity(densityRaw);
      if (!seen.has("ui.language")) {
        language = uiLanguage(languageRaw);
        setLocale(language);
      }
      if (!seen.has("player.navigate_on_play")) {
        navigateOnPlayPref = playerNavigateOnPlay(navOnPlayRaw);
      }
      applyAttrs();
    } catch {
      // 読み取り失敗は既定（dark / comfortable / ja）のままにする
    }
  })();
  return () => {
    disposed = true;
    media?.removeEventListener("change", applyAttrs);
    void unlistenPromise.then((u) => u()).catch(() => {});
  };
}
