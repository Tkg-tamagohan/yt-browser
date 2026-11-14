<script lang="ts">
  // チャットパネルの描画。リマウント回避（webkit2gtk のヒットずれ対策、
  // FR-80・仕様決定 DE）で常時マウントされ、表示はカード側がラッパの
  // hidden で切り替える。
  // 表示済みメッセージは chat.svelte.ts の共有ストアにあり、
  // 閉じている間も受信は蓄積され続ける。
  import { t } from "$lib/i18n";
  import { chatPanels, openChatPopup } from "$lib/chat.svelte";
  import ChatList from "$lib/ChatList.svelte";

  let { instanceId, videoId }: { instanceId: number; videoId: string } =
    $props();

  const panel = $derived(chatPanels.list.get(instanceId));
</script>

<div class="chat-panel sub-panel">
  <div class="chat-head">
    <h3>{t("chat.title")}</h3>
    <!-- ポップアップ窓へ切り替える（FR-27）。開くとこのパネルは閉じる -->
    <button
      class="link"
      title={t("chat.popup.hint")}
      onclick={() => void openChatPopup(instanceId, videoId)}
    >
      {t("chat.popup")}
    </button>
  </div>
  <!-- 状態行もリマウントを避けるため hidden で出し分ける（FR-80） -->
  <p class="chat-status" class:hidden={!panel?.status}>{panel?.status}</p>
  <ChatList items={panel?.items ?? []} />
</div>

<style>
  .chat-head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    margin-bottom: 8px;
  }

  .chat-panel h3 {
    font-size: 1rem;
    margin: 0;
  }

  .chat-head .link {
    border: none;
    background: none;
    color: var(--accent);
    font-size: 0.85rem;
    cursor: pointer;
    padding: 0;
  }

  .chat-status {
    color: var(--warn);
    font-size: 0.85rem;
    margin: 4px 0;
  }
</style>
