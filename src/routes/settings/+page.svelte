<script lang="ts">
  import { beforeNavigate, goto } from "$app/navigation";
  import { onDestroy, onMount, tick } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { t, TONE_MAPPING_MPV } from "$lib/i18n";
  import {
    PIP_GEOMETRY_DEFAULT,
    isValidPipGeometry,
    pipDefaultEnabled,
    pipFitAspectEnabled,
  } from "$lib/pip";
  import { QUALITY_PRESETS } from "$lib/quality";
  import {
    feedAutoReadDays as parseFeedAutoReadDays,
    feedAutoReadPlayedEnabled,
    feedNotifyNewEnabled,
    feedShowShortsEnabled,
  } from "$lib/feed-settings";
  import {
    popupCloseOnEndEnabled,
    popupShowAuthorEnabled,
  } from "$lib/chat-popup-logic";
  import { closeOnEofEnabled } from "$lib/eof-logic";
  import { shortcutsEnabled as parseShortcutsEnabled } from "$lib/shortcuts-logic";
  import { refreshShortcuts } from "$lib/shortcuts.svelte";
  import {
    decideUnsavedLeave,
    playerNavigateOnPlay as parseNavigateOnPlay,
    uiConfirmUnsaved as parseConfirmUnsaved,
    uiDensity as parseUiDensity,
    uiLanguage as parseUiLanguage,
    uiTheme as parseUiTheme,
  } from "$lib/ui-logic";
  import { refreshUiPrefs } from "$lib/ui-prefs";
  import {
    asErrorMessage,
    formatOverrides,
    initPlayerEvents,
    isUiErrorCode,
    playerStates,
  } from "$lib/players.svelte";
  import { notify as pushGlobalToast } from "$lib/notices.svelte";
  import {
    CUSTOM,
    HWDEC_MODES,
    SPONSOR_CATEGORIES,
    TONE_MAPPINGS,
  } from "$lib/settings-consts";
  import {
    normalizeOptionalRaw,
    saveOptionalRangeSetting,
    saveSettingBatch,
    saveStringSetting,
    saveToggleSetting,
    validateAutoReadDays,
    validateWheelDelta,
    type BatchEntry,
    type SetterFn,
  } from "$lib/settings-save-logic";
  import {
    cloneSettingsFormState,
    defaultSettingsFormState,
    diffSettingsFormState,
    effectiveQualityFormat,
    type SettingsFormState,
  } from "$lib/settings-diff-logic";
  import SettingsBlockedSection from "$lib/SettingsBlockedSection.svelte";
  import SettingsCategoriesSection from "$lib/SettingsCategoriesSection.svelte";
  import SettingsChatPopupSection from "$lib/SettingsChatPopupSection.svelte";
  import SettingsDepsSection from "$lib/SettingsDepsSection.svelte";
  import SettingsDiagSection from "$lib/SettingsDiagSection.svelte";
  import SettingsFeedSection from "$lib/SettingsFeedSection.svelte";
  import SettingsFiltersSection from "$lib/SettingsFiltersSection.svelte";
  import SettingsHdrSection from "$lib/SettingsHdrSection.svelte";
  import SettingsLiveWaitSection from "$lib/SettingsLiveWaitSection.svelte";
  import SettingsNgUsersSection from "$lib/SettingsNgUsersSection.svelte";
  import SettingsNoticesSection from "$lib/SettingsNoticesSection.svelte";
  import SettingsOpsSection from "$lib/SettingsOpsSection.svelte";
  import SettingsPipSection from "$lib/SettingsPipSection.svelte";
  import SettingsPlayerSection from "$lib/SettingsPlayerSection.svelte";
  import SettingsQualitySection from "$lib/SettingsQualitySection.svelte";
  import SettingsShortcutsSection from "$lib/SettingsShortcutsSection.svelte";
  import SettingsSponsorSection from "$lib/SettingsSponsorSection.svelte";
  import SettingsUiSection from "$lib/SettingsUiSection.svelte";
  import SettingsUpdateSection from "$lib/SettingsUpdateSection.svelte";
  import SettingsWheelSection from "$lib/SettingsWheelSection.svelte";
  import SettingsWindowSection from "$lib/SettingsWindowSection.svelte";
  import type { MonitorInfo } from "$lib/monitors";
  import {
    filterSectionIds,
    resolveSectionDefs,
  } from "$lib/settings-filter";

  // 設計書 §4.3 のプリセット表（プレイヤーカードの画質選択と共有）
  const PRESETS = QUALITY_PRESETS;

  // 編集中のフォーム状態と、DB 保存済みの基準スナップショット（FR-90、仕様決定 DS）。
  // 含まれる設定項目:
  // - quality.format（selected / customFormat）
  // - sponsor.categories（カテゴリごとの "skip" | "notify" | "off"、既定は sponsor のみ skip）
  // - sponsor.show_segments（シークバーへの区間帯表示、FR-36、既定 on）
  // - wheel.volume_delta（mpv script-opts の wheel-volume_delta。次回再生から有効。既定 2）
  // - pip.default（再生の既定表示を PiP にするか、仕様決定 AJ、既定 on）
  // - pip.geometry（PiP 小窓の --geometry 値。空欄での保存は既定値へ戻す）
  // - pip.fit_aspect（PiP 小窓を動画アスペクト比へ追従させるか、仕様決定 AL、既定 on）
  // - pip.quality.format（PiP 既定画質式。空欄は全体の画質設定に従う、仕様決定 W）
  // - feed.show_shorts（フィード一覧に Shorts を表示するか、仕様決定 AP、既定 off）
  // - feed.notify_new（新着検出でデスクトップ通知するか、FR-58・仕様決定 CI、既定 off）
  // - feed.auto_read_played（再生済み項目の自動既読、FR-64・仕様決定 CO、既定 off）
  // - feed.auto_read_days（N 日経過での自動既読、FR-64・仕様決定 CO、0 は無効、空欄は無効扱い）
  // - live_wait.action（待機中配信の開始時動作、FR-44・仕様決定 BT(3)、既定 play）
  // - ui.shortcuts（アプリ側キーボードショートカット、FR-79・仕様決定 DD、既定 off）
  // - chat.popup_show_author / chat.popup_font_size / chat.popup_opacity（FR-68・仕様決定 CS）
  // - player.initial_speed / player.initial_volume（FR-39・仕様決定 BO）
  // - player.navigate_on_play（再生開始時の再生画面への遷移、FR-92・仕様決定 DU、既定 on）
  // - window.screen / player.close_on_eof（FR-31・仕様決定 AZ、FR-43・仕様決定 BS）
  // - ui.theme / ui.density / ui.language（FR-76・FR-77・FR-81、仕様決定 DA・DB・DF）
  // - ui.confirm_unsaved（未保存タブ移動時の確認、FR-91・仕様決定 DT、既定 on）
  // - hdr.tone_mapping / hdr.compute_peak / mpv.extra_args / mpv.network_timeout_ms / mpv.hwdec（仕様決定 Y・BE）
  // - ytdlp.path / ytdlp.po_token（FR-72・仕様決定 CW、FR-86・仕様決定 DK）
  let form = $state<SettingsFormState>(defaultSettingsFormState());
  let savedSnapshot = $state<SettingsFormState>(defaultSettingsFormState());

  let loading = $state(true);
  let saving = $state(false);
  let notices = $state<string[]>([]);
  // DB に最後に書き込んだ画質式（読込時点では読み取った値）。
  // 書込み済みと適用完了を分けて追うため、即時適用の成否とは独立に進める
  let persistedFormat = $state("");
  // monitor_list の列挙結果。取得失敗環境では空列のまま（選択肢が無効になる）
  let monitors = $state<MonitorInfo[]>([]);
  // 稼働中インスタンスへの即時適用が残っている画質式。全台に適用できたら null
  let pendingApply = $state<string | null>(null);

  // 未保存タブ移動時の確認ダイアログ状態（FR-91、仕様決定 DT）
  let pendingNavigation = $state<URL | null>(null);
  let allowNavigationOnce = false;
  let dialogSaving = $state(false);
  let cancelDialogBtn: HTMLButtonElement | undefined = $state();

  // 節フィルタの絞り込み入力（FR-73、仕様決定 CX）。節のラベル文字列への
  // 大小文字を区別しない部分一致で表示する節を絞る
  let sectionQuery = $state("");
  // 節の絞り込みで非表示にするだけなら hidden で切り替える
  // （再マウントで各節の状態や取得済みデータを失わないため）
  const visibleSectionIds = $derived(
    filterSectionIds(sectionQuery, resolveSectionDefs(t)),
  );

  // 現在の変更一覧（読込中は空、言語・モニター取得後に即座に文言追従する）
  const changes = $derived(
    loading
      ? []
      : diffSettingsFormState(savedSnapshot, form, { t, monitors }),
  );
  const hasChanges = $derived(changes.length > 0);

  let disposed = false;
  let loadSeq = 0;
  const noticeTimers = new Map<string, ReturnType<typeof setTimeout>>();

  function notify(msg: string): void {
    if (disposed) return;
    const prevTimer = noticeTimers.get(msg);
    if (prevTimer !== undefined) clearTimeout(prevTimer);
    if (!notices.includes(msg)) {
      notices = [...notices.slice(-4), msg];
    }
    const timer = setTimeout(() => {
      noticeTimers.delete(msg);
      if (!disposed) {
        notices = notices.filter((n) => n !== msg);
      }
    }, 6000);
    noticeTimers.set(msg, timer);
  }

  onDestroy(() => {
    disposed = true;
    for (const timer of noticeTimers.values()) clearTimeout(timer);
    noticeTimers.clear();
  });

  function resetChanges(): void {
    if (loading || saving || !hasChanges) return;
    form = cloneSettingsFormState(savedSnapshot);
  }

  /// 変更のある項目のみ DB へ保存し、成功した項目のスナップショットを進める。
  /// 全変更が保存し終えた場合のみ true を返す。
  async function save(toastOnLeave = false): Promise<boolean> {
    if (loading || saving) return false;
    saving = true;
    let invalid = false;
    let qualitySkipped = false;

    const emitSuccess = (msg: string) => {
      notify(msg);
      if (toastOnLeave) pushGlobalToast(msg);
    };

    try {
      const setSetting: SetterFn = (key, value) =>
        invoke("settings_set", { key, value });
      const failKey = (msg: string) => {
        invalid = true;
        notify(t("settings.failed", { message: msg }));
      };

      // 1. quality.format（カテゴリだけの保存で再生中インスタンスが再読込されないよう変更時のみ書込）
      const format = effectiveQualityFormat(form.selected, form.customFormat);
      const emptyCustom = form.selected === CUSTOM && format === "";
      if (emptyCustom) {
        // 空のカスタム式は変更しない既存規則に合わせ、画面表示も直近の保存値へ揃える
        qualitySkipped = true;
        form.selected = savedSnapshot.selected;
        form.customFormat = savedSnapshot.customFormat;
      } else if (format && format !== persistedFormat) {
        const snapSelected = form.selected;
        const snapCustom = form.customFormat;
        await setSetting("quality.format", format);
        persistedFormat = format;
        pendingApply = format;
        if (effectiveQualityFormat(form.selected, form.customFormat) === format) {
          savedSnapshot.selected = snapSelected;
          savedSnapshot.customFormat = snapCustom;
        } else {
          invalid = true;
          notify(t("settings.failed", { message: "quality.format" }));
        }
      }

      // 2. sponsor.categories / sponsor.show_segments
      const sponsorChanged = SPONSOR_CATEGORIES.some(
        (c) => form.sponsorActions[c] !== savedSnapshot.sponsorActions[c],
      );
      if (sponsorChanged) {
        const snapSponsor = { ...form.sponsorActions };
        await setSetting("sponsor.categories", JSON.stringify(snapSponsor));
        const stillSame = SPONSOR_CATEGORIES.every(
          (c) => form.sponsorActions[c] === snapSponsor[c],
        );
        if (stillSame) {
          savedSnapshot.sponsorActions = snapSponsor;
        } else {
          invalid = true;
          notify(t("settings.failed", { message: "sponsor.categories" }));
        }
      }
      if (form.sponsorShowSegments !== savedSnapshot.sponsorShowSegments) {
        const snapShow = form.sponsorShowSegments;
        if (
          !(await saveToggleSetting(
            setSetting,
            "sponsor.show_segments",
            snapShow,
            () => form.sponsorShowSegments,
            () => notify(t("settings.failed", { message: "sponsor.show_segments" })),
            () => {
              savedSnapshot.sponsorShowSegments = snapShow;
            },
          ))
        ) {
          invalid = true;
        }
      }

      // 3. wheel.volume_delta: 有限数値・±100 以内だけ保存する（空欄はスキップ）
      if (form.wheelDelta !== savedSnapshot.wheelDelta) {
        const wdSnapshot = form.wheelDelta;
        const wd = validateWheelDelta(wdSnapshot);
        if (wd.skip) {
          // 空欄時は保存スキップとなるため、未保存残存を防ぐべく画面値を保存済み値へ戻す
          if (form.wheelDelta === undefined) {
            form.wheelDelta = savedSnapshot.wheelDelta;
          }
        } else if (wd.ok) {
          await setSetting("wheel.volume_delta", wd.value);
          if (form.wheelDelta === wdSnapshot) {
            savedSnapshot.wheelDelta = wdSnapshot;
          } else {
            invalid = true;
            notify(t("settings.failed", { message: "wheel.volume_delta" }));
          }
        } else {
          failKey(`wheel.volume_delta: ${wd.raw}`);
        }
      }

      // 4. pip.*（仕様決定 AJ・AL）
      if (form.pipDefault !== savedSnapshot.pipDefault) {
        const snap = form.pipDefault;
        if (
          !(await saveToggleSetting(
            setSetting,
            "pip.default",
            snap,
            () => form.pipDefault,
            () => notify(t("settings.pip.unsaved")),
            () => {
              savedSnapshot.pipDefault = snap;
            },
          ))
        ) {
          invalid = true;
        }
      }
      if (form.pipFitAspect !== savedSnapshot.pipFitAspect) {
        const snap = form.pipFitAspect;
        if (
          !(await saveToggleSetting(
            setSetting,
            "pip.fit_aspect",
            snap,
            () => form.pipFitAspect,
            () => notify(t("settings.pip.unsaved")),
            () => {
              savedSnapshot.pipFitAspect = snap;
            },
          ))
        ) {
          invalid = true;
        }
      }
      if (form.pipGeometry.trim() !== savedSnapshot.pipGeometry.trim()) {
        const geo = form.pipGeometry.trim();
        if (!geo) {
          // 空欄は既定値へのリセット。保存成功後に画面値も戻す
          await setSetting("pip.geometry", PIP_GEOMETRY_DEFAULT);
          if (!form.pipGeometry.trim()) {
            form.pipGeometry = PIP_GEOMETRY_DEFAULT;
            savedSnapshot.pipGeometry = PIP_GEOMETRY_DEFAULT;
          } else {
            invalid = true;
            notify(t("settings.pip.unsaved"));
          }
        } else if (isValidPipGeometry(geo)) {
          await setSetting("pip.geometry", geo);
          if (form.pipGeometry.trim() === geo) {
            form.pipGeometry = geo;
            savedSnapshot.pipGeometry = geo;
          } else {
            invalid = true;
            notify(t("settings.pip.unsaved"));
          }
        } else {
          failKey(`pip.geometry: ${geo}`);
        }
      }
      if (form.pipQuality.trim() !== savedSnapshot.pipQuality.trim()) {
        const snap = form.pipQuality.trim();
        if (
          !(await saveStringSetting(
            setSetting,
            "pip.quality.format",
            snap,
            () => form.pipQuality.trim(),
            () => notify(t("settings.pip.unsaved")),
            () => {
              savedSnapshot.pipQuality = snap;
            },
          ))
        ) {
          invalid = true;
        }
      }

      // 5. feed.*（仕様決定 AP・FR-58/CI・FR-64/CO）
      if (form.feedShowShorts !== savedSnapshot.feedShowShorts) {
        const snap = form.feedShowShorts;
        if (
          !(await saveToggleSetting(
            setSetting,
            "feed.show_shorts",
            snap,
            () => form.feedShowShorts,
            () => notify(t("settings.feed.unsaved")),
            () => {
              savedSnapshot.feedShowShorts = snap;
            },
          ))
        ) {
          invalid = true;
        }
      }
      if (form.feedNotifyNew !== savedSnapshot.feedNotifyNew) {
        const snap = form.feedNotifyNew;
        if (
          !(await saveToggleSetting(
            setSetting,
            "feed.notify_new",
            snap,
            () => form.feedNotifyNew,
            () => notify(t("settings.feed.unsaved")),
            () => {
              savedSnapshot.feedNotifyNew = snap;
            },
          ))
        ) {
          invalid = true;
        }
      }
      if (form.feedAutoReadPlayed !== savedSnapshot.feedAutoReadPlayed) {
        const snap = form.feedAutoReadPlayed;
        if (
          !(await saveToggleSetting(
            setSetting,
            "feed.auto_read_played",
            snap,
            () => form.feedAutoReadPlayed,
            () => notify(t("settings.feed.unsaved")),
            () => {
              savedSnapshot.feedAutoReadPlayed = snap;
            },
          ))
        ) {
          invalid = true;
        }
      }
      if (form.feedAutoReadDays !== savedSnapshot.feedAutoReadDays) {
        const adSnapshot = form.feedAutoReadDays;
        const adRes = validateAutoReadDays(adSnapshot);
        if (adRes.ok) {
          await setSetting("feed.auto_read_days", adRes.value);
          if (form.feedAutoReadDays !== adSnapshot) {
            invalid = true;
            notify(t("settings.feed.unsaved"));
          } else {
            const normalized = adSnapshot ?? 0;
            form.feedAutoReadDays = normalized;
            savedSnapshot.feedAutoReadDays = normalized;
          }
        } else {
          failKey(`feed.auto_read_days: ${adRes.raw}`);
        }
      }

      // 6. live_wait.action / ui.shortcuts
      if (form.liveWaitAction !== savedSnapshot.liveWaitAction) {
        const snap = form.liveWaitAction;
        if (
          !(await saveStringSetting(
            setSetting,
            "live_wait.action",
            snap,
            () => form.liveWaitAction,
            () => notify(t("settings.livewait.unsaved")),
            () => {
              savedSnapshot.liveWaitAction = snap;
            },
          ))
        ) {
          invalid = true;
        }
      }
      if (form.shortcutsOn !== savedSnapshot.shortcutsOn) {
        const snap = form.shortcutsOn;
        if (
          !(await saveToggleSetting(
            setSetting,
            "ui.shortcuts",
            snap,
            () => form.shortcutsOn,
            () => notify(t("settings.shortcuts.unsaved")),
            () => {
              savedSnapshot.shortcutsOn = snap;
            },
          ))
        ) {
          invalid = true;
        }
      }

      // 7. chat.popup_*（FR-68、仕様決定 CS）
      if (form.chatShowAuthor !== savedSnapshot.chatShowAuthor) {
        const snap = form.chatShowAuthor;
        if (
          !(await saveToggleSetting(
            setSetting,
            "chat.popup_show_author",
            snap,
            () => form.chatShowAuthor,
            () => notify(t("settings.chatpopup.unsaved")),
            () => {
              savedSnapshot.chatShowAuthor = snap;
            },
          ))
        ) {
          invalid = true;
        }
      }
      if (form.chatFontSize.trim() !== savedSnapshot.chatFontSize.trim()) {
        const snap = form.chatFontSize.trim();
        if (
          !(await saveOptionalRangeSetting(
            setSetting,
            "chat.popup_font_size",
            snap,
            () => form.chatFontSize,
            10,
            32,
            (raw) => failKey(`chat.popup_font_size: ${raw}`),
            () => notify(t("settings.chatpopup.unsaved")),
            () => {
              savedSnapshot.chatFontSize = snap;
            },
          ))
        ) {
          invalid = true;
        }
      }
      if (form.chatOpacity.trim() !== savedSnapshot.chatOpacity.trim()) {
        const snap = form.chatOpacity.trim();
        if (
          !(await saveOptionalRangeSetting(
            setSetting,
            "chat.popup_opacity",
            snap,
            () => form.chatOpacity,
            0.2,
            1.0,
            (raw) => failKey(`chat.popup_opacity: ${raw}`),
            () => notify(t("settings.chatpopup.unsaved")),
            () => {
              savedSnapshot.chatOpacity = snap;
            },
          ))
        ) {
          invalid = true;
        }
      }
      if (form.chatCloseOnEnd !== savedSnapshot.chatCloseOnEnd) {
        const snap = form.chatCloseOnEnd;
        if (
          !(await saveToggleSetting(
            setSetting,
            "chat.popup_close_on_end",
            snap,
            () => form.chatCloseOnEnd,
            () => notify(t("settings.chatpopup.unsaved")),
            () => {
              savedSnapshot.chatCloseOnEnd = snap;
            },
          ))
        ) {
          invalid = true;
        }
      }

      // 8. player.*（FR-39/BO、FR-92/DU）
      if (
        normalizeOptionalRaw(form.initialSpeed) !==
        normalizeOptionalRaw(savedSnapshot.initialSpeed)
      ) {
        const snap = form.initialSpeed;
        const committed =
          snap !== undefined && Number.isFinite(snap) ? snap : undefined;
        if (
          !(await saveOptionalRangeSetting(
            setSetting,
            "player.initial_speed",
            snap,
            () => form.initialSpeed,
            0.25,
            4.0,
            (raw) => failKey(`player.initial_speed: ${raw}`),
            () => notify(t("settings.player.unsaved")),
            () => {
              savedSnapshot.initialSpeed = committed;
            },
          ))
        ) {
          invalid = true;
        }
      }
      if (
        normalizeOptionalRaw(form.initialVolume) !==
        normalizeOptionalRaw(savedSnapshot.initialVolume)
      ) {
        const snap = form.initialVolume;
        const committed =
          snap !== undefined && Number.isFinite(snap) ? snap : undefined;
        if (
          !(await saveOptionalRangeSetting(
            setSetting,
            "player.initial_volume",
            snap,
            () => form.initialVolume,
            0,
            130,
            (raw) => failKey(`player.initial_volume: ${raw}`),
            () => notify(t("settings.player.unsaved")),
            () => {
              savedSnapshot.initialVolume = committed;
            },
          ))
        ) {
          invalid = true;
        }
      }
      if (form.navigateOnPlay !== savedSnapshot.navigateOnPlay) {
        const snap = form.navigateOnPlay;
        if (
          !(await saveToggleSetting(
            setSetting,
            "player.navigate_on_play",
            snap,
            () => form.navigateOnPlay,
            () => notify(t("settings.player.unsaved")),
            () => {
              savedSnapshot.navigateOnPlay = snap;
            },
          ))
        ) {
          invalid = true;
        }
      }

      // 9. window.screen / player.close_on_eof（FR-31/AZ・FR-43/BS）
      if (form.windowScreen !== savedSnapshot.windowScreen) {
        const snap = form.windowScreen;
        if (
          !(await saveStringSetting(
            setSetting,
            "window.screen",
            snap,
            () => form.windowScreen,
            () => notify(t("settings.window.unsaved")),
            () => {
              savedSnapshot.windowScreen = snap;
            },
          ))
        ) {
          invalid = true;
        }
      }
      if (form.closeOnEof !== savedSnapshot.closeOnEof) {
        const snap = form.closeOnEof;
        if (
          !(await saveToggleSetting(
            setSetting,
            "player.close_on_eof",
            snap,
            () => form.closeOnEof,
            () => notify(t("settings.window.unsaved")),
            () => {
              savedSnapshot.closeOnEof = snap;
            },
          ))
        ) {
          invalid = true;
        }
      }

      // 10. ui.theme / ui.density / ui.language / ui.confirm_unsaved
      // （FR-76・FR-77・FR-81・FR-91、DA・DB・DF・DT）
      const uiEntries: BatchEntry[] = [];
      const snapTheme = form.uiTheme;
      const snapDensity = form.uiDensity;
      const snapLanguage = form.uiLanguage;
      if (snapTheme !== savedSnapshot.uiTheme) {
        uiEntries.push({
          key: "ui.theme",
          snapshot: snapTheme,
          getCurrent: () => form.uiTheme,
          onCommitted: () => {
            savedSnapshot.uiTheme = snapTheme;
          },
        });
      }
      if (snapDensity !== savedSnapshot.uiDensity) {
        uiEntries.push({
          key: "ui.density",
          snapshot: snapDensity,
          getCurrent: () => form.uiDensity,
          onCommitted: () => {
            savedSnapshot.uiDensity = snapDensity;
          },
        });
      }
      if (snapLanguage !== savedSnapshot.uiLanguage) {
        uiEntries.push({
          key: "ui.language",
          snapshot: snapLanguage,
          getCurrent: () => form.uiLanguage,
          onCommitted: () => {
            savedSnapshot.uiLanguage = snapLanguage;
          },
        });
      }
      if (uiEntries.length > 0) {
        if (
          !(await saveSettingBatch(
            setSetting,
            uiEntries,
            () => notify(t("settings.ui.unsaved")),
          ))
        ) {
          invalid = true;
        }
      }
      if (form.confirmUnsaved !== savedSnapshot.confirmUnsaved) {
        const snap = form.confirmUnsaved;
        if (
          !(await saveToggleSetting(
            setSetting,
            "ui.confirm_unsaved",
            snap,
            () => form.confirmUnsaved,
            () => notify(t("settings.ui.unsaved")),
            () => {
              savedSnapshot.confirmUnsaved = snap;
            },
          ))
        ) {
          invalid = true;
        }
      }

      // 11. hdr.tone_mapping / hdr.compute_peak（選択肢 ID を TONE_MAPPING_MPV で変換）
      const hdrEntries: BatchEntry[] = [];
      const snapTone = form.toneMapping;
      const snapPeak = form.computePeak;
      if (snapTone !== savedSnapshot.toneMapping) {
        hdrEntries.push({
          key: "hdr.tone_mapping",
          snapshot: snapTone,
          writeValue: TONE_MAPPING_MPV[snapTone] ?? snapTone,
          getCurrent: () => form.toneMapping,
          onCommitted: () => {
            savedSnapshot.toneMapping = snapTone;
          },
        });
      }
      if (snapPeak !== savedSnapshot.computePeak) {
        hdrEntries.push({
          key: "hdr.compute_peak",
          snapshot: snapPeak,
          getCurrent: () => form.computePeak,
          onCommitted: () => {
            savedSnapshot.computePeak = snapPeak;
          },
        });
      }
      if (hdrEntries.length > 0) {
        if (
          !(await saveSettingBatch(
            setSetting,
            hdrEntries,
            () => notify(t("settings.hdr.unsaved")),
          ))
        ) {
          invalid = true;
        }
      }

      // 12. mpv.extra_args / mpv.network_timeout_ms / mpv.hwdec（仕様決定 Y・BE）
      if (form.mpvExtraArgs.trim() !== savedSnapshot.mpvExtraArgs.trim()) {
        const snap = form.mpvExtraArgs.trim();
        if (
          !(await saveStringSetting(
            setSetting,
            "mpv.extra_args",
            snap,
            () => form.mpvExtraArgs.trim(),
            () => notify(t("settings.hdr.unsaved")),
            () => {
              savedSnapshot.mpvExtraArgs = snap;
            },
          ))
        ) {
          invalid = true;
        }
      }
      if (
        normalizeOptionalRaw(form.mpvNetworkTimeout) !==
        normalizeOptionalRaw(savedSnapshot.mpvNetworkTimeout)
      ) {
        const snapNum = form.mpvNetworkTimeout;
        const snapText = normalizeOptionalRaw(snapNum);
        if (
          snapText !== "" &&
          (snapNum === undefined || !Number.isInteger(snapNum) || snapNum < 0)
        ) {
          failKey(`mpv.network_timeout_ms: ${snapText || String(snapNum)}`);
        } else {
          const committedVal =
            snapText === "" ? undefined : (snapNum as number);
          if (
            !(await saveStringSetting(
              setSetting,
              "mpv.network_timeout_ms",
              snapText,
              () => normalizeOptionalRaw(form.mpvNetworkTimeout),
              () => notify(t("settings.hdr.unsaved")),
              () => {
                savedSnapshot.mpvNetworkTimeout = committedVal;
              },
            ))
          ) {
            invalid = true;
          }
        }
      }
      if (form.mpvHwdec !== savedSnapshot.mpvHwdec) {
        const snap = form.mpvHwdec;
        if (
          !(await saveStringSetting(
            setSetting,
            "mpv.hwdec",
            snap,
            () => form.mpvHwdec,
            () => notify(t("settings.hdr.unsaved")),
            () => {
              savedSnapshot.mpvHwdec = snap;
            },
          ))
        ) {
          invalid = true;
        }
      }

      // 13. ytdlp.path / ytdlp.po_token（FR-72/CW・FR-86/DK）
      if (form.ytdlpPath.trim() !== savedSnapshot.ytdlpPath.trim()) {
        const snap = form.ytdlpPath.trim();
        if (
          !(await saveStringSetting(
            setSetting,
            "ytdlp.path",
            snap,
            () => form.ytdlpPath.trim(),
            () => notify(t("settings.deps.unsaved")),
            () => {
              savedSnapshot.ytdlpPath = snap;
            },
          ))
        ) {
          invalid = true;
        }
      }
      if (form.poToken.trim() !== savedSnapshot.poToken.trim()) {
        const snap = form.poToken.trim();
        if (
          !(await saveStringSetting(
            setSetting,
            "ytdlp.po_token",
            snap,
            () => form.poToken.trim(),
            () => notify(t("settings.deps.unsaved")),
            () => {
              savedSnapshot.poToken = snap;
            },
          ))
        ) {
          invalid = true;
        }
      }

      // 画質の即時適用（pendingApply）は不正値があっても最後まで実行する。
      // 書き込み済みの画質式が適用されないまま残るのを防ぐため、成功通知だけ抑える
      // （invalid には未保存変更の検出も含む）
      const activeFormat = effectiveQualityFormat(
        form.selected,
        form.customFormat,
      );
      if (!activeFormat || pendingApply !== activeFormat) {
        if (!invalid) {
          emitSuccess(
            qualitySkipped
              ? t("settings.savedQualitySkipped")
              : t("settings.saved"),
          );
        }
        return (
          !invalid &&
          diffSettingsFormState(savedSnapshot, form, { t, monitors }).length === 0
        );
      }
      // 再生中のインスタンスへ即時適用（設計書 §4.3: set_property + loadfile replace）
      let applied = 0;
      const failed: number[] = [];
      for (const id of playerStates.list.keys()) {
        // インスタンス別画質を指定済みの台は除外する（仕様決定 X の
        // セッション内有効な個別指定が全体既定で消えないようにする）
        if (formatOverrides.has(id)) continue;
        try {
          await invoke("player_control", {
            instanceId: id,
            action: { type: "quality", format: activeFormat },
          });
          applied += 1;
        } catch (e) {
          // 終了済みインスタンスは失敗に数えない:
          // - mpv_no_instance … エントリが既に消えている
          // - 共有マップに無い … player://ended が届いて終了を反映済み
          // それ以外の IPC エラーは稼働中インスタンスへの適用失敗として記録する
          if (
            !isUiErrorCode(e, "mpv_no_instance") &&
            playerStates.list.has(id)
          ) {
            failed.push(id);
          }
        }
      }
      if (failed.length > 0) {
        // 未適用の台が残るため pendingApply を残し、次回保存で適用を再試行できるようにする
        emitSuccess(t("settings.applyPartial", { count: failed.length }));
      } else {
        pendingApply = null;
        if (!invalid) {
          emitSuccess(
            applied > 0
              ? t("settings.applied", { count: applied })
              : t("settings.saved"),
          );
        }
      }
      return (
        !invalid &&
        diffSettingsFormState(savedSnapshot, form, { t, monitors }).length === 0
      );
    } catch (e) {
      notify(t("settings.failed", { message: asErrorMessage(e) }));
      return false;
    } finally {
      saving = false;
    }
  }

  const SETTING_KEYS = [
    "quality.format",
    "sponsor.categories",
    "sponsor.show_segments",
    "wheel.volume_delta",
    "pip.default",
    "pip.fit_aspect",
    "pip.geometry",
    "pip.quality.format",
    "feed.show_shorts",
    "feed.notify_new",
    "feed.auto_read_played",
    "feed.auto_read_days",
    "live_wait.action",
    "ui.shortcuts",
    "chat.popup_show_author",
    "chat.popup_font_size",
    "chat.popup_opacity",
    "chat.popup_close_on_end",
    "player.initial_speed",
    "player.initial_volume",
    "player.navigate_on_play",
    "window.screen",
    "player.close_on_eof",
    "ui.theme",
    "ui.density",
    "ui.language",
    "ui.confirm_unsaved",
    "hdr.tone_mapping",
    "hdr.compute_peak",
    "mpv.extra_args",
    "mpv.network_timeout_ms",
    "mpv.hwdec",
    "ytdlp.path",
    "ytdlp.po_token",
  ] as const;

  async function loadSettings(): Promise<void> {
    const seq = ++loadSeq;
    let readFailed = 0;
    const values = new Map<(typeof SETTING_KEYS)[number], string | null>();
    // モニター列挙と全設定キーの読み取りを並列化し、起動待ちを短縮する
    await Promise.all([
      invoke<MonitorInfo[]>("monitor_list")
        .then((list) => {
          if (!disposed && seq === loadSeq) monitors = list;
        })
        .catch(() => {
          if (!disposed && seq === loadSeq) monitors = [];
        }),
      ...SETTING_KEYS.map(async (key) => {
        try {
          const val = await invoke<string | null>("settings_get", { key });
          values.set(key, val);
        } catch {
          readFailed += 1;
          values.set(key, null);
        }
      }),
    ]);
    if (disposed || seq !== loadSeq) return;

    const raw = (key: (typeof SETTING_KEYS)[number]): string | null =>
      values.get(key) ?? null;

    const parseOptNum = (s: string | null): number | undefined => {
      if (s === null) return undefined;
      const trimmed = s.trim();
      if (trimmed === "") return undefined;
      const n = Number(trimmed);
      return Number.isFinite(n) ? n : undefined;
    };

    const next = defaultSettingsFormState();

    const stored = raw("quality.format");
    if (stored) {
      if (PRESETS.some((p) => p.format === stored)) {
        next.selected = stored;
      } else {
        next.selected = CUSTOM;
        next.customFormat = stored;
      }
    }
    const sponsorRaw = raw("sponsor.categories");
    const sponsorShowRaw = raw("sponsor.show_segments");
    const wheelRaw = raw("wheel.volume_delta");
    next.pipDefault = pipDefaultEnabled(raw("pip.default"));
    next.pipFitAspect = pipFitAspectEnabled(raw("pip.fit_aspect"));
    const pipGeoRaw = raw("pip.geometry");
    if (pipGeoRaw !== null && isValidPipGeometry(pipGeoRaw.trim())) {
      next.pipGeometry = pipGeoRaw.trim();
    }
    const pipQualityRaw = raw("pip.quality.format");
    if (pipQualityRaw !== null) {
      next.pipQuality = pipQualityRaw.trim();
    }
    next.feedShowShorts = feedShowShortsEnabled(raw("feed.show_shorts"));
    // feed.notify_new / feed.auto_read_*（FR-58・FR-64、仕様決定 CI・CO）。
    // 解釈は Rust 側と同じで、未設定・その他の値は既定（off / 0=無効）
    next.feedNotifyNew = feedNotifyNewEnabled(raw("feed.notify_new"));
    next.feedAutoReadPlayed = feedAutoReadPlayedEnabled(
      raw("feed.auto_read_played"),
    );
    next.feedAutoReadDays = parseFeedAutoReadDays(raw("feed.auto_read_days"));
    const liveWaitRaw = raw("live_wait.action");
    // 未設定・その他の値は play（仕様決定 BT(3) の既定）
    next.liveWaitAction = liveWaitRaw?.trim() === "notify" ? "notify" : "play";
    // ui.shortcuts（FR-79、仕様決定 DD）。解釈は feed.* 系の on/off と同じ
    next.shortcutsOn = parseShortcutsEnabled(raw("ui.shortcuts"));
    // chat.popup_*（FR-68、仕様決定 CS）。投稿者名は on 系のみ表示、
    // 文字サイズ・不透明度は保存文字列をそのまま欄に戻す
    next.chatShowAuthor = popupShowAuthorEnabled(raw("chat.popup_show_author"));
    const chatFontRaw = raw("chat.popup_font_size");
    if (chatFontRaw !== null) {
      next.chatFontSize = chatFontRaw.trim();
    }
    const chatOpacityRaw = raw("chat.popup_opacity");
    if (chatOpacityRaw !== null) {
      next.chatOpacity = chatOpacityRaw.trim();
    }
    next.chatCloseOnEnd = popupCloseOnEndEnabled(
      raw("chat.popup_close_on_end"),
    );
    next.initialSpeed = parseOptNum(raw("player.initial_speed"));
    next.initialVolume = parseOptNum(raw("player.initial_volume"));
    next.navigateOnPlay = parseNavigateOnPlay(raw("player.navigate_on_play"));
    const screenRaw = raw("window.screen");
    if (screenRaw !== null) {
      next.windowScreen = screenRaw.trim();
    }
    // player.close_on_eof（FR-43、仕様決定 BS）。解釈は Rust 側と同じ
    // 明示的な on 系値のみで、未設定・その他は off（既定）
    next.closeOnEof = closeOnEofEnabled(raw("player.close_on_eof"));
    // ui.theme / ui.density / ui.language / ui.confirm_unsaved
    // （FR-76・FR-77・FR-81・FR-91、仕様決定 DA・DB・DF・DT）
    next.uiTheme = parseUiTheme(raw("ui.theme"));
    next.uiDensity = parseUiDensity(raw("ui.density"));
    next.uiLanguage = parseUiLanguage(raw("ui.language"));
    next.confirmUnsaved = parseConfirmUnsaved(raw("ui.confirm_unsaved"));
    const toneRaw = raw("hdr.tone_mapping");
    if (toneRaw !== null) {
      const v = toneRaw.trim();
      // DB には mpv の値（bt.2390 等）が入るので、選択肢 ID へ逆引きする
      const id = Object.entries(TONE_MAPPING_MPV).find(
        ([, mpv]) => mpv === v,
      )?.[0];
      if (TONE_MAPPINGS.includes(v)) {
        next.toneMapping = v;
      } else if (id) {
        next.toneMapping = id;
      }
    }
    const peakRaw = raw("hdr.compute_peak");
    if (peakRaw !== null && ["auto", "yes", "no"].includes(peakRaw.trim())) {
      next.computePeak = peakRaw.trim();
    }
    const extraRaw = raw("mpv.extra_args");
    if (extraRaw !== null) {
      next.mpvExtraArgs = extraRaw;
    }
    next.mpvNetworkTimeout = parseOptNum(raw("mpv.network_timeout_ms"));
    const hwRaw = raw("mpv.hwdec");
    if (
      hwRaw !== null &&
      (HWDEC_MODES as readonly string[]).includes(hwRaw.trim())
    ) {
      next.mpvHwdec = hwRaw.trim();
    }
    // ytdlp.path（FR-72、仕様決定 CW）。空・未設定は自動検出
    const ytdlpPathRaw = raw("ytdlp.path");
    if (ytdlpPathRaw !== null) {
      next.ytdlpPath = ytdlpPathRaw.trim();
    }
    // ytdlp.po_token（FR-86、仕様決定 DK）。空・未設定は未設定
    const poTokenRaw = raw("ytdlp.po_token");
    if (poTokenRaw !== null) {
      next.poToken = poTokenRaw.trim();
    }
    if (wheelRaw !== null) {
      const n = Number(wheelRaw);
      if (Number.isFinite(n)) next.wheelDelta = n;
    }
    // 未設定・その他の値は on（仕様決定 BI）
    next.sponsorShowSegments = sponsorShowRaw !== "off";
    if (sponsorRaw) {
      try {
        const parsed = JSON.parse(sponsorRaw) as Record<string, string>;
        next.sponsorActions = {
          ...next.sponsorActions,
          ...Object.fromEntries(
            SPONSOR_CATEGORIES.filter((c) => c in parsed).map((c) => [
              c,
              parsed[c],
            ]),
          ),
        };
      } catch {
        // JSON 壊れは既定のままにする
      }
    }
    if (readFailed > 0) {
      notify(
        t("settings.failed", {
          message: `設定の読み取りに一部失敗しました（${readFailed} 件）`,
        }),
      );
    }
    form = next;
    savedSnapshot = cloneSettingsFormState(next);
    persistedFormat = effectiveQualityFormat(next.selected, next.customFormat);
    pendingApply = null;
    loading = false;
  }

  onMount(async () => {
    await initPlayerEvents();
    await loadSettings();
  });

  async function handleImported(): Promise<void> {
    await Promise.all([loadSettings(), refreshUiPrefs(), refreshShortcuts()]);
  }

  // 未保存の変更がある状態で別タブやアプリ外へ移動しようとしたときの警告
  // （FR-91、仕様決定 DT）。有効判定は保存済みの設定値を正とし、
  // 再読み込み・外部離脱時は willUnload / to === null の前に抑止判定を行う
  beforeNavigate((navigation) => {
    const decision = decideUnsavedLeave({
      allowOnce: allowNavigationOnce,
      loading,
      confirmUnsaved: savedSnapshot.confirmUnsaved,
      hasChanges,
      willUnload: navigation.willUnload,
      toPathname: navigation.to?.url.pathname ?? null,
    });
    if (decision === "allow") {
      if (allowNavigationOnce && !navigation.willUnload && navigation.to !== null) {
        allowNavigationOnce = false;
      }
      return;
    }
    navigation.cancel();
    if (decision === "cancelUnload" || navigation.to === null) {
      return;
    }
    pendingNavigation = navigation.to.url;
    void tick().then(() => {
      cancelDialogBtn?.focus();
    });
  });

  function cancelLeave(): void {
    if (dialogSaving) return;
    pendingNavigation = null;
  }

  function discardAndLeave(): void {
    if (dialogSaving || !pendingNavigation) return;
    const target = pendingNavigation;
    pendingNavigation = null;
    allowNavigationOnce = true;
    void goto(target);
  }

  async function saveAndLeave(): Promise<void> {
    if (dialogSaving || !pendingNavigation) return;
    const target = pendingNavigation;
    dialogSaving = true;
    try {
      const ok = await save(true);
      if (!ok) {
        // バリデーション失敗や保存中変更があった場合は移動せずエラー表示を残す
        pendingNavigation = null;
        return;
      }
      pendingNavigation = null;
      allowNavigationOnce = true;
      await goto(target);
    } finally {
      dialogSaving = false;
    }
  }

  function onDialogKeydown(e: KeyboardEvent): void {
    if (pendingNavigation !== null && e.key === "Escape" && !dialogSaving) {
      e.preventDefault();
      cancelLeave();
    }
  }
