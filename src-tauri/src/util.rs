//! モジュール横断で使う同期系の共有ユーティリティ。

use std::sync::{Mutex, MutexGuard};

/// Windows でコンソール系の子プロセス（mpv.exe / yt-dlp.exe）を spawn すると
/// 黒いコンソール窓が一瞬表示される。CREATE_NO_WINDOW で抑制する。
#[cfg(windows)]
pub(crate) const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Mutex のポイズンを握りつぶしてロックする。ロック保持中にパニックしない
/// 実装なので安全側に倒す。
pub(crate) fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// 未設定時は false とし、明示的なオン値（`on` / `true` / `1` / `yes`、
/// 前後空白・大文字小文字を許容）のときだけ true を返す。
pub(crate) fn setting_on_enabled(value: Option<String>) -> bool {
    match value {
        Some(v) => matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "on" | "true" | "1" | "yes"
        ),
        None => false,
    }
}

/// 未設定時は true（既定オン）とし、明示的なオフ値（`off` / `false` / `0` / `no`、
/// 前後空白・大文字小文字を許容）のときだけ false を返す。
pub(crate) fn setting_off_disabled(value: Option<String>) -> bool {
    match value {
        Some(v) => !matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "off" | "false" | "0" | "no"
        ),
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setting_on_enabled_accepts_explicit_on_values() {
        for v in ["on", "ON", " true ", "1", "Yes"] {
            assert!(
                setting_on_enabled(Some(v.to_string())),
                "expected true for {v:?}"
            );
        }
        for v in ["off", "false", "0", "no", "", "maybe"] {
            assert!(
                !setting_on_enabled(Some(v.to_string())),
                "expected false for {v:?}"
            );
        }
        assert!(!setting_on_enabled(None));
    }

    #[test]
    fn setting_off_disabled_defaults_true_unless_explicit_off() {
        assert!(setting_off_disabled(None));
        for v in ["on", "true", "1", "yes", "", "maybe"] {
            assert!(
                setting_off_disabled(Some(v.to_string())),
                "expected true for {v:?}"
            );
        }
        for v in ["off", "OFF", " false ", "0", "No"] {
            assert!(
                !setting_off_disabled(Some(v.to_string())),
                "expected false for {v:?}"
            );
        }
    }
}
