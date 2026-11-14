// アプリ側キーボードショートカット（FR-79、仕様決定 DD）の配線。
// レイアウトから initShortcuts() で一度だけ張る。有効化は設定
// `ui.shortcuts`（既定 off、暫定解釈）を settings_get で読み、
// settings://changed で追従する。
// キーマップ・ガード・選択遷移の判断は shortcuts-logic.ts に集約する。

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { queuePanel } from "$lib/queue.svelte";
import { videoContextMenu } from "$lib/video-actions.svelte";
import {
  isActivateTarget,
  isEditableTarget,
  moveIndex,
  resolveShortcut,
  shortcutsEnabled,
} from "$lib/shortcuts-logic";

/// `ui.shortcuts` の現在有効値。設定の読み取りに失敗したときは
/// 既定（off）のまま静かに動く
let enabled = $state(false);

/// 選択カーソルは「画面に出ている動画行（li.vr）の DOM 順の何番目か」
/// を保持する。一覧の差し替えで指す行が変わり得るため位置だけの軽い
/// 状態とし、行集合はキーごとに取り直す（暫定解釈）
let cursor = -1;
let selectedEl: HTMLElement | null = null;

/// 画面の動画行を DOM 順で返す。キューパネルやその他の一覧は
/// li.vr を使わないため、VideoRow を持つ一覧だけが対象になる
function rows(): HTMLElement[] {
  return [
    ...document.querySelectorAll<HTMLElement>("li.vr[data-video-id]"),
  ];
}

function clearSelected(): void {
  selectedEl?.removeAttribute("data-shortcut-selected");
  selectedEl = null;
}

/// 行を選択して見える位置へ寄せる。選択の目印は CSS 側の
/// .vr[data-shortcut-selected] スタイル
function selectAt(i: number, list: HTMLElement[]): void {
  clearSelected();
  const el = list[i];
  if (el === undefined) return;
  el.setAttribute("data-shortcut-selected", "");
  el.scrollIntoView({ block: "nearest" });
  selectedEl = el;
}

/// 選択行の行アクションボタンを押す。対象行内の
/// [data-act="..."]（再生はタイトルボタン、キュー追加は Q＋）を
/// クリックし、各ページの既存処理をそのまま流す
function clickRowAction(i: number, act: string): void {
  const btn = rows()[i]?.querySelector<HTMLElement>(`[data-act="${act}"]`);
  if (btn === null || btn === undefined) return;
  btn.click();
}

function onKeydown(e: KeyboardEvent): void {
  if (!enabled) return;
  // コンテキストメニュー表示中は一覧操作を効かせない
  // （Escape によるクローズは VideoContextMenu 側で処理する）
  if (videoContextMenu.current !== null) return;
  // 入力欄フォーカス中と修飾キー付きは対象外
  if (isEditableTarget(e.target)) return;
  const action = resolveShortcut(e);
  if (action === null) return;
  switch (action) {
    case "listNext":
    case "listPrev": {
      const list = rows();
      cursor = moveIndex(cursor, action === "listNext" ? 1 : -1, list.length);
      if (cursor < 0) return;
      selectAt(cursor, list);
      e.preventDefault();
      return;
    }
    case "listPlay": {
      // ボタン・リンク上の Enter はその要素本来の起動を優先する
      if (isActivateTarget(e.target)) return;
      if (cursor < 0) return;
      clickRowAction(cursor, "play");
      e.preventDefault();
      return;
    }
    case "listQueueAdd": {
      if (cursor < 0) return;
      clickRowAction(cursor, "queue-add");
      e.preventDefault();
      return;
    }
    case "queueToggle": {
      queuePanel.open = !queuePanel.open;
      e.preventDefault();
      return;
    }
  }
}

/// ショートカットの有効化購読とキーハンドラを張る。
/// 戻り値は解除用（アンマウントやチャットポップアップ窓への
/// 切り替えでレイアウト側が呼ぶ）
export function initShortcuts(): () => void {
  let unlisten: UnlistenFn | undefined;
  // 設定画面の保存と初期読み取りが競合しないよう、コミット済みの値を持つ
  // settings://changed を先に購読し、届いた以降はイベント側を正とする
  // （+page.svelte の pip.default と同じ構造）
  let changed = false;
  void listen<{ key: string; value: string }>(
    "settings://changed",
    (ev) => {
      if (ev.payload.key === "ui.shortcuts") {
        changed = true;
        enabled = shortcutsEnabled(ev.payload.value);
      }
    },
  )
    .then((u) => {
      unlisten = u;
    })
    .catch(() => {
      // 変更追従なしでも初期値の読み込みで動く
    });
  void invoke<string | null>("settings_get", { key: "ui.shortcuts" })
    .then((raw) => {
      // 購読中に届いたイベントの値を正とし、遅延解決の読み取りで戻さない
      if (!changed) enabled = shortcutsEnabled(raw);
    })
    .catch(() => {
      // 読み取り失敗時は既定（off）のまま動く
    });
  window.addEventListener("keydown", onKeydown);
  return () => {
    window.removeEventListener("keydown", onKeydown);
    unlisten?.();
    clearSelected();
    cursor = -1;
  };
}
