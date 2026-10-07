-- wheel.lua: 一時停止中はコマ送り、再生中は音量（設計書 §4.2 / 仕様決定 D）
-- 音量の変化量は script-opts の wheel-volume_delta（設定キー wheel.volume_delta）で上書きできる
local options = { volume_delta = 2 }
require("mp.options").read_options(options, "wheel")

local function wheel(ev, paused_cmd, playing_delta)
  -- マウスホイールのノッチは複合バインドで press として届く（down が来るバックエンドも一応許容）
  if ev.event ~= "press" and ev.event ~= "down" then return end
  if mp.get_property_bool("pause") then
    mp.command(paused_cmd)
  else
    mp.commandv("add", "volume", playing_delta)
  end
end

mp.add_key_binding("WHEEL_UP",   "yb_wheel_up",   function(e) wheel(e, "frame-step",       options.volume_delta) end, {complex=true})
mp.add_key_binding("WHEEL_DOWN", "yb_wheel_down", function(e) wheel(e, "frame-back-step", -options.volume_delta) end, {complex=true})
