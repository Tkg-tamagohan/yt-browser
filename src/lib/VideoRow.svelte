<script lang="ts">
  // 動画行の共有骨格（監査「動画行スケルトンの共有化」）。
  // フィード・検索・ライブラリ（履歴/お気に入り/プレイリスト項目）・関連動画の
  // 6 行型を 1 コンポーネントに集約する。
  //
  // 統一方針（計画書 video-row.md の採択オプション）:
  //   VR-1a: タイトルは全行で再生を呼ぶ button（onplay を渡すとボタン化）。
  //   VR-2a: actionsPlacement で below（メタ内下段）/ side（行右端・縦中央）の
  //         2 系統を維持。
  //   VR-3a: 幅は thumbSize の 3 系統（96/120/160px）を維持し、16:9 cover・
  //         角丸・下地色は統一。サムネイル欠落時は mqdefault で補完し、
  //         loading="lazy" を全行に付ける。
  //   VR-4a: 罫線は border-bottom #2d2f33 に統一。関連パネルは dense variant
  //         （小フォント・狭余白・罫線なし）。
  import type { Snippet } from "svelte";

  type Props = {
    /// 動画 ID。サムネイル欠落時の mqdefault 補完に使う。
    videoId: string;
    /// 行タイトル（onplay 指定時は再生ボタンとして描画）。
    title: string;
    /// サムネイル URL。null/省略時は i.ytimg.com/vi/<id>/mqdefault.jpg で補完。
    thumbnailUrl?: string | null;
    /// サムネイル幅: sm=96px, md=120px, lg=160px。
    thumbSize?: "sm" | "md" | "lg";
    /// 既読などの減光表示（フィードの既読行）。
    dimmed?: boolean;
    /// 関連パネルの小さめ表示（小フォント・狭余白・罫線なし）。
    dense?: boolean;
    /// アクション群の位置: below=メタ内下段 / side=行右端（縦中央揃え）。
    actionsPlacement?: "below" | "side";
    /// 指定時タイトルを再生ボタンとして描画する。
    onplay?: () => void;
    /// 行頭の追加要素（プレイリストの順位番号など）。
    leading?: Snippet;
    /// サブ行（チャンネル名や日時など）。中身は各ページの現行マークアップを受ける。
    sub: Snippet;
    /// アクション群。再生・ブロック・VideoActions など各ページの現行ボタンを受ける。
    actions: Snippet;
  };
  let {
    videoId,
    title,
    thumbnailUrl = null,
    thumbSize = "md",
    dimmed = false,
    dense = false,
    actionsPlacement = "below",
    onplay,
    leading,
    sub,
    actions,
  }: Props = $props();

  let thumbSrc = $derived(
    thumbnailUrl ?? `https://i.ytimg.com/vi/${videoId}/mqdefault.jpg`,
  );
</script>

<li
  class="vr"
  class:side={actionsPlacement === "side"}
  class:dense
  class:dimmed
>
  {@render leading?.()}
  <img
    class="thumb {thumbSize}"
    src={thumbSrc}
    alt=""
    loading="lazy"
  />
  <div class="meta">
    {#if onplay}
      <button class="title" onclick={onplay}>{title}</button>
    {:else}
      <div class="title">{title}</div>
    {/if}
    <div class="sub">{@render sub()}</div>
    {#if actionsPlacement === "below"}
      <div class="actions">{@render actions()}</div>
    {/if}
  </div>
  {#if actionsPlacement === "side"}
    {@render actions()}
  {/if}
</li>

<style>
  .vr {
    display: flex;
    gap: 12px;
    align-items: flex-start;
    padding: 10px 0;
    border-bottom: 1px solid #2d2f33;
  }

  .vr.side {
    align-items: center;
  }

  .vr.dimmed {
    opacity: 0.55;
  }

  .vr.dense {
    gap: 10px;
    padding: 6px 0;
    border-bottom: none;
  }

  .thumb {
    aspect-ratio: 16 / 9;
    object-fit: cover;
    border-radius: 6px;
    background: #26282c;
    flex-shrink: 0;
  }

  .thumb.sm {
    width: 96px;
  }

  .thumb.md {
    width: 120px;
  }

  .thumb.lg {
    width: 160px;
  }

  .meta {
    flex: 1;
    min-width: 0;
  }

  .title {
    display: block;
    font-weight: 600;
    overflow-wrap: anywhere;
  }

  button.title {
    width: 100%;
    padding: 0;
    border: none;
    background: none;
    color: #e8e8e8;
    font-family: inherit;
    font-size: inherit;
    text-align: left;
    cursor: pointer;
  }

  button.title:hover {
    text-decoration: underline;
  }

  .dense .title {
    font-size: 0.9rem;
    font-weight: 400;
  }

  .sub {
    color: #9aa0a6;
    font-size: 0.85rem;
    margin: 4px 0 8px;
  }

  .side .sub {
    margin: 0;
  }

  .dense .sub {
    font-size: 0.8rem;
    margin: 2px 0 6px;
  }

  .actions {
    display: flex;
    gap: 10px;
    align-items: center;
    flex-wrap: wrap;
  }

  .dense .actions {
    gap: 8px;
    font-size: 0.85rem;
  }
</style>