</script>

<svelte:window onkeydown={onDialogKeydown} />

<main class="container settings">
  <h1>{t("settings.title")}</h1>

  <div class="settings-layout">
    <div class="settings-main">
      <!-- 節フィルタ（FR-73、仕様決定 CX）。ラベル文字列への部分一致で
           表示する節を絞る。非表示は hidden 切替で、各節の状態は保持される -->
      <input
        class="section-filter"
        type="search"
        placeholder={t("settings.filter.placeholder")}
        bind:value={sectionQuery}
      />
      <!-- 該当なし表示もリマウントを避けるため hidden 切替（FR-80） -->
      <p
        class="subtle"
        class:hidden={sectionQuery.trim() === "" ||
          visibleSectionIds.size > 0}
      >
        {t("settings.filter.noMatch")}
      </p>

      <div class:hidden={!visibleSectionIds.has("quality")}>
        <SettingsQualitySection
          {loading}
          bind:selected={form.selected}
          bind:customFormat={form.customFormat}
        />
      </div>

      <div class:hidden={!visibleSectionIds.has("wheel")}>
        <SettingsWheelSection {loading} bind:wheelDelta={form.wheelDelta} />
      </div>

      <div class:hidden={!visibleSectionIds.has("player")}>
        <SettingsPlayerSection
          {loading}
          bind:initialSpeed={form.initialSpeed}
          bind:initialVolume={form.initialVolume}
          bind:navigateOnPlay={form.navigateOnPlay}
        />
      </div>

      <div class:hidden={!visibleSectionIds.has("pip")}>
        <SettingsPipSection
          {loading}
          bind:pipDefault={form.pipDefault}
          bind:pipFitAspect={form.pipFitAspect}
          bind:pipGeometry={form.pipGeometry}
          bind:pipQuality={form.pipQuality}
        />
      </div>

      <div class:hidden={!visibleSectionIds.has("window")}>
        <SettingsWindowSection
          {loading}
          {monitors}
          bind:windowScreen={form.windowScreen}
          bind:closeOnEof={form.closeOnEof}
        />
      </div>

      <div class:hidden={!visibleSectionIds.has("hdr")}>
        <SettingsHdrSection
          {loading}
          bind:toneMapping={form.toneMapping}
          bind:computePeak={form.computePeak}
          bind:mpvExtraArgs={form.mpvExtraArgs}
          bind:networkTimeout={form.mpvNetworkTimeout}
          bind:hwdec={form.mpvHwdec}
        />
      </div>

      <div class:hidden={!visibleSectionIds.has("sponsor")}>
        <SettingsSponsorSection
          {loading}
          bind:sponsorActions={form.sponsorActions}
          bind:sponsorShowSegments={form.sponsorShowSegments}
        />
      </div>

      <div class:hidden={!visibleSectionIds.has("feed")}>
        <SettingsFeedSection
          {loading}
          bind:feedShowShorts={form.feedShowShorts}
          bind:feedNotifyNew={form.feedNotifyNew}
          bind:feedAutoReadPlayed={form.feedAutoReadPlayed}
          bind:feedAutoReadDays={form.feedAutoReadDays}
        />
      </div>

      <div class:hidden={!visibleSectionIds.has("categories")}>
        <SettingsCategoriesSection {notify} />
      </div>

      <div class:hidden={!visibleSectionIds.has("livewait")}>
        <SettingsLiveWaitSection
          {loading}
          bind:liveWaitAction={form.liveWaitAction}
        />
      </div>

      <div class:hidden={!visibleSectionIds.has("shortcuts")}>
        <SettingsShortcutsSection
          {loading}
          bind:shortcutsEnabled={form.shortcutsOn}
        />
      </div>

      <div class:hidden={!visibleSectionIds.has("chatpopup")}>
        <SettingsChatPopupSection
          {loading}
          bind:showAuthor={form.chatShowAuthor}
          bind:fontSize={form.chatFontSize}
          bind:opacity={form.chatOpacity}
          bind:closeOnEnd={form.chatCloseOnEnd}
        />
      </div>

      <div class:hidden={!visibleSectionIds.has("ui")}>
        <SettingsUiSection
          {loading}
          bind:uiTheme={form.uiTheme}
          bind:uiDensity={form.uiDensity}
          bind:uiLanguage={form.uiLanguage}
          bind:confirmUnsaved={form.confirmUnsaved}
        />
      </div>

      <div class:hidden={!visibleSectionIds.has("blocked")}>
        <SettingsBlockedSection {notify} />
      </div>

      <div class:hidden={!visibleSectionIds.has("filters")}>
        <SettingsFiltersSection {notify} />
      </div>

      <div class:hidden={!visibleSectionIds.has("ngusers")}>
        <SettingsNgUsersSection {notify} />
      </div>

      <div class:hidden={!visibleSectionIds.has("ops")}>
        <SettingsOpsSection {notify} onImported={handleImported} />
      </div>

      <div class:hidden={!visibleSectionIds.has("notices")}>
        <SettingsNoticesSection />
      </div>

      <div class:hidden={!visibleSectionIds.has("deps")}>
        <SettingsDepsSection
          {loading}
          bind:ytdlpPath={form.ytdlpPath}
          bind:poToken={form.poToken}
        />
      </div>

      <div class:hidden={!visibleSectionIds.has("diag")}>
        <SettingsDiagSection {notify} />
      </div>

      <div class:hidden={!visibleSectionIds.has("update")}>
        <SettingsUpdateSection />
      </div>
    </div>

    <!-- 右側常時表示ペイン（FR-90、仕様決定 DS）。スクロールに追従し、
         保存ボタン・元に戻すボタンとボタン下部の変更内容一覧を置く -->
    <aside class="settings-sidebar panel panel--filled">
      <div class="sidebar-actions">
        <button
          class="save-btn primary"
          onclick={() => void save(false)}
          disabled={loading || saving || (!hasChanges && pendingApply === null)}
        >
          {saving ? t("settings.saving") : t("settings.save")}
        </button>
        <button
          class="reset-btn"
          onclick={resetChanges}
          disabled={loading || saving || !hasChanges}
        >
          {t("settings.reset")}
        </button>
      </div>

      <div class="sidebar-notices" class:hidden={notices.length === 0}>
        {#each notices as n}
          <p class="notice">{n}</p>
        {/each}
      </div>

      <div class="sidebar-changes">
        <h2>
          {changes.length > 0
            ? t("settings.changes.count", { count: changes.length })
            : t("settings.changes.title")}
        </h2>
        <!-- 空表示と一覧の出し分けはリマウントを避けるため hidden 切替（FR-80） -->
        <p class="subtle changes-empty" class:hidden={hasChanges}>
          {t("settings.changes.empty")}
        </p>
        <ul class="change-list" class:hidden={!hasChanges}>
          {#each changes as ch (ch.id)}
            <li class="change-item">
              <div class="change-meta">{ch.sectionLabel}</div>
              <div class="change-field">{ch.fieldLabel}</div>
              <div class="change-diff">
                <span class="change-before">{ch.beforeText}</span>
                <span class="change-arrow" aria-hidden="true">→</span>
                <span class="change-after">{ch.afterText}</span>
              </div>
              <div class="change-note" class:hidden={!ch.note}>
                {ch.note ?? ""}
              </div>
            </li>
          {/each}
        </ul>
      </div>
    </aside>
  </div>
</main>

<!-- 未保存タブ移動時の 3 択確認ダイアログ（FR-91、仕様決定 DT）。
     リマウントを避けるため常時マウントし hidden で切り替える（FR-80・仕様決定 DE） -->
<div
  class="unsaved-overlay"
  class:hidden={pendingNavigation === null}
>
  <div
    class="unsaved-dialog"
    role="dialog"
    aria-modal="true"
    aria-labelledby="unsaved-dialog-title"
  >
    <h2 id="unsaved-dialog-title">{t("settings.unsavedDialog.title")}</h2>
    <p>{t("settings.unsavedDialog.body")}</p>
    <div class="unsaved-actions">
      <button
        bind:this={cancelDialogBtn}
        onclick={cancelLeave}
        disabled={dialogSaving}
      >
        {t("settings.unsavedDialog.cancel")}
      </button>
      <button
        class="danger"
        onclick={discardAndLeave}
        disabled={dialogSaving}
      >
        {t("settings.unsavedDialog.discard")}
      </button>
      <button
        class="primary"
        onclick={() => void saveAndLeave()}
        disabled={dialogSaving}
      >
        {dialogSaving ? t("settings.saving") : t("settings.unsavedDialog.save")}
      </button>
    </div>
  </div>
</div>

<style>
  .settings {
    max-width: 1120px;
  }

  h1 {
    font-size: 2rem;
    margin-bottom: 1.5rem;
  }

  .settings-layout {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 320px;
    gap: 24px;
    align-items: start;
  }

  .settings-main {
    min-width: 0;
  }

  .section-filter {
    width: 100%;
    box-sizing: border-box;
    padding: 8px 12px;
    border-radius: 8px;
    border: 1px solid var(--border);
    background: var(--bg-panel);
    color: var(--fg);
    margin-bottom: 16px;
  }

  .settings-sidebar {
    position: sticky;
    top: calc(var(--app-nav-height, 48px) + 16px);
    max-height: calc(100vh - var(--app-nav-height, 48px) - 32px);
    display: flex;
    flex-direction: column;
    gap: 12px;
    box-sizing: border-box;
  }

  .sidebar-actions {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
    flex-shrink: 0;
  }

  .save-btn.primary {
    font-weight: 600;
    border-color: var(--accent);
    color: var(--accent);
  }

  .sidebar-notices {
    display: flex;
    flex-direction: column;
    gap: 6px;
    flex-shrink: 0;
  }

  .sidebar-notices .notice {
    margin-top: 0;
    font-size: 0.85rem;
  }

  .sidebar-changes {
    display: flex;
    flex-direction: column;
    min-height: 0;
    border-top: 1px solid var(--border-row);
    padding-top: 10px;
  }

  .sidebar-changes h2 {
    font-size: 0.95rem;
    margin: 0 0 8px;
    flex-shrink: 0;
  }

  .changes-empty {
    margin: 0;
    font-size: 0.85rem;
  }

  .change-list {
    list-style: none;
    margin: 0;
    padding: 0;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .change-item {
    padding: 8px 10px;
    border: 1px solid var(--border-row);
    border-radius: 8px;
    background: var(--bg-inset);
    font-size: 0.85rem;
  }

  .change-meta {
    color: var(--fg-muted);
    font-size: 0.75rem;
  }

  .change-field {
    font-weight: 600;
    color: var(--fg-strong);
    margin: 2px 0 4px;
    overflow-wrap: anywhere;
  }

  .change-diff {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 4px 6px;
    overflow-wrap: anywhere;
  }

  .change-before {
    color: var(--fg-muted);
  }

  .change-arrow {
    color: var(--fg-muted);
  }

  .change-after {
    color: var(--accent);
    font-weight: 600;
  }

  .change-note {
    margin-top: 4px;
    color: var(--warn);
    font-size: 0.78rem;
  }

  .unsaved-overlay {
    position: fixed;
    inset: 0;
    background: var(--overlay);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 65;
  }

  .unsaved-dialog {
    background: var(--bg-panel);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 20px;
    max-width: 440px;
    width: calc(100% - 48px);
  }

  .unsaved-dialog h2 {
    margin-top: 0;
  }

  .unsaved-actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    flex-wrap: wrap;
  }

  .unsaved-actions .primary {
    font-weight: 600;
    border-color: var(--accent);
    color: var(--accent);
  }

  @media (max-width: 820px) {
    .settings-layout {
      grid-template-columns: 1fr;
    }

    .settings-sidebar {
      position: static;
      max-height: none;
    }
  }
</style>
