<script lang="ts">
  // カテゴリ並べ替えセクション（FR-63、仕様決定 CN）。
  // `categories.sort_order` を FR-35（プレイリスト）と同じ操作体系
  // （上下ボタンとドラッグ＆ドロップ）で更新する。
  // 保存は操作ごとに `category_reorder` で即時に行う（プレイリスト一覧の
  // 並べ替えと同じく保存ボタンを通さない。一覧の飛行中更新は直列化して
  // 最新順を追い送りする）
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { t } from "$lib/i18n";
  import { asErrorMessage, type Category } from "$lib/players.svelte";

  let { notify }: { notify: (msg: string) => void } = $props();

  let categories = $state<Category[]>([]);
  let dragFrom = $state<number | null>(null);
  let dragOver = $state<number | null>(null);
  let reorderBusy = $state(false);

  async function load(): Promise<void> {
    try {
      categories = await invoke<Category[]>("list_categories");
    } catch (e) {
      notify(t("settings.categories.reorderFailed", {
        message: asErrorMessage(e),
      }));
    }
  }

  // 並べ替え保存の直列化（FR-35・仕様決定 BF の一覧並べ替えと同じ構造）。
  // 飛行中に次の変更が来たら完了を待って最新順で追い送りする。
  // 失敗時は list_categories を取り直して確定済みの DB 順へ戻す
  let reorderRun: Promise<void> | null = null;
  let reorderAgain = false;

  async function persistOrder(): Promise<void> {
    if (reorderRun) {
      reorderAgain = true;
      return reorderRun;
    }
    reorderBusy = true;
    let failed = false;
    reorderRun = (async () => {
      try {
        for (;;) {
          reorderAgain = false;
          try {
            await invoke("category_reorder", {
              categoryIds: categories.map((c) => c.id),
            });
          } catch (e) {
            notify(
              t("settings.categories.reorderFailed", {
                message: asErrorMessage(e),
              }),
            );
            failed = true;
            return;
          }
          if (!reorderAgain) return;
        }
      } finally {
        reorderRun = null;
        reorderBusy = false;
      }
    })();
    await reorderRun;
    if (failed) {
      try {
        await load();
      } catch {
        // 取り直しの失敗は既存表示のまま
      }
    }
  }

  /// ローカルの一覧順を変えて保存する（並べ替え UI の共通部品）
  function applyOrder(from: number, to: number): void {
    const list = [...categories];
    const [m] = list.splice(from, 1);
    list.splice(to, 0, m);
    categories = list;
    void persistOrder();
  }

  /// 上下ボタン（FR-35 と同じ）。隣と交換して保存する
  function move(index: number, dir: -1 | 1): void {
    const to = index + dir;
    if (to < 0 || to >= categories.length) return;
    applyOrder(index, to);
  }

  function dragStart(index: number, e: DragEvent): void {
    dragFrom = index;
    e.dataTransfer?.setData("text/plain", String(categories[index].id));
    if (e.dataTransfer) e.dataTransfer.effectAllowed = "move";
  }

  function dragOverAt(index: number, e: DragEvent): void {
    if (dragFrom === null) return;
    e.preventDefault();
    dragOver = index;
  }

  function dragLeave(index: number): void {
    if (dragOver === index) dragOver = null;
  }

  function drop(index: number, e: DragEvent): void {
    if (dragFrom === null) return;
    e.preventDefault();
    if (dragFrom !== index) applyOrder(dragFrom, index);
    dragFrom = null;
    dragOver = null;
  }

  function dragEnd(): void {
    dragFrom = null;
    dragOver = null;
  }

  onMount(() => {
    void load();
  });
</script>

<section class="panel panel--filled">
  <h2>{t("settings.categories.title")}</h2>
  <p class="subtle desc">{t("settings.categories.desc")}</p>
  <!-- 空表示と一覧の出し分けはリマウントを避けるため hidden 切替
       （ヒットずれ対策、FR-80） -->
  <p class="subtle" class:hidden={categories.length > 0}>
    {t("settings.categories.empty")}
  </p>
  <p
    class="subtle drag-hint"
    class:hidden={categories.length === 0}
  >
    {t("settings.categories.dragHint")}
  </p>
    <ul class="cat-list" class:hidden={categories.length === 0}>
      {#each categories as c, index (c.id)}
        <li
          class:drop-target={dragOver === index && dragFrom !== index}
          draggable="true"
          ondragstart={(e) => dragStart(index, e)}
          ondragover={(e) => dragOverAt(index, e)}
          ondragleave={() => dragLeave(index)}
          ondrop={(e) => drop(index, e)}
          ondragend={() => dragEnd()}
        >
          <span class="cat-name">{c.name}</span>
          <button
            class="link"
            title={t("settings.categories.moveUp")}
            disabled={index === 0 || reorderBusy}
            onclick={() => move(index, -1)}>↑</button
          >
          <button
            class="link"
            title={t("settings.categories.moveDown")}
            disabled={index === categories.length - 1 || reorderBusy}
            onclick={() => move(index, 1)}>↓</button
          >
        </li>
      {/each}
    </ul>
</section>

<style>
  .drag-hint {
    font-size: 0.8rem;
  }

  .cat-list {
    list-style: none;
    margin: 0;
    padding: 0;
  }

  .cat-list li {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 4px 0;
    border-top: 1px solid var(--border-row);
  }

  .cat-list li[draggable="true"] {
    cursor: grab;
  }

  .cat-list li.drop-target {
    border-top: 2px solid var(--accent);
  }

  .cat-name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
