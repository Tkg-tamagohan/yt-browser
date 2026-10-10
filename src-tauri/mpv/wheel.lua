-- wheel.lua: 一時停止中はコマ送り、再生中は音量（設計書 §4.2 / 仕様決定 D）
-- 音量の変化量は script-opts の wheel-volume_delta（設定キー wheel.volume_delta）で上書きできる
local options = { volume_delta = 2 }
require("mp.options").read_options(options, "wheel")

-- コマ送りの無音化（FR-22、仕様決定 AO）。mpv はステップ実行時に短い音声
-- 断片を出しうるため、mute=yes の窓で包み、タイマーで元の状態へ復帰する。
-- 連続ステップでは最初に捕捉したミュート値で復帰し、窓の途中でユーザーが
-- ミュートを切り替えた場合は開始時の値で上書きされる（既知の制約）。
local STEP_UNMUTE_DELAY = 0.15
local saved_mute = nil
local unmute_timer = nil

local function silent_step(cmd)
  if saved_mute == nil then
    saved_mute = mp.get_property_bool("mute") or false
  end
  mp.set_property_bool("mute", true)
  mp.command(cmd)
  if unmute_timer ~= nil then
    unmute_timer:kill()
  end
  unmute_timer = mp.add_timeout(STEP_UNMUTE_DELAY, function()
    unmute_timer = nil
    if saved_mute ~= nil then
      mp.set_property_bool("mute", saved_mute)
      saved_mute = nil
    end
  end)
end

local function wheel(ev, paused_cmd, playing_delta)
  -- マウスホイールのノッチは複合バインドで press として届く（down が来るバックエンドも一応許容）
  if ev.event ~= "press" and ev.event ~= "down" then return end
  if mp.get_property_bool("pause") then
    silent_step(paused_cmd)
  else
    mp.commandv("add", "volume", playing_delta)
  end
end

mp.add_key_binding("WHEEL_UP",   "yb_wheel_up",   function(e) wheel(e, "frame-step",       options.volume_delta) end, {complex=true})
mp.add_key_binding("WHEEL_DOWN", "yb_wheel_down", function(e) wheel(e, "frame-back-step", -options.volume_delta) end, {complex=true})

-- mpv 標準の , / . キーバインドを上書きし、mpv 窓上のキー操作も無音化の対象に含める
mp.add_key_binding(",", "yb_frame_back_step_key", function() silent_step("frame-back-step") end)
mp.add_key_binding(".", "yb_frame_step_key",      function() silent_step("frame-step") end)

-- アプリ側 UI（player_control）からの委譲経路。同じラッパで実行して二重実装を避ける
mp.register_script_message("yb_frame_step",      function() silent_step("frame-step") end)
mp.register_script_message("yb_frame_back_step", function() silent_step("frame-back-step") end)
