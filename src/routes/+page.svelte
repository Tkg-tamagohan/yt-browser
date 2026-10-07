<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";

  type DbStatus = { schemaVersion: number };

  let dbStatus = $state<DbStatus | null>(null);
  let dbError = $state("");

  onMount(async () => {
    try {
      dbStatus = await invoke<DbStatus>("db_status");
    } catch (e) {
      dbError = String(e);
    }
  });
</script>

<main class="container">
  <h1>yt-browser</h1>
  <p class="lead">mpv による軽量な再生とローカル完結のデータ管理を一体化した YouTube 専用ブラウザ。</p>

  {#if dbStatus}
    <p class="status ok">DB 接続: OK（スキーマ v{dbStatus.schemaVersion}）</p>
  {:else if dbError}
    <p class="status ng">DB 接続: 失敗 — {dbError}</p>
  {:else}
    <p class="status">DB 接続を確認中…</p>
  {/if}
</main>

<style>
  :root {
    font-family: Inter, "Hiragino Kaku Gothic ProN", "Noto Sans CJK JP", Meiryo, sans-serif;
    font-size: 16px;
    line-height: 1.6;
    color: #e8e8e8;
    background-color: #1a1b1e;
    font-synthesis: none;
    text-rendering: optimizeLegibility;
    -webkit-font-smoothing: antialiased;
  }

  .container {
    margin: 0 auto;
    max-width: 640px;
    padding: 12vh 24px 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    text-align: center;
  }

  h1 {
    font-size: 2rem;
    margin-bottom: 0.5rem;
  }

  .lead {
    color: #9aa0a6;
    margin-bottom: 2.5rem;
  }

  .status {
    padding: 0.5rem 1rem;
    border-radius: 8px;
    background: #26282c;
    font-family: monospace;
  }

  .status.ok {
    color: #7ee787;
  }

  .status.ng {
    color: #ff7b72;
  }
</style>
