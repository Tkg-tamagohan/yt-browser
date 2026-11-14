//! mpv 窓の前面化（FR-42、仕様決定 BR）。
//! Windows は mpv プロセスの PID から可視トップレベル窓を特定し、
//! `window-id`（mpv 0.37 以降で HWND）が取れたときはそれを優先する。
//! X11 は `window-id`（mpv 0.36 以降で XID）を必須とし、EWMH の
//! `_NET_ACTIVE_WINDOW` を窓マネージャへ送って活性化する。
//! Wayland はプロトコル上クライアント・第三者とも前面化の手段が無い
//! 恒久制限のため、呼び出し側で事前に拒否する（ここでも残りの環境は
//! `UnsupportedWindowing` で弾く防御を残す）。

use super::MpvError;

/// mpv の `window-id` プロパティ値（int64）をネイティブ窓識別子へ変換する。
/// XID / HWND とも 0 以下は有効な値になり得ないため正の値のみ受理する。
/// プロパティが数値でない・未公開（窓未作成・古い mpv）のとき呼び出し側は
/// None として扱う。
pub(crate) fn native_window_id(value: i64) -> Option<u64> {
    u64::try_from(value).ok().filter(|v| *v > 0)
}

/// 前面化の実行。`window_id` は `get_property window-id` の結果（x11: XID、
/// win32: HWND）。`pid` は mpv プロセス ID（win32 での列挙フォールバック用）。
/// `windowing` は `windowing_system()` の分類値。対象窓が見つからないときは
/// `WindowNotReady`、環境自体が非対応なら `UnsupportedWindowing` を返す。
pub(crate) fn focus_mpv_window(
    windowing: &str,
    window_id: Option<i64>,
    pid: Option<u32>,
    instance_id: u32,
) -> Result<(), MpvError> {
    match windowing {
        "windows" => focus_win32(window_id, pid, instance_id),
        "x11" => match window_id.and_then(native_window_id) {
            Some(xid) => focus_x11(xid as u32),
            // x11 では window-id が唯一の窓特定経路。起動直後・アイドル中・
            // mpv 0.36 未満では取得できないため前面化は不可
            None => Err(MpvError::WindowNotReady(instance_id)),
        },
        other => Err(MpvError::UnsupportedWindowing(other.to_string())),
    }
}

#[cfg(windows)]
fn focus_win32(window_id: Option<i64>, pid: Option<u32>, instance_id: u32) -> Result<(), MpvError> {
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        BringWindowToTop, EnumWindows, GetForegroundWindow, GetWindowThreadProcessId, IsIconic,
        IsWindow, IsWindowVisible, SetForegroundWindow, ShowWindow, SW_RESTORE,
    };

    /// `window-id` が取れたときは HWND として優先。無ければ mpv プロセスの
    /// 可視トップレベル窓を列挙して特定する（FR-42、仕様決定 BR）。
    unsafe fn hwnd_of(window_id: Option<i64>, pid: Option<u32>) -> Option<HWND> {
        if let Some(id) = window_id.and_then(native_window_id) {
            let hwnd = id as HWND;
            if unsafe { IsWindow(hwnd) } != 0 {
                return Some(hwnd);
            }
        }
        let pid = pid?;
        /// EnumWindows のコンテキスト。対象 PID と一致した可視窓を集める。
        struct Ctx {
            pid: u32,
            out: Vec<HWND>,
        }
        unsafe extern "system" fn collect(hwnd: HWND, lparam: isize) -> i32 {
            let ctx = unsafe { &mut *(lparam as *mut Ctx) };
            let mut wpid = 0u32;
            let tid = unsafe { GetWindowThreadProcessId(hwnd, &mut wpid) };
            if tid != 0 && wpid == ctx.pid && unsafe { IsWindowVisible(hwnd) } != 0 {
                ctx.out.push(hwnd);
            }
            1
        }
        let mut ctx = Ctx {
            pid,
            out: Vec::new(),
        };
        let _ = unsafe { EnumWindows(Some(collect), &mut ctx as *mut Ctx as isize) };
        ctx.out.into_iter().next()
    }

    /// 前面化。まず `SetForegroundWindow` を直接試し、フォアグラウンド権限
    /// で拒否された場合は現在のフォアグラウンド窓スレッドへ入力を
    /// アタッチしてから再試行する（AttachThreadInput 共有で権限を引き継ぐ
    /// 定石）。最小化中は `SW_RESTORE` で先に戻す。
    unsafe fn bring_to_foreground(hwnd: HWND) -> bool {
        unsafe {
            if IsIconic(hwnd) != 0 {
                ShowWindow(hwnd, SW_RESTORE);
            }
            if SetForegroundWindow(hwnd) != 0 {
                return true;
            }
            let fg = GetForegroundWindow();
            let fg_tid = if fg.is_null() {
                0
            } else {
                GetWindowThreadProcessId(fg, std::ptr::null_mut())
            };
            let my_tid = GetCurrentThreadId();
            if fg_tid == 0 || fg_tid == my_tid {
                return false;
            }
            AttachThreadInput(my_tid, fg_tid, 1);
            let _ = BringWindowToTop(hwnd);
            let ok = SetForegroundWindow(hwnd) != 0;
            AttachThreadInput(my_tid, fg_tid, 0);
            ok
        }
    }

    match unsafe { hwnd_of(window_id, pid) } {
        Some(hwnd) => {
            if unsafe { bring_to_foreground(hwnd) } {
                Ok(())
            } else {
                Err(MpvError::ForegroundFailed(
                    "SetForegroundWindow が拒否されました".to_string(),
                ))
            }
        }
        None => Err(MpvError::WindowNotReady(instance_id)),
    }
}

