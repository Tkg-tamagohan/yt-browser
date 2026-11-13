// シークバー（独自スライダー）の位置換算と SponsorBlock 区間帯の純粋ロジック
// （FR-36、BG-8、仕様決定 BI）。
// native の input[type=range] ではドラッグ終点が値丸めでずれる問題があったため、
// ピクセル→割合→秒の変換をここで一元化して境界値を Vitest（SB-NN）で固定する。

/// 0〜1 へクランプする。
export function clampRatio(v: number): number {
  if (!Number.isFinite(v)) return 0;
  return Math.min(1, Math.max(0, v));
}

/// 再生位置からバー上の割合（0〜1）を返す。duration が不明（0 以下）のときは 0。
export function ratioForPosition(position: number, duration: number): number {
  if (duration <= 0) return 0;
  return clampRatio(position / duration);
}

/// ポインタの X 座標からトラック上の割合（0〜1）を返す。
/// clientX がトラック外にはみ出しても端へクランプする（ドラッグ終点ずれ防止）。
export function ratioFromPointerX(
  clientX: number,
  trackLeft: number,
  trackWidth: number,
): number {
  if (trackWidth <= 0) return 0;
  return clampRatio((clientX - trackLeft) / trackWidth);
}

/// 割合から再生位置（秒）を返す。duration が不明のときは 0。
export function positionForRatio(ratio: number, duration: number): number {
  if (duration <= 0) return 0;
  return clampRatio(ratio) * duration;
}

/// キーボード 1 ノッチの移動量（秒）。
export const ARROW_STEP_SEC = 5;

/// キー操作に対応するシーク先（秒）を返す。対応しないキーは null。
/// ArrowLeft/Right は ±5 秒、Home/End は先頭・末尾。
export function seekTargetForKey(
  key: string,
  position: number,
  duration: number,
): number | null {
  if (duration <= 0) return null;
  switch (key) {
    case "ArrowLeft":
      return Math.max(0, position - ARROW_STEP_SEC);
    case "ArrowRight":
      return Math.min(duration, position + ARROW_STEP_SEC);
    case "Home":
      return 0;
    case "End":
      return duration;
    default:
      return null;
  }
}

/// 区間のバー上の帯（左端%と幅%）を返す。
/// 無効区間（end<=start）・範囲外・duration 不明は null。
/// はみ出す区間は 0〜duration へクランプしてから換算する。
export function segmentBand(
  start: number,
  end: number,
  duration: number,
): { leftPct: number; widthPct: number } | null {
  if (duration <= 0 || end <= start) return null;
  const s = Math.min(Math.max(start, 0), duration);
  const e = Math.min(Math.max(end, 0), duration);
  if (e <= s) return null;
  return { leftPct: (s / duration) * 100, widthPct: ((e - s) / duration) * 100 };
}

/// 表示対象の区間だけ残す（動作 off の区間はシークバーへ出さない、仕様決定 BI）。
export function visibleSegments<T extends { action: string }>(
  segments: readonly T[],
): T[] {
  return segments.filter((s) => s.action !== "off");
}
