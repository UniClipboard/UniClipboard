//! Windows-specific quick panel helpers.
//!
//! On Windows, `SetForegroundWindow` has strict restrictions — only processes
//! that satisfy certain conditions (e.g. being the foreground process, or
//! having received the last input event) can successfully claim foreground.
//!
//! The standard workaround, used by apps like PowerToys Run, is to attach our
//! thread to the current foreground thread via `AttachThreadInput` before
//! calling `SetForegroundWindow`. This bypasses the restrictions.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use super::paste_sequence::{ctrl_v_sequence, SimulatedKeyEvent};
use tracing::{debug, warn};
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, SendInput, SetFocus, INPUT, INPUT_KEYBOARD, KEYEVENTF_KEYUP, VIRTUAL_KEY,
    VK_MENU,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetGUIThreadInfo, GetWindowThreadProcessId, IsIconic, IsWindow,
    SetForegroundWindow, ShowWindow, GUITHREADINFO, SW_RESTORE,
};

/// How long to wait after re-activating the previous app before sending Ctrl+V.
const RESTORE_SETTLE_MS: Duration = Duration::from_millis(40);
const ALT_RELEASE_WAIT_MS: Duration = Duration::from_millis(120);
const ALT_RELEASE_POLL_MS: Duration = Duration::from_millis(10);

/// 之前的前台窗口（顶层 HWND）以及它内部真正持有键盘焦点的子 HWND。
///
/// Electron / WebView2 / Chromium 类应用（如 Microsoft Teams、Slack、Discord、
/// VS Code、Chrome）的顶层窗口下嵌套着 `Chrome_RenderWidgetHostHWND` 之类的
/// 子窗口，真正接收 `WM_KEYDOWN` 的是内层子窗口。仅靠 `SetForegroundWindow`
/// 顶层窗口并不会自动把键盘焦点下沉到子窗口，于是合成的 Ctrl+V 落到了顶层
/// 窗口本身而不是输入框上 —— 这正是 quick window 自动粘贴对 Teams 失效的
/// 直接原因。
///
/// 这里同时保存顶层 HWND 与 `GetGUIThreadInfo` 拿到的 `hwndFocus`，恢复时
/// 先把顶层窗口拉回前台，再用 `AttachThreadInput` + `SetFocus` 把焦点重新
/// 下沉到内层子窗口。
///
/// 两个值都以 `isize` 存放，因为 `HWND` 内部是 `*mut c_void`，不实现
/// `Send + Sync`；用 0 表示"未捕获到内层焦点窗口，退化为只恢复顶层窗口"。
static PREVIOUS_FOREGROUND_WINDOW: Mutex<Option<(isize, isize)>> = Mutex::new(None);

/// Force the given Tauri window to the foreground on Windows.
///
/// Uses the `AttachThreadInput` trick to temporarily join our thread to the
/// foreground window's input queue, allowing `SetForegroundWindow` to succeed
/// even when Windows would normally block it.
pub fn force_foreground(window: &tauri::WebviewWindow) {
    let Some(hwnd) = window.hwnd().ok() else {
        warn!("Could not get HWND for quick panel");
        return;
    };

    // quick panel 自身没有需要单独下沉的内层焦点窗口，传 null 即可走顶层激活路径。
    let _ = activate_window(hwnd, HWND(std::ptr::null_mut()), "quick panel");
}

/// Remember the foreground window before the quick panel claims focus.
pub fn remember_previous_foreground(window: &tauri::WebviewWindow) {
    let Some(panel_hwnd) = window.hwnd().ok() else {
        warn!("Could not get HWND for quick panel while capturing previous foreground window");
        return;
    };

    unsafe {
        let foreground_hwnd = GetForegroundWindow();
        if foreground_hwnd.is_invalid() || foreground_hwnd == panel_hwnd {
            return;
        }

        // 顺手取该窗口所在 GUI 线程当前真正持有键盘焦点的子 HWND。
        // `GetGUIThreadInfo` 可以查询任意线程的 GUI 状态，不需要 AttachThreadInput；
        // 拿不到时（线程已退出 / 顶层窗口本身就是焦点）就退化为 null。
        let target_thread = GetWindowThreadProcessId(foreground_hwnd, None);
        let focus_hwnd = if target_thread != 0 {
            let mut info: GUITHREADINFO = std::mem::zeroed();
            info.cbSize = std::mem::size_of::<GUITHREADINFO>() as u32;
            if GetGUIThreadInfo(target_thread, &mut info).is_ok() {
                info.hwndFocus
            } else {
                HWND(std::ptr::null_mut())
            }
        } else {
            HWND(std::ptr::null_mut())
        };

        match PREVIOUS_FOREGROUND_WINDOW.lock() {
            Ok(mut guard) => {
                *guard = Some((foreground_hwnd.0 as isize, focus_hwnd.0 as isize));
                debug!(
                    ?foreground_hwnd,
                    ?focus_hwnd,
                    "Captured previous foreground window for quick panel"
                );
            }
            Err(_) => warn!("Previous foreground window lock poisoned"),
        }
    }
}

