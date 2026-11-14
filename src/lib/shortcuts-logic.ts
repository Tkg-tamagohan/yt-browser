// アプリ側キーボードショートカット（FR-79、仕様決定 DD）の純粋ロジック。
// 既定キーマップ・有効化設定の解釈・対象ガード・選択行の遷移をここに集約し、
// DOM/Tauri への配線は shortcuts.svelte.ts が受け持つ。
//
// 暫定解釈（仕様決定 DD は「既定キーマップと有効化設定を持つ」とだけ定め、
// キー割り当て・既定値・対象操作の内訳は未定のため）:
//   - 一覧操作: j/↓ で次行、k/↑ で前行、Enter で再生、a でキューに追加
//   - キュー操作: q でキューパネル開閉、上の a（選択行のキュー追加）
//   - 有効化設定 ui.shortcuts の既定は off
//   - キューパネル内の項目編集（並べ替え・削除など）は対象外

/// ショートカットが引き起こす操作の識別子。
/// 名前は DOM やページ実装から独立させ、配線側で行の実ボタンへ写像する
export type ShortcutAction =
  | "listNext"
  | "listPrev"
  | "listPlay"
  | "listQueueAdd"
  | "queueToggle";

/// 既定キーマップ（暫定解釈。上のコメント参照）。
/// キーは KeyboardEvent.key と比較する。1 文字キーは大小を区別し、
/// Shift 押下で別文字になる打鍵は対象外（修飾なしで解決される）
export const DEFAULT_KEYMAP: Readonly<Record<string, ShortcutAction>> = {
  j: "listNext",
  ArrowDown: "listNext",
  k: "listPrev",
  ArrowUp: "listPrev",
  Enter: "listPlay",
  a: "listQueueAdd",
  q: "queueToggle",
};

/// 有効化設定 `ui.shortcuts` の既定値（暫定解釈: 無効寄りの安全側）。
/// "on" 系の文字列だけを true とみなす。値の読み方は feed-settings.ts
/// のオプトイン形式と同じに揃える
export function shortcutsEnabled(raw: string | null | undefined): boolean {
  if (raw === null || raw === undefined) return false;
  const v = raw.trim().toLowerCase();
  return v === "on" || v === "true" || v === "1" || v === "yes";
}

/// キーイベントを操作へ解決する。Ctrl/Alt/Meta を伴う打鍵は
/// ブラウザ・OS 側のショートカットと衝突するため対象外とし、
/// キーマップに無いキーは null を返す
export function resolveShortcut(e: {
  key: string;
  ctrlKey?: boolean;
  altKey?: boolean;
  metaKey?: boolean;
}): ShortcutAction | null {
  if (e.ctrlKey || e.altKey || e.metaKey) return null;
  return DEFAULT_KEYMAP[e.key] ?? null;
}

/// イベント発生源が文字入力要素か。input/textarea/select または
/// contenteditable 内の要素にフォーカスがある間はショートカットを
/// 効かせない（タイプ中の j/k/Enter を一覧操作に奪わないため）
export function isEditableTarget(target: unknown): boolean {
  const tag = targetTag(target);
  if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") return true;
  if (target === null || typeof target !== "object") return false;
  const el = target as {
    isContentEditable?: unknown;
    closest?: unknown;
  };
  if (el.isContentEditable === true) return true;
  if (typeof el.closest === "function") {
    const host = (el.closest as (s: string) => unknown)("[contenteditable]");
    if (host !== null && host !== undefined) return true;
  }
  return false;
}

/// Enter がボタン・リンク等の既定起動を持つ要素上で押されたか。
/// その場合は要素本来の起動（click）を優先し、ショートカット側の
/// 「選択行を再生」は発火させない
export function isActivateTarget(target: unknown): boolean {
  const tag = targetTag(target);
  return tag === "BUTTON" || tag === "A" || tag === "SUMMARY";
}

/// 対象要素のタグ名を大文字で返す（DOM でない値には "" を返す）
function targetTag(target: unknown): string {
  if (target === null || typeof target !== "object") return "";
  const tag = (target as { tagName?: unknown }).tagName;
  return typeof tag === "string" ? tag.toUpperCase() : "";
}

/// 一覧選択の移動先インデックス。
/// len <= 0 なら常に -1（選択なし）。未選択（cur < 0）からは
/// 下へ先頭・上へ末尾へ入り、以後は 0..len-1 にクランプする
/// （端で折り返さない）
export function moveIndex(cur: number, delta: number, len: number): number {
  if (len <= 0) return -1;
  if (cur < 0) return delta >= 0 ? 0 : len - 1;
  return Math.min(len - 1, Math.max(0, cur + delta));
}
