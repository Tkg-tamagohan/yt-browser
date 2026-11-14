// 設定画面（src/routes/settings/+page.svelte）の保存検証・競合検出ヘルパー。
// チェックボックスの on/off、任意入力＋範囲チェック、文字列設定の保存中に
// ユーザーが画面を再操作した場合の未保存検出を共通化する。

export type SetterFn = (key: string, value: string) => Promise<void>;

export type RangeResult =
  | { ok: true; value: string }
  | { ok: false; raw: string };

export type WheelDeltaResult =
  | { skip: true }
  | { skip: false; ok: true; value: string }
  | { skip: false; ok: false; raw: string };

/// 空欄は既定値リセット（""）として受理し、非空は有限数値 [min, max] を検証する
export function validateOptionalRange(
  raw: string,
  min: number,
  max: number,
): RangeResult {
  const trimmed = raw.trim();
  if (trimmed === "") {
    return { ok: true, value: "" };
  }
  const n = Number(trimmed);
  if (Number.isFinite(n) && n >= min && n <= max) {
    return { ok: true, value: trimmed };
  }
  return { ok: false, raw: trimmed };
}

/// feed.auto_read_days の入力検証（未定義または 0 以上の整数を受理）
export function validateAutoReadDays(ad: number | undefined): RangeResult {
  if (ad === undefined || (Number.isInteger(ad) && ad >= 0)) {
    return { ok: true, value: String(ad ?? 0) };
  }
  return { ok: false, raw: String(ad) };
}

/// wheel.volume_delta の入力検証（未定義は保存スキップ、有限数値かつ ±100 以内）
export function validateWheelDelta(
  delta: number | undefined,
): WheelDeltaResult {
  if (delta === undefined) {
    return { skip: true };
  }
  if (!Number.isFinite(delta) || Math.abs(delta) > 100) {
    return { skip: false, ok: false, raw: String(delta) };
  }
  return { skip: false, ok: true, value: String(delta) };
}

/// 真偽値設定を on/off で保存し、保存待ち中の画面変更を検知する
export async function saveToggleSetting(
  setSetting: SetterFn,
  key: string,
  snapshot: boolean,
  getCurrent: () => boolean,
  onUnsaved: () => void,
): Promise<boolean> {
  await setSetting(key, snapshot ? "on" : "off");
  if (getCurrent() !== snapshot) {
    onUnsaved();
    return false;
  }
  return true;
}

/// 文字列設定を保存し、保存待ち中の画面変更を検知する
export async function saveStringSetting(
  setSetting: SetterFn,
  key: string,
  snapshot: string,
  getCurrent: () => string,
  onUnsaved: () => void,
): Promise<boolean> {
  await setSetting(key, snapshot);
  if (getCurrent() !== snapshot) {
    onUnsaved();
    return false;
  }
  return true;
}

/// 任意入力・範囲付き設定を検証して保存し、不正時や待機中の画面変更を通知する
export async function saveOptionalRangeSetting(
  setSetting: SetterFn,
  key: string,
  snapshotRaw: string,
  getCurrentRaw: () => string,
  min: number,
  max: number,
  onFailed: (raw: string) => void,
  onUnsaved: () => void,
): Promise<boolean> {
  const snapshot = snapshotRaw.trim();
  const res = validateOptionalRange(snapshot, min, max);
  let ok = true;
  if (res.ok) {
    await setSetting(key, res.value);
  } else {
    ok = false;
    onFailed(res.raw);
  }
  if (getCurrentRaw().trim() !== snapshot) {
    ok = false;
    onUnsaved();
  }
  return ok;
}

export type BatchEntry = {
  key: string;
  snapshot: string;
  writeValue?: string;
  getCurrent: () => string;
};

/// 先に全スナップショットを確定した複数キーを順次保存し、まとめて競合を検知する
export async function saveSettingBatch(
  setSetting: SetterFn,
  entries: BatchEntry[],
  onUnsaved: () => void,
): Promise<boolean> {
  for (const e of entries) {
    await setSetting(e.key, e.writeValue ?? e.snapshot);
  }
  if (entries.some((e) => e.getCurrent() !== e.snapshot)) {
    onUnsaved();
    return false;
  }
  return true;
}
