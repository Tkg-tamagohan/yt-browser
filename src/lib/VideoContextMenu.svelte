<script lang="ts">
  // 動画行のコンテキストメニュー（FR-78、仕様決定 DC）。
  // レイアウトで 1 つだけマウントし、video-actions.svelte.ts の共有状態
  // （対象動画・開いた側の行アクション状態・クリック位置）に従って
  // メニューを出す。メニュー部品は行アクション「＋」と同じ
  // VideoActionsMenu をそのまま使う（メニュー部品の共用）
  import { page } from "$app/state";
  import {
    contextMenuMaxHeight,
    contextMenuPos,
  } from "$lib/context-menu-logic";
  import {
    closeVideoContextMenu,
    videoContextMenu,
  } from "$lib/video-actions.svelte";
  import type { VideoRef } from "$lib/players.svelte";
  import VideoActionsMenu from "$lib/VideoActionsMenu.svelte";

  let menuEl = $state<HTMLElement | null>(null);

  let resizeTick = $state(0);
  $effect(() => {
    if (videoContextMenu.current === null) return;
    const onResize = () => {
      resizeTick += 1;
    };
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  });

  // ビューポートの右端・下端をはみ出す場合は内側へ折り返す
  // （概算寸法で決める。context-menu-logic.ts、VC-NN 系テスト）
  let pos = $derived.by(() => {
    void resizeTick;
    if (videoContextMenu.current === null) return { x: 0, y: 0 };
    return contextMenuPos(
      videoContextMenu.current.x,
      videoContextMenu.current.y,
      window.innerWidth,
      window.innerHeight,
    );
  });

  let maxHeight = $derived.by(() => {
    void resizeTick;
    if (videoContextMenu.current === null) return 0;
    return contextMenuMaxHeight(pos.y, window.innerHeight);
  });

  // メニュー外のポインタ操作（左クリック・右クリックどちらでも）で閉じる。
  // キャプチャ相で拾い、行の右クリックハンドラより先に判定しても
  // メニュー内や開いたトリガーボタンのイベントは閉じ対象にしない
  function onPointerDown(e: PointerEvent): void {
    const target = e.target;
    if (target instanceof Node) {
      if (menuEl !== null && menuEl.contains(target)) {
        return;
      }
      if (
        videoContextMenu.current?.triggerEl != null &&
        videoContextMenu.current.triggerEl.contains(target)
      ) {
        return;
      }
    }
    closeVideoContextMenu();
  }

  function onKeydown(e: KeyboardEvent): void {
    if (e.key === "Escape") closeVideoContextMenu();
  }

  $effect(() => {
    window.addEventListener("pointerdown", onPointerDown, true);
    window.addEventListener("keydown", onKeydown, true);
    return () => {
      window.removeEventListener("pointerdown", onPointerDown, true);
      window.removeEventListener("keydown", onKeydown, true);
    };
  });

  // 画面遷移でメニューを開いたまま残さない
  $effect(() => {
    void page.url.pathname;
    closeVideoContextMenu();
  });

  // 常時マウント化で「閉じている間はコンポーネントごと無い」にできなく
  // なったため、非表示時の props には空のフォールバックを渡す
  const emptyVideo: VideoRef = {
    videoId: "",
    title: "",
    channelId: null,
    channelTitle: null,
    thumbnailUrl: null,
  };
</script>

<!-- メニューの開閉でリマウントされないよう常時マウントし hidden で
     切り替える（webkit2gtk のヒットずれ対策、FR-80・仕様決定 DE） -->
<div
  class="vctx"
  class:hidden={videoContextMenu.current === null}
  style="left: {pos.x}px; top: {pos.y}px; --vctx-max-h: {maxHeight}px;"
  bind:this={menuEl}
>
  <VideoActionsMenu
    video={videoContextMenu.current?.video ?? emptyVideo}
    playlists={videoContextMenu.current?.va
      ? videoContextMenu.current.va.playlists
      : (videoContextMenu.current?.playlists ?? [])}
    floating
    onplaylistcreated={(pl) => {
      if (videoContextMenu.current?.va) {
        videoContextMenu.current.va.onPlaylistCreated(pl);
      } else {
        if (videoContextMenu.current?.playlists) {
          videoContextMenu.current.playlists = [
            ...videoContextMenu.current.playlists,
            pl,
          ];
        }
        videoContextMenu.current?.onPlaylistCreated?.(pl);
      }
    }}
    onplaylistadd={videoContextMenu.current?.onplaylistadd}
    onclose={closeVideoContextMenu}
  />
</div>

<style>
  .vctx {
    position: fixed;
    /* キューパネル（z-index: 40）より手前、トースト（50）より奥 */
    z-index: 45;
  }
</style>
