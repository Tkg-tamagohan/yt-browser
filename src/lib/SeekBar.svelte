<script lang="ts">
  // シークバーの自前スライダー（FR-36、BG-8、仕様決定 BI）。
  // input[type=range] ではドラッグ終点がブラウザの値丸めでずれるため、
  // Pointer Events でピクセル→割合→秒を直接換算する（換算は seek-logic.ts）。
  // SponsorBlock 区間はバー上に色帯で重ね、カテゴリ名をホバーで示す。
  import { t, type MessageKey } from "$lib/i18n";
  import { fmtDuration } from "$lib/format";
  import type { SponsorSegment } from "$lib/players.svelte";
  import {
    positionForRatio,
    ratioForPosition,
    ratioFromPointerX,
    seekTargetForKey,
    segmentBand,
    visibleSegments,
  } from "$lib/seek-logic";

  let {
    position,
    duration,
    disabled = false,
    segments = [],
    showSegments = true,
    onseek,
    onpreview,
  }: {
    /// 現在の再生位置（秒）。
    position: number;
    /// 動画の長さ（秒）。0 以下は未確定として操作を無効化する。
    duration: number;
    disabled?: boolean;
    /// SponsorBlock 区間（このインスタンスの動画に対する取得結果）。
    segments?: SponsorSegment[];
    /// `sponsor.show_segments` の反映値。off なら帯を出さない。
    showSegments?: boolean;
    /// 確定したシーク先（秒）。
    onseek: (seconds: number) => void;
    /// ドラッグ中のプレビュー位置（秒）。終了時は null で通知する。
    onpreview?: (seconds: number | null) => void;
  } = $props();

  let track: HTMLDivElement | undefined = $state();
  // ドラッグ中のプレビュー位置。非ドラッグ中は null で親からの position を使う
  let preview = $state<number | null>(null);

  const shown = $derived(preview ?? position);
  const fillPct = $derived(ratioForPosition(shown, duration) * 100);
  const bands = $derived(
    showSegments
      ? visibleSegments(segments)
          .map((s) => ({ s, band: segmentBand(s.start, s.end, duration) }))
          .filter((x): x is { s: SponsorSegment; band: { leftPct: number; widthPct: number } } => x.band !== null)
      : [],
  );

  function previewAt(clientX: number): number {
    const rect = track?.getBoundingClientRect();
    if (!rect) return 0;
    return positionForRatio(
      ratioFromPointerX(clientX, rect.left, rect.width),
      duration,
    );
  }

  function down(e: PointerEvent): void {
    if (disabled || duration <= 0) return;
    e.preventDefault();
    track?.setPointerCapture(e.pointerId);
    preview = previewAt(e.clientX);
    onpreview?.(preview);
  }

  function move(e: PointerEvent): void {
    if (preview === null) return;
    preview = previewAt(e.clientX);
    onpreview?.(preview);
  }

  function up(): void {
    if (preview === null) return;
    const seconds = preview;
    preview = null;
    onpreview?.(null);
    onseek(seconds);
  }

  function cancel(): void {
    if (preview === null) return;
    preview = null;
    onpreview?.(null);
  }

  function key(e: KeyboardEvent): void {
    if (disabled || duration <= 0) return;
    const target = seekTargetForKey(e.key, shown, duration);
    if (target === null) return;
    e.preventDefault();
    onseek(target);
  }

  function segmentTitle(s: SponsorSegment): string {
    // 設定画面と同じ日本語ラベルに揃える。未定義カテゴリは API の値のまま
    const catKey = `sponsor.cat.${s.category}` as MessageKey;
    const localized = t(catKey);
    const label = localized === catKey ? s.category : localized;
    return `${label} ${fmtDuration(s.start)}–${fmtDuration(s.end)}`;
  }
</script>

<div
  class="seekbar"
  class:disabled
  bind:this={track}
  role="slider"
  tabindex={disabled ? -1 : 0}
  aria-label={t("player.seekbar")}
  aria-valuemin={0}
  aria-valuemax={Math.max(duration, 0)}
  aria-valuenow={shown}
  onpointerdown={down}
  onpointermove={move}
  onpointerup={up}
  onpointercancel={cancel}
  onkeydown={key}
>
  {#each bands as b}
    <div
      class="seg-band"
      style:left="{b.band.leftPct}%"
      style:width="{b.band.widthPct}%"
      title={segmentTitle(b.s)}
    ></div>
  {/each}
  <div class="fill" style:width="{fillPct}%"></div>
  <div class="thumb" style:left="{fillPct}%"></div>
</div>

<style>
  /* 動画下部の帯全体がトラック。ドラッグ中は掴んだ指がバー外に出ても
     pointer capture で追従する */
  .seekbar {
    position: relative;
    width: 100%;
    height: 14px;
    border-radius: 7px;
    background: var(--bg-hover);
    cursor: pointer;
    touch-action: none;
    user-select: none;
    overflow: visible;
  }

  .seekbar.disabled {
    opacity: 0.5;
    cursor: default;
  }

  .seekbar:focus-visible {
    outline: 2px solid var(--accent-strong);
    outline-offset: 2px;
  }

  .fill {
    position: absolute;
    top: 0;
    left: 0;
    height: 100%;
    border-radius: 7px 0 0 7px;
    background: var(--accent-strong);
    pointer-events: none;
  }

  .thumb {
    position: absolute;
    top: 50%;
    width: 14px;
    height: 14px;
    border-radius: 50%;
    background: var(--accent-bright);
    transform: translate(-50%, -50%);
    pointer-events: none;
  }

  /* SponsorBlock 区間帯（FR-36、仕様決定 BI）。動作が off でない全区間を
     単色で重ね、カテゴリ名は帯のホバー title で見せる */
  .seg-band {
    position: absolute;
    top: 0;
    height: 100%;
    background: var(--warn-band);
    pointer-events: auto;
  }
</style>
