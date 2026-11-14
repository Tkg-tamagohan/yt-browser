<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { t, type MessageKey } from "$lib/i18n";
  import { asErrorMessage, type Filter } from "$lib/players.svelte";
  import { ngAuthorRows } from "$lib/nguser-logic";

  let { notify }: { notify: (msg: string) => void } = $props();

  // NG ユーザー（FR-70、仕様決定 CU）: chat_author フィルタの専用管理節。
  // NG 系種別（literal / regex）の chat_author 行だけを扱う。
  // highlight 種別の chat_author 行は発言を除去しないため対象外で、
  // 一覧は NG フィルタ節側で管理する（暫定解釈。決定記録参照）
  let rows = $state<Filter[]>([]);
  let pattern = $state("");

  async function loadRows(): Promise<void> {
    try {
      rows = ngAuthorRows(await invoke<Filter[]>("filter_list"));
    } catch {
      rows = [];
    }
  }

  async function addUser(): Promise<void> {
    const p = pattern.trim();
    if (!p) return;
    try {
      // 専用節からの追加は部分一致（literal）に固定する。
      // 正規表現で絞りたい場合は NG フィルタ節で target=chat_author を登録する
      await invoke("filter_add", {
        target: "chat_author",
        kind: "literal",
        pattern: p,
      });
      pattern = "";
      await loadRows();
      notify(t("settings.ngusers.added"));
    } catch (e) {
      notify(t("settings.ngusers.addFailed", { message: asErrorMessage(e) }));
    }
  }

  async function removeUser(f: Filter): Promise<void> {
    try {
      await invoke("filter_remove", { id: f.id });
      rows = rows.filter((x) => x.id !== f.id);
      notify(t("settings.ngusers.removed"));
    } catch (e) {
      notify(t("settings.ngusers.removeFailed", { message: asErrorMessage(e) }));
    }
  }

  /// 種別の日本語ラベル。未定義の値は生値をそのまま出す
  /// （SettingsFiltersSection と同じ方式）
  function kindLabel(kind: string): string {
    const key = `filter.kind.${kind}` as MessageKey;
    const s = t(key);
    return s === key ? kind : s;
  }

  onMount(() => {
    void loadRows();
  });
</script>

  <section class="panel panel--filled">
    <h2>{t("settings.ngusers.title")}</h2>
    <p class="subtle desc">{t("settings.ngusers.desc")}</p>
    <div class="nguser-form">
      <input
        type="text"
        class="pattern-input"
        bind:value={pattern}
        placeholder={t("settings.ngusers.placeholder")}
        onkeydown={(e) => e.key === "Enter" && addUser()}
      />
      <button onclick={addUser} disabled={!pattern.trim()}>
        {t("settings.ngusers.add")}
      </button>
    </div>
    {#if rows.length === 0}
      <p class="subtle">{t("settings.ngusers.empty")}</p>
    {:else}
      <ul class="nguser-list">
        {#each rows as f (f.id)}
          <li>
            <code class="f-pattern">{f.pattern}</code>
            <span class="f-kind">{kindLabel(f.kind)}</span>
            <button class="link" onclick={() => removeUser(f)}>
              {t("settings.ngusers.remove")}
            </button>
          </li>
        {/each}
      </ul>
    {/if}
  </section>

<style>
  h2 {
    font-size: 1.1rem;
    margin: 0 0 4px;
  }

  .desc {
    font-size: 0.9rem;
    margin-top: 0;
  }

  .nguser-form {
    display: flex;
    gap: 8px;
    margin: 12px 0;
    flex-wrap: wrap;
  }

  .pattern-input {
    flex: 1;
    min-width: 200px;
  }

  .nguser-list {
    list-style: none;
    padding: 0;
    margin: 8px 0 0;
  }

  .nguser-list li {
    display: flex;
    gap: 10px;
    align-items: baseline;
    padding: 4px 0;
    font-size: 0.9rem;
  }

  .f-pattern {
    color: #e8eaed;
    overflow-wrap: anywhere;
    flex: 1;
  }

  .f-kind {
    color: #9aa0a6;
    font-size: 0.8rem;
    white-space: nowrap;
  }
</style>