/// win32 以外のビルドでフォールバック経路まで残さないためのスタブ。
/// `windowing_system()` が "windows" を返すのは Windows ビルドのみ。
#[cfg(not(windows))]
fn focus_win32(_: Option<i64>, _: Option<u32>, instance_id: u32) -> Result<(), MpvError> {
    Err(MpvError::WindowNotReady(instance_id))
}

#[cfg(target_os = "linux")]
fn focus_x11(xid: u32) -> Result<(), MpvError> {
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{
        ClientMessageData, ClientMessageEvent, ConfigureWindowAux, ConnectionExt, EventMask,
        InputFocus, StackMode, CLIENT_MESSAGE_EVENT,
    };

    let (conn, screen) =
        x11rb::connect(None).map_err(|e| MpvError::ForegroundFailed(e.to_string()))?;
    let root = conn.setup().roots[screen].root;
    let active = conn
        .intern_atom(false, b"_NET_ACTIVE_WINDOW")
        .map_err(|e| MpvError::ForegroundFailed(e.to_string()))?
        .reply()
        .map_err(|e| MpvError::ForegroundFailed(e.to_string()))?
        .atom;

    // EWMH _NET_ACTIVE_WINDOW（source=2: ユーザーの直接操作として扱う、
    // timestamp は CurrentTime=0）。主要な経路で、最小化の解除も WM 側が
    // 面倒を見る。エラーは check() で拾い BadWindow 等を表面化する
    let ev = ClientMessageEvent {
        response_type: CLIENT_MESSAGE_EVENT,
        format: 32,
        sequence: 0,
        window: xid,
        type_: active,
        data: ClientMessageData::from([2u32, 0, 0, 0, 0]),
    };
    conn.send_event(
        false,
        root,
        EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
        ev,
    )
    .map_err(|e| MpvError::ForegroundFailed(e.to_string()))?
    .check()
    .map_err(|e| MpvError::ForegroundFailed(e.to_string()))?;

    // EWMH を見ない WM・確実にフォーカスを渡したいケースの補助として
    // XRaiseWindow + XSetInputFocus も投げる（仕様決定 BR の副経路）。
    // 失敗しても主経路は送付済みのため結果は見ない
    let _ = conn.configure_window(xid, &ConfigureWindowAux::new().stack_mode(StackMode::ABOVE));
    let _ = conn.set_input_focus(InputFocus::PARENT, xid, 0u32);
    conn.flush()
        .map_err(|e| MpvError::ForegroundFailed(e.to_string()))?;
    Ok(())
}

/// X11 でないビルド向けスタブ。"x11" は Linux ビルドでのみ返る。
#[cfg(not(target_os = "linux"))]
fn focus_x11(_: u32) -> Result<(), MpvError> {
    Err(MpvError::UnsupportedWindowing("x11".to_string()))
}

#[cfg(test)]
mod tests {
    use super::native_window_id;

    /// DB-FG-01: `window-id` 値のネイティブ識別子化（FR-42、仕様決定 BR）。
    /// XID・HWND とも 0 以下は無効で、正の int64 のみ受理する。
    /// プロパティ未公開時は呼び出し側が None を渡す（テスト対象外）。
    #[test]
    fn native_window_id_values() {
        // XID・HWND は正の値のみ有効
        assert_eq!(native_window_id(0x2e00007), Some(0x2e00007));
        assert_eq!(native_window_id(1), Some(1));
        assert_eq!(native_window_id(i64::MAX), Some(i64::MAX as u64));
        for bad in [0, -1, i64::MIN] {
            assert_eq!(native_window_id(bad), None, "{bad} は None になるべき");
        }
    }
}
