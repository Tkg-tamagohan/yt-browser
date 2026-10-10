<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { t, TONE_MAPPING_MPV } from "$lib/i18n";
  import {
    PIP_GEOMETRY_DEFAULT,
    isValidPipGeometry,
    pipDefaultEnabled,
    pipFitAspectEnabled,
  } from "$lib/pip";
  import { QUALITY_PRESETS } from "$lib/quality";
  import { feedShowShortsEnabled } from "$lib/feed-settings";
  import {
    asErrorMessage,
    formatOverrides,
    initPlayerEvents,
    playerStates,
    type UiError,
  } from "$lib/players.svelte";
  import {
    CUSTOM,
    SPONSOR_CATEGORIES,
    TONE_MAPPINGS,
    type SponsorCategory,
  } from "$lib/settings-consts";
  import SettingsBlockedSection from "$lib/SettingsBlockedSection.svelte";
  import SettingsChatSearchSection from "$lib/SettingsChatSearchSection.svelte";
  import SettingsFiltersSection from "$lib/SettingsFiltersSection.svelte";
  import SettingsUpdateSection from "$lib/SettingsUpdateSection.svelte";

  // 設計書 §4.3 のプリセット表（プレイヤーカードの画質選択と共有）
  const PRESETS = QUALITY_PRESETS;

  let selected = $state<string>(PRESETS[0].format);
  let customFormat = $state("");
  // カテゴリごとの動作（"skip" | "notify" | "off"）。既定は sponsor のみ skip
  let sponsorActions = $state<Record<SponsorCategory, string>>({
    ...(Object.fromEntries(SPONSOR_CATEGORIES.map((c) => [c, "off"])) as Record<
      SponsorCategory,
      string
    >),
    sponsor: "skip",
  });
  let loading = $state(true);
  let saving = $state(false);
  let notices = $state<string[]>([]);
  // DB に最後に書き込んだ画質式（読込時点では読み取った値）。
  // 書込み済みと適用完了を分けて追うため、即時適用の成否とは独立に進める
  let persistedFormat = $state("");
  // ホイール 1 ノッチの音量変化量（mpv script-opts の wheel-volume_delta。
  // 次回再生から有効。既定 2、undefined は「Lua 既定のまま」＝保存スキップ）
  let wheelDelta = $state<number | undefined>(2);
  // 再生の既定表示を PiP にするか（pip.default、仕様決定 AJ）。既定は on
  let pipDefault = $state(true);
  // PiP 小窓の --geometry 値。mpv 形式（WxH + 任意の +-x+-y）だけ保存する。
  // 空欄での保存は「既定値へ戻す」操作として扱い、UI 表示も既定に戻す
  let pipGeometry = $state(PIP_GEOMETRY_DEFAULT);
  // PiP 小窓のサイズを動画のアスペクト比へ追従させるか（pip.fit_aspect、
  // 仕様決定 AL）。既定は on。稼働中の PiP 窓にも即時反映される
  let pipFitAspect = $state(true);
  // PiP 既定画質式（pip.quality.format）。空欄は「全体の画質設定に従う」
  // （未設定として扱われる。仕様決定 W）
  let pipQuality = $state("");
  // フィード一覧に Shorts を表示するか（feed.show_shorts、仕様決定 AP）。既定は off
  let feedShowShorts = $state(false);
  // 稼働中インスタンスへの即時適用が残っている画質式。全台に適用できたら null
  let pendingApply = $state<string | null>(null);

  let toneMapping = $state("auto");
  let computePeak = $state("auto");
  let mpvExtraArgs = $state("");

  const effectiveFormat = $derived(
    selected === CUSTOM ? customFormat.trim() : selected,
  );

  function notify(msg: string): void {
    notices = [...notices.slice(-4), msg];
    setTimeout(() => {
      notices = notices.filter((n) => n !== msg);
    }, 6000);
  }

  async function save(): Promise<void> {
    // 画質式が前回保存値から変わったときだけ適用する（カテゴリだけの保存で
    // 再生中インスタンスが再読込されないようにする。loadfile replace は再生を中断させる）
    const format = effectiveFormat;
    saving = true;
    try {
      // DB の値と違うときだけ書き込む。書いた値は必ず pendingApply にして
      // 適用成功まで追跡する（書き戻し時も適用が残る形になる）
      if (format && format !== persistedFormat) {
        await invoke("settings_set", { key: "quality.format", value: format });
        persistedFormat = format;
        pendingApply = format;
      }
      await invoke("settings_set", {
        key: "sponsor.categories",
        value: JSON.stringify(sponsorActions),
      });
      // 不正値があっても他キーの保存自体は行うが、全体の成功通知は出さない
      let invalid = false;
      // wheel.volume_delta: 有限数値・±100 以内だけ保存する
      // （mpv 側がそのまま script-opts に渡すため、ここで弾く）
      if (wheelDelta !== undefined) {
        if (!Number.isFinite(wheelDelta) || Math.abs(wheelDelta) > 100) {
          invalid = true;
          notify(
            t("settings.failed", {
              message: `wheel.volume_delta: ${wheelDelta}`,
            }),
          );
        } else {
          await invoke("settings_set", {
            key: "wheel.volume_delta",
            value: String(wheelDelta),
          });
        }
      }
      // pip.default: チェックボックスの 2 値を on/off で保存する（仕様決定 AJ）。
      // 保存待ちの間に再操作された場合は DB と画面がずれるため未保存と通知する
      const pd = pipDefault;
      await invoke("settings_set", {
        key: "pip.default",
        value: pd ? "on" : "off",
      });
      if (pipDefault !== pd) {
        invalid = true;
        notify(t("settings.pip.unsaved"));
      }
      // pip.fit_aspect: チェックボックスの 2 値を on/off で保存する（仕様決定 AL）。
      // 稼働中インスタンスへの即時反映はバックエンド側で行われる
      const fa = pipFitAspect;
      await invoke("settings_set", {
        key: "pip.fit_aspect",
        value: fa ? "on" : "off",
      });
      if (pipFitAspect !== fa) {
        invalid = true;
        notify(t("settings.pip.unsaved"));
      }
      // pip.geometry: mpv の geometry 形式だけ保存する（ここで弾く）。
      // 空欄は既定値へのリセットとして扱い、不正形式は失敗通知のみ
      const geo = pipGeometry.trim();
      if (!geo) {
        // 空欄は既定値へのリセット。保存に成功してから画面値を戻す
        // （失敗時に表示と DB の値がずれないようにする）。
        // 保存中にユーザーが再入力していた場合はその値は DB に残っていない
        // ため、画面の値を残したまま未保存であることを通知する
        await invoke("settings_set", {
          key: "pip.geometry",
          value: PIP_GEOMETRY_DEFAULT,
        });
        if (!pipGeometry.trim()) {
          pipGeometry = PIP_GEOMETRY_DEFAULT;
        } else {
          invalid = true;
          notify(t("settings.pip.unsaved"));
        }
      } else if (isValidPipGeometry(geo)) {
        await invoke("settings_set", { key: "pip.geometry", value: geo });
      } else {
        invalid = true;
        notify(t("settings.failed", { message: `pip.geometry: ${geo}` }));
      }
      // pip.quality.format: 空欄は未設定（全体画質に従う）。値は自由記述の
      // フォーマット式なのでバリデーションせずそのまま保存する。
      // 保存待ちの間に再入力された場合は DB と画面がずれるため未保存と通知する
      const pq = pipQuality.trim();
      await invoke("settings_set", {
        key: "pip.quality.format",
        value: pq,
      });
      if (pipQuality.trim() !== pq) {
        invalid = true;
        notify(t("settings.pip.unsaved"));
      }
      // feed.show_shorts: チェックボックスの 2 値を on/off で保存する（仕様決定 AP）。
      // 次回のフィード一覧取得から反映される（list_feed の都度読み取り）
      const fs = feedShowShorts;
      await invoke("settings_set", {
        key: "feed.show_shorts",
        value: fs ? "on" : "off",
      });
      if (feedShowShorts !== fs) {
        invalid = true;
        notify(t("settings.feed.unsaved"));
      }
      // hdr.tone_mapping / hdr.compute_peak: DDL 相当の値は select なので
      // そのまま保存（auto は未指定として扱う）。選択肢 ID はドット無しなので
      // TONE_MAPPING_MPV で mpv の値へ変換する。
      // 保存待ちの間に select が変更されると DB と画面がずれるため、
      // 先にスナップショットを取り、完了後のずれは未保存として通知する
      const tmSnapshot = toneMapping;
      const cpSnapshot = computePeak;
      await invoke("settings_set", {
        key: "hdr.tone_mapping",
        value: TONE_MAPPING_MPV[tmSnapshot] ?? tmSnapshot,
      });
      await invoke("settings_set", {
        key: "hdr.compute_peak",
        value: cpSnapshot,
      });
      if (toneMapping !== tmSnapshot || computePeak !== cpSnapshot) {
        invalid = true;
        notify(t("settings.hdr.unsaved"));
      }
      // mpv.extra_args: 無検証の自由記述（仕様決定 Y の汎用受け皿）。
      // 無効値は mpv 起動失敗として Spawn エラー通知に乗る
      const extra = mpvExtraArgs.trim();
      await invoke("settings_set", { key: "mpv.extra_args", value: extra });
      if (mpvExtraArgs.trim() !== extra) {
        invalid = true;
        notify(t("settings.hdr.unsaved"));
      }
      // 画質の即時適用（pendingApply）は不正値があっても最後まで実行する。
      // 書き込み済みの画質式が適用されないまま残るのを防ぐため、成功通知だけ抑える
      // （invalid には未保存変更の検出も含む）
      if (!format || pendingApply !== format) {
        if (!invalid) {
          notify(
            format
              ? t("settings.saved")
              : t("settings.savedQualitySkipped"),
          );
        }
        return;
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
            action: { type: "quality", format },
          });
          applied += 1;
        } catch (e) {
          // 終了済みインスタンスは失敗に数えない:
          // - mpv_no_instance … エントリが既に消えている
          // - 共有マップに無い … player://ended が届いて終了を反映済み
          // それ以外の IPC エラーは稼働中インスタンスへの適用失敗として記録する
          const code = (e as UiError)?.code;
          if (code !== "mpv_no_instance" && playerStates.list.has(id)) {
            failed.push(id);
          }
        }
      }
      if (failed.length > 0) {
        // 未適用の台が残るため pendingApply を残し、次回保存で適用を再試行できるようにする
        notify(t("settings.applyPartial", { count: failed.length }));
      } else {
        pendingApply = null;
        if (!invalid) {
          notify(
            applied > 0
              ? t("settings.applied", { count: applied })
              : t("settings.saved"),
          );
        }
      }
    } catch (e) {
      notify(t("settings.failed", { message: asErrorMessage(e) }));
    } finally {
      saving = false;
    }
  }

  onMount(async () => {
    await initPlayerEvents();
    try {
      const stored = await invoke<string | null>("settings_get", {
        key: "quality.format",
      });
      if (stored) {
        if (PRESETS.some((p) => p.format === stored)) {
          selected = stored;
        } else {
          selected = CUSTOM;
          customFormat = stored;
        }
      }
      const sponsorRaw = await invoke<string | null>("settings_get", {
        key: "sponsor.categories",
      });
      const wheelRaw = await invoke<string | null>("settings_get", {
        key: "wheel.volume_delta",
      });
      const pipDefaultRaw = await invoke<string | null>("settings_get", {
        key: "pip.default",
      });
      pipDefault = pipDefaultEnabled(pipDefaultRaw);
      const pipFitAspectRaw = await invoke<string | null>("settings_get", {
        key: "pip.fit_aspect",
      });
      pipFitAspect = pipFitAspectEnabled(pipFitAspectRaw);
      const pipGeoRaw = await invoke<string | null>("settings_get", {
        key: "pip.geometry",
      });
      if (pipGeoRaw !== null && isValidPipGeometry(pipGeoRaw.trim())) {
        pipGeometry = pipGeoRaw.trim();
      }
      const pipQualityRaw = await invoke<string | null>("settings_get", {
        key: "pip.quality.format",
      });
      if (pipQualityRaw !== null) {
        pipQuality = pipQualityRaw.trim();
      }
      const feedShortsRaw = await invoke<string | null>("settings_get", {
        key: "feed.show_shorts",
      });
      feedShowShorts = feedShowShortsEnabled(feedShortsRaw);
      const toneRaw = await invoke<string | null>("settings_get", {
        key: "hdr.tone_mapping",
      });
      if (toneRaw !== null) {
        const v = toneRaw.trim();
        // DB には mpv の値（bt.2390 等）が入るので、選択肢 ID へ逆引きする
        const id = Object.entries(TONE_MAPPING_MPV).find(
          ([, mpv]) => mpv === v,
        )?.[0];
        if (TONE_MAPPINGS.includes(v)) {
          toneMapping = v;
        } else if (id) {
          toneMapping = id;
        }
      }
      const peakRaw = await invoke<string | null>("settings_get", {
        key: "hdr.compute_peak",
      });
      if (peakRaw !== null && ["auto", "yes", "no"].includes(peakRaw.trim())) {
        computePeak = peakRaw.trim();
      }
      const extraRaw = await invoke<string | null>("settings_get", {
        key: "mpv.extra_args",
      });
      if (extraRaw !== null) {
        mpvExtraArgs = extraRaw;
      }
      if (wheelRaw !== null) {
        const n = Number(wheelRaw);
        if (Number.isFinite(n)) wheelDelta = n;
      }
      if (sponsorRaw) {
        try {
          const parsed = JSON.parse(sponsorRaw) as Record<string, string>;
          sponsorActions = {
            ...sponsorActions,
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
    } catch {
      // 読み取り失敗時はプリセット既定のままにする
    }
    persistedFormat =
      selected === CUSTOM ? customFormat.trim() : selected;
    loading = false;
  });
</script>

<main class="container">
  <h1>{t("settings.title")}</h1>

  <section class="panel">
    <h2>{t("settings.quality.title")}</h2>
    <p class="subtle desc">{t("settings.quality.desc")}</p>
    {#if loading}
      <p class="subtle">…</p>
    {:else}
      <div class="presets">
        {#each PRESETS as p}
          <label>
            <input type="radio" bind:group={selected} value={p.format} />
            {t(p.key)}
            <code>{p.format}</code>
          </label>
        {/each}
        <label>
          <input type="radio" bind:group={selected} value={CUSTOM} />
          {t("settings.quality.custom")}
        </label>
        {#if selected === CUSTOM}
          <input
            type="text"
            class="format-input"
            bind:value={customFormat}
            placeholder={t("settings.quality.format.label")}
          />
        {/if}
      </div>
    {/if}
  </section>

  <section class="panel">
    <h2>{t("settings.wheel.title")}</h2>
    <p class="subtle desc">{t("settings.wheel.desc")}</p>
    {#if loading}
      <p class="subtle">…</p>
    {:else}
      <label class="wheel-row">
        {t("settings.wheel.volumeDelta")}
        <input
          type="number"
          class="wheel-input"
          bind:value={wheelDelta}
          min="-100"
          max="100"
          step="1"
        />
      </label>
    {/if}
  </section>

  <section class="panel">
    <h2>{t("settings.pip.title")}</h2>
    <p class="subtle desc">{t("settings.pip.desc")}</p>
    {#if loading}
      <p class="subtle">…</p>
    {:else}
      <label class="wheel-row">
        <input type="checkbox" bind:checked={pipDefault} />
        {t("settings.pip.default")}
      </label>
      <p class="subtle desc">{t("settings.pip.default.desc")}</p>
      <label class="wheel-row">
        <input type="checkbox" bind:checked={pipFitAspect} />
        {t("settings.pip.fitAspect")}
      </label>
      <p class="subtle desc">{t("settings.pip.fitAspect.desc")}</p>
      <label class="wheel-row">
        {t("settings.pip.geometry")}
        <input
          type="text"
          class="format-input"
          bind:value={pipGeometry}
          placeholder={PIP_GEOMETRY_DEFAULT}
        />
      </label>
      <label class="wheel-row">
        {t("settings.pip.quality")}
        <input
          type="text"
          class="format-input"
          bind:value={pipQuality}
          placeholder="bv*[height<=480]+ba/b[height<=480]"
        />
      </label>
      <p class="subtle desc">{t("settings.pip.quality.desc")}</p>
    {/if}
  </section>

  <section class="panel">
    <h2>{t("settings.hdr.title")}</h2>
    <p class="subtle desc">{t("settings.hdr.desc")}</p>
    {#if loading}
      <p class="subtle">…</p>
    {:else}
      <label class="wheel-row">
        {t("settings.hdr.toneMapping")}
        <select bind:value={toneMapping}>
          {#each TONE_MAPPINGS as m}
            <option value={m}>
              {m === "auto" ? t("settings.hdr.auto") : (TONE_MAPPING_MPV[m] ?? m)}
            </option>
          {/each}
        </select>
      </label>
      <label class="wheel-row">
        {t("settings.hdr.computePeak")}
        <select bind:value={computePeak}>
          <option value="auto">{t("settings.hdr.auto")}</option>
          <option value="yes">{t("settings.hdr.yes")}</option>
          <option value="no">{t("settings.hdr.no")}</option>
        </select>
      </label>
      <label class="wheel-row">
        {t("settings.hdr.extraArgs")}
        <input
          type="text"
          class="format-input"
          bind:value={mpvExtraArgs}
          placeholder="--target-colorspace-hint=yes"
        />
      </label>
      <p class="subtle desc">{t("settings.hdr.extraArgs.desc")}</p>
    {/if}
  </section>

  <section class="panel">
    <h2>{t("settings.sponsor.title")}</h2>
    <p class="subtle desc">{t("settings.sponsor.desc")}</p>
    {#if loading}
      <p class="subtle">…</p>
    {:else}
      <div class="sponsor-table">
        {#each SPONSOR_CATEGORIES as cat}
          <label>
            <span class="cat-label">{t(`sponsor.cat.${cat}`)}</span>
            <select bind:value={sponsorActions[cat]}>
              <option value="skip">{t("settings.sponsor.action.skip")}</option>
              <option value="notify">
                {t("settings.sponsor.action.notify")}
              </option>
              <option value="off">{t("settings.sponsor.action.off")}</option>
            </select>
          </label>
        {/each}
      </div>
    {/if}
  </section>

  <section class="panel">
    <h2>{t("settings.feed.title")}</h2>
    <p class="subtle desc">{t("settings.feed.desc")}</p>
    {#if loading}
      <p class="subtle">…</p>
    {:else}
      <label class="wheel-row">
        <input type="checkbox" bind:checked={feedShowShorts} />
        {t("settings.feed.showShorts")}
      </label>
      <p class="subtle desc">{t("settings.feed.showShorts.desc")}</p>
    {/if}
  </section>

  <SettingsBlockedSection {notify} />

  <SettingsFiltersSection {notify} />

  <SettingsChatSearchSection {notify} />

  <SettingsUpdateSection />

  <button
    class="save-btn"
    onclick={save}
    disabled={loading || saving}
  >
    {saving ? t("settings.saving") : t("settings.save")}
  </button>

  {#each notices as n}
    <p class="notice">{n}</p>
  {/each}
</main>

<style>
  h1 {
    font-size: 2rem;
    margin-bottom: 1.5rem;
  }

  .panel {
    padding: 16px;
    border: 1px solid #3c4043;
    border-radius: 12px;
    background: #202124;
  }

  h2 {
    font-size: 1.1rem;
    margin: 0 0 4px;
  }

  .desc {
    font-size: 0.9rem;
    margin-top: 0;
  }

  .presets {
    display: flex;
    flex-direction: column;
    gap: 10px;
    margin: 12px 0 16px;
  }

  .presets label {
    display: flex;
    gap: 8px;
    align-items: baseline;
    font-size: 0.95rem;
  }

  .presets code {
    color: #9aa0a6;
    font-size: 0.8rem;
    overflow-wrap: anywhere;
  }

  .format-input {
    width: 100%;
    margin-top: 4px;
    font-family: monospace;
  }

  .wheel-row {
    display: flex;
    align-items: center;
    gap: 10px;
    margin: 12px 0;
    font-size: 0.95rem;
  }

  .wheel-input {
    width: 80px;
  }

  .sponsor-table {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin: 12px 0;
  }

  .sponsor-table label {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 12px;
    font-size: 0.95rem;
  }

  .cat-label {
    flex: 1;
  }

  .save-btn {
    align-self: flex-start;
    margin-top: 16px;
  }
</style>
