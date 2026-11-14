<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { t } from "$lib/i18n";
  import { asErrorMessage, type Filter } from "$lib/players.svelte";
  import { ngAuthorRows } from "$lib/nguser-logic";
  import { filterKindLabel } from "$lib/settings-consts";

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

  onMount(() => {
    void loadRows();
  });
</script>

  <section class="panel panel--filled">
    <h2>{t("settings.ngusers.title")}</h2>
    <p class="subtle desc">{t("settings.ngusers.desc")}</p>
    <div class="filter-form">
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
    <!-- 空表示と一覧の出し分けはリマウントを避けるため hidden 切替
         （ヒットずれ対策、FR-80） -->
    <p class="subtle" class:hidden={rows.length > 0}>
      {t("settings.ngusers.empty")}
    </p>
      <ul class="filter-list" class:hidden={rows.length === 0}>
        {#each rows as f (f.id)}
          <li>
            <code class="f-pattern">{f.pattern}</code>
            <span class="f-kind">{filterKindLabel(f.kind)}</span>
            <button class="link" onclick={() => removeUser(f)}>
              {t("settings.ngusers.remove")}
            </button>
          </li>
        {/each}
      </ul>
  </section>