/// Restore focus to the previously active window.
pub fn restore_previous_foreground() -> Result<(), String> {
    let (top_hwnd, focus_hwnd) = match PREVIOUS_FOREGROUND_WINDOW.lock() {
        Ok(mut guard) => guard.take(),
        Err(_) => return Err("Previous foreground window lock poisoned".into()),
    }
    .map(|(top, focus)| {
        (
            HWND(top as *mut std::ffi::c_void),
            HWND(focus as *mut std::ffi::c_void),
        )
    })
    .ok_or_else(|| "No previous foreground window captured".to_string())?;

    activate_window(top_hwnd, focus_hwnd, "previous app")
}

/// Simulate a global Ctrl+V keystroke.
pub fn simulate_paste() -> Result<(), String> {
    std::thread::sleep(RESTORE_SETTLE_MS);
    let alt_still_down = !wait_for_alt_release();
    let events = ctrl_v_sequence(alt_still_down);
    let inputs = keyboard_inputs_for_events(&events);

    unsafe {
        let sent = SendInput(&inputs, std::mem::size_of::<INPUT>() as _);
        if sent != inputs.len() as u32 {
            return Err(format!(
                "SendInput sent {sent} events, expected {}",
                inputs.len()
            ));
        }
    }

    Ok(())
}

fn wait_for_alt_release() -> bool {
    if !is_key_down(VK_MENU) {
        return true;
    }

    let deadline = Instant::now() + ALT_RELEASE_WAIT_MS;
    while Instant::now() < deadline {
        std::thread::sleep(ALT_RELEASE_POLL_MS);
        if !is_key_down(VK_MENU) {
            return true;
        }
    }

    false
}

fn is_key_down(key: VIRTUAL_KEY) -> bool {
    unsafe { (GetAsyncKeyState(key.0 as i32) as u16 & 0x8000) != 0 }
}

fn keyboard_inputs_for_events(events: &[SimulatedKeyEvent]) -> Vec<INPUT> {
    events
        .iter()
        .map(|event| {
            let (key, is_key_up) = match *event {
                SimulatedKeyEvent::KeyDown(code) => (VIRTUAL_KEY(code), false),
                SimulatedKeyEvent::KeyUp(code) => (VIRTUAL_KEY(code), true),
            };

            let mut input: INPUT = unsafe { std::mem::zeroed() };
            input.r#type = INPUT_KEYBOARD;
            input.Anonymous.ki.wVk = key;
            if is_key_up {
                input.Anonymous.ki.dwFlags = KEYEVENTF_KEYUP;
            }
            input
        })
        .collect()
}

fn activate_window(top_hwnd: HWND, focus_hwnd: HWND, label: &str) -> Result<(), String> {
    unsafe {
        if !IsWindow(Some(top_hwnd)).as_bool() {
            return Err(format!("Stored {label} window handle is no longer valid"));
        }

        if IsIconic(top_hwnd).as_bool() {
            let _ = ShowWindow(top_hwnd, SW_RESTORE);
        }

        let foreground_hwnd = GetForegroundWindow();
        let foreground_thread = GetWindowThreadProcessId(foreground_hwnd, None);
        let current_thread = GetCurrentThreadId();
        let attached_fg = foreground_thread != 0 && foreground_thread != current_thread;

        if attached_fg {
            let _ = AttachThreadInput(foreground_thread, current_thread, true);
        }

        let result = SetForegroundWindow(top_hwnd);
        let _ = SetFocus(Some(top_hwnd));

        if attached_fg {
            let _ = AttachThreadInput(foreground_thread, current_thread, false);
        }

        if !result.as_bool() {
            return Err(format!(
                "SetForegroundWindow failed while restoring {label}"
            ));
        }

        // 顶层窗口已经回到前台。如果先前捕获到了真正持有键盘焦点的内层
        // 子窗口（Electron / WebView2 / Chromium 类应用的常见情况），再用
        // 一次 `AttachThreadInput` 把输入焦点下沉回那个子窗口，这样合成的
        // Ctrl+V 才会落到真正的输入框上 —— 这是修复 Teams 等应用自动粘贴
        // 失败的关键。`focus_hwnd` 为 null 或等于顶层窗口时跳过即可。
        if !focus_hwnd.is_invalid()
            && focus_hwnd != top_hwnd
            && IsWindow(Some(focus_hwnd)).as_bool()
        {
            let target_thread = GetWindowThreadProcessId(focus_hwnd, None);
            let attached_focus = target_thread != 0 && target_thread != current_thread;
            if attached_focus {
                let _ = AttachThreadInput(target_thread, current_thread, true);
            }
            let _ = SetFocus(Some(focus_hwnd));
            if attached_focus {
                let _ = AttachThreadInput(target_thread, current_thread, false);
            }
            debug!(?focus_hwnd, label, "Restored inner keyboard focus HWND");
        }

        debug!(?top_hwnd, label, "Activated window via AttachThreadInput");
    }

    Ok(())
}
