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

/// epoch ミリ秒を `"YYYY-MM-DDTHH:MM:SS+00:00"`（UTC の ISO 表記）へ戻す。
/// `iso8601_ms` の逆変換。civil-from-days（Howard Hinnant）で曜日を
/// 使わず日付を復元する。
pub(crate) fn iso_utc(ms: i64) -> String {
    let days = ms.div_euclid(86_400_000);
    let rem = ms.rem_euclid(86_400_000);
    let (h, mi, s) = (rem / 3_600_000, (rem / 60_000) % 60, (rem / 1_000) % 60);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mo <= 2 { y + 1 } else { y };
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}+00:00")
}

/// `"YYYY-MM-DDTHH:MM:SS(.sss)?(Z|±HH:MM)"` を epoch ミリ秒へ変換する最小パーサ。
/// YouTube のタイムスタンプ形式（`liveBroadcastDetails` / Data API v3）を扱う。
pub(crate) fn iso8601_ms(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    let num = |i: usize, n: usize| -> Option<i64> {
        let slice = b.get(i..i + n)?;
        if !slice.iter().all(|c| c.is_ascii_digit()) {
            return None;
        }
        let mut x = 0i64;
        for c in slice {
            x = x * 10 + (c - b'0') as i64;
        }
        Some(x)
    };
    if b.len() < 19
        || b[4] != b'-'
        || b[7] != b'-'
        || (b[10] != b'T' && b[10] != b' ')
        || b[13] != b':'
        || b[16] != b':'
    {
        return None;
    }
    let (y, mo, d) = (num(0, 4)?, num(5, 2)?, num(8, 2)?);
    let (h, mi, sec) = (num(11, 2)?, num(14, 2)?, num(17, 2)?);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || sec > 60 {
        return None;
    }
    // days-from-civil（Howard Hinnant の式）で 1970-01-01 からの日数を得る
    let y2 = if mo <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let mp = (mo + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let days = era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468;
    let mut ms = (days * 86_400 + h * 3_600 + mi * 60 + sec) * 1_000;
    let mut i = 19;
    if b.get(i) == Some(&b'.') {
        i += 1;
        let start = i;
        while b.get(i).is_some_and(|c| c.is_ascii_digit()) {
            i += 1;
        }
        let digits = i - start;
        if digits == 0 {
            return None;
        }
        // 小数秒はミリ秒（3 桁）へ丸める
        let f = num(start, digits.min(3))?;
        ms += f * 10i64.pow(3 - digits.min(3) as u32);
    }
    match b.get(i) {
        None => {}
        Some(&b'Z') => {
            if i + 1 != b.len() {
                return None;
            }
        }
        Some(sign @ (&b'+' | &b'-')) => {
            let has_colon = b.get(i + 3) == Some(&b':');
            let oh = num(i + 1, 2)?;
            let (om, end) = if has_colon {
                (num(i + 4, 2)?, i + 6)
            } else {
                (num(i + 3, 2)?, i + 5)
            };
            if end != b.len() || oh > 23 || om > 59 {
                return None;
            }
            let off = (oh * 3_600 + om * 60) * 1_000;
            ms += if *sign == b'+' { -off } else { off };
        }
        _ => return None,
    }
    Some(ms)
}

/// ISO 8601 期間文字列（`"P0D"`, `"PT0S"`, `"P#DT#H#M#S"`）を秒数へ変換する。
/// ライブ配信の `"P0D"` / `"PT0S"` は `Some(0)` を返す。
/// 月・年・週（`Y`/`M`/`W` の日付部）や負数・逆順・空形式（`"P"`, `"PT"`）は拒否する。
pub(crate) fn parse_iso8601_duration(s: &str) -> Option<i64> {
    let rest = s.strip_prefix('P')?;
    if rest.is_empty() {
        return None;
    }
    let (date_part, time_part) = match rest.split_once('T') {
        Some((d, t)) => {
            if t.is_empty() {
                return None;
            }
            (d, Some(t))
        }
        None => (rest, None),
    };
    let mut total: i64 = 0;
    let mut any = false;

    if !date_part.is_empty() {
        let days_str = date_part.strip_suffix('D')?;
        if days_str.is_empty() || !days_str.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let days: i64 = days_str.parse().ok()?;
        total = total.checked_add(days.checked_mul(86_400)?)?;
        any = true;
    }

    if let Some(t) = time_part {
        let mut cur = t;
        let mut stage = 0u8;
        while !cur.is_empty() {
            let num_end = cur.bytes().position(|b| !b.is_ascii_digit())?;
            if num_end == 0 {
                return None;
            }
            let val: i64 = cur[..num_end].parse().ok()?;
            let unit = cur.as_bytes()[num_end];
            let (next_stage, mult) = match unit {
                b'H' if stage < 1 => (1, 3_600),
                b'M' if stage < 2 => (2, 60),
                b'S' if stage < 3 => (3, 1),
                _ => return None,
            };
            stage = next_stage;
            total = total.checked_add(val.checked_mul(mult)?)?;
            any = true;
            cur = &cur[num_end + 1..];
        }
    }

    any.then_some(total)
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
