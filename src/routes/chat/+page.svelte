<script lang="ts">
  // チャットポップアップ窓の本体（FR-27、仕様決定 AT・AU）。
  // chat_popup_open が /chat?v=<videoId>&i=<instanceId> で開く別
  // WebviewWindow にだけ表示される。表示行は投稿時刻と本文のみで、
  // 投稿者名と superchat / membership のバッジは出さない
  // （chat.popup_show_author が on のとき投稿者名も出す。FR-68・CS）。
  import { page } from "$app/state";
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { t } from "$lib/i18n";
  import ChatList from "$lib/ChatList.svelte";
  import {
    chatPopups,
    initChatEvents,
    openChatPopupView,
  } from "$lib/chat.svelte";
  import {
    asErrorMessage,
    initPlayerEvents,
    playerStates,
  } from "$lib/players.svelte";
  import { chatPopupOntopEnabled } from "$lib/pip";
  import {
    popupFontSize,
    popupOpacity,
    popupShowAuthorEnabled,
  } from "$lib/chat-popup-logic";

  const videoId = page.url.searchParams.get("v") ?? "";
  const instanceId = Number(page.url.searchParams.get("i") ?? "0");

  const panel = $derived(chatPopups.list.get(videoId));
  // 「パネルに戻す」は起票元インスタンスが同じ動画を再生中のときだけ有効
  // （終了済み・別動画へ切替済みなら戻ってもチャットは繋がらない）
  const canReturn = $derived(
    playerStates.list.get(instanceId)?.videoId === videoId,
  );

  let ontop = $state(true);
  let error = $state<string | null>(null);
  // 表示カスタマイズ（FR-68、仕様決定 CS）。既定は従来の見た目
  // （投稿者名なし・14px・不透明）
  let showAuthor = $state(false);
  let fontSize = $state(14);
  let opacity = $state(1.0);

  onMount(async () => {
    openChatPopupView(videoId);
    await initChatEvents();
    await initPlayerEvents();
    // 初期読み取りと設定変更の競合を避けるため、先に
    // settings://changed を購読してから settings_get する
    // （届いたイベントの値を正とする。+page.svelte と同じ型）
    let displayChanged = false;
    await listen<{ key: string; value: string }>(
      "settings://changed",
      (ev) => {
        const { key, value } = ev.payload;
        if (key === "chat.popup_show_author") {
          displayChanged = true;
          showAuthor = popupShowAuthorEnabled(value);
        } else if (key === "chat.popup_font_size") {
          displayChanged = true;
          fontSize = popupFontSize(value);
        } else if (key === "chat.popup_opacity") {
          displayChanged = true;
          opacity = popupOpacity(value);
        }
      },
    );
    try {
      const raw = await invoke<string | null>("settings_get", {
        key: "chat.popup_ontop",
      });
      ontop = chatPopupOntopEnabled(raw);
    } catch {
      // 設定が読めなくても既定（最前面 ON）のまま表示する
    }
    try {
      const [authorRaw, sizeRaw, opacityRaw] = await Promise.all([
        invoke<string | null>("settings_get", {
          key: "chat.popup_show_author",
        }),
        invoke<string | null>("settings_get", {
          key: "chat.popup_font_size",
        }),
        invoke<string | null>("settings_get", {
          key: "chat.popup_opacity",
        }),
      ]);
      // 購読中に届いたイベント側を正とし、初期読み取りで戻さない
      if (!displayChanged) {
        showAuthor = popupShowAuthorEnabled(authorRaw);
        fontSize = popupFontSize(sizeRaw);
        opacity = popupOpacity(opacityRaw);
      }
    } catch {
      // 読み取り失敗時は既定（従来の見た目）のままにする
    }
  });

  async function toggleOntop(): Promise<void> {
    const next = !ontop;
    try {
      await getCurrentWindow().setAlwaysOnTop(next);
      await invoke("settings_set", {
        key: "chat.popup_ontop",
        value: next ? "on" : "off",
      });
      ontop = next;
      error = null;
    } catch (e) {
      error = asErrorMessage(e);
    }
  }

  async function returnToPanel(): Promise<void> {
    try {
      // 成功時はバックエンド側でパネルの利用者登録・メイン窓への
      // `chat://open_panel` 発行・この窓のクローズまで行う
      await invoke("chat_popup_return", { videoId, instanceId });
    } catch (e) {
      error = asErrorMessage(e);
    }
  }
</script>

<div
  class="popup"
  style:--chat-font-size="{fontSize}px"
  style:opacity={opacity}
>
  <div class="popup-head">
    <h1>{t("chat.title")}</h1>
    <div class="popup-actions">
      <button
        class:active={ontop}
        title={t("chat.pin.hint")}
        onclick={() => void toggleOntop()}
      >
        {t("chat.pin")}
      </button>
      <button disabled={!canReturn} onclick={() => void returnToPanel()}>
        {t("chat.toPanel")}
      </button>
    </div>
  </div>
  <!-- 状態行・エラー行は出現・消失でリマウントされないよう
       hidden 切替（ヒットずれ対策、FR-80） -->
  <p class="chat-status" class:hidden={!panel?.status}>{panel?.status}</p>
  <p class="chat-status" class:hidden={!error}>{error}</p>
  <!-- 投稿者名は chat.popup_show_author で切替（FR-68、仕様決定 CS）。
       superchat / membership のバッジは設定化の対象外のため出さない -->
  <ChatList
    items={panel?.items ?? []}
    {showAuthor}
    showBadges={false}
    fillHeight
  />
</div>

<style>
  .popup {
    display: flex;
    flex-direction: column;
    height: 100vh;
    box-sizing: border-box;
    padding: 8px 12px;
  }

  .popup-head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 8px;
    flex-shrink: 0;
  }

  .popup-head h1 {
    font-size: 1rem;
    margin: 0;
  }

  .popup-actions {
    display: flex;
    gap: 8px;
  }

  .popup-actions button {
    border: 1px solid var(--border);
    background: none;
    color: var(--fg-dim);
    border-radius: 6px;
    padding: 2px 8px;
    font-size: 0.85rem;
    cursor: pointer;
  }

  .popup-actions button.active {
    border-color: var(--accent);
    color: var(--accent);
  }

  .popup-actions button:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .chat-status {
    color: var(--warn);
    font-size: 0.85rem;
    margin: 4px 0;
    flex-shrink: 0;
  }
</style>
