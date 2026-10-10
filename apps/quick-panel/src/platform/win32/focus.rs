//! Moving the keyboard focus between windows of different processes.
//!
//! Windows only lets the process that last received input call `SetForegroundWindow`. A global
//! shortcut or a modifier double tap gives this process no input event, so the call joins the input
//! queue of the current foreground thread with `AttachThreadInput` for its duration. This is the
//! same workaround as `activateWindow` in `apps/gui-go/previous_app_windows.go`.

use windows::Win32::Foundation::HWND;
use windows::Win32::System::Threading::{
    AttachThreadInput, GetCurrentProcessId, GetCurrentThreadId,
};
use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetGUIThreadInfo, GetWindowThreadProcessId, IsIconic, IsWindow,
    SetForegroundWindow, ShowWindow, GUITHREADINFO, SW_RESTORE,
};

pub fn foreground() -> HWND {
    unsafe { GetForegroundWindow() }
}

pub fn is_window(hwnd: HWND) -> bool {
    !hwnd.is_invalid() && unsafe { IsWindow(Some(hwnd)) }.as_bool()
}

/// The thread and process that own `hwnd`.
pub fn owner(hwnd: HWND) -> (u32, u32) {
    let mut process = 0_u32;
    let thread = unsafe { GetWindowThreadProcessId(hwnd, Some(&mut process)) };
    (thread, process)
}

pub fn is_ours(hwnd: HWND) -> bool {
    owner(hwnd).1 == unsafe { GetCurrentProcessId() }
}

/// The child window of `top`'s thread that holds the keyboard focus. Chromium, Electron and
/// WebView2 keep the focus in a nested render widget, so restoring only the top-level window
/// would send keystrokes to the frame instead of the input box.
pub fn focused_child(top: HWND) -> HWND {
    let (thread, _) = owner(top);
    if thread == 0 {
        return HWND::default();
    }
    let mut info = GUITHREADINFO {
        cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
        ..Default::default()
    };
    match unsafe { GetGUIThreadInfo(thread, &mut info) } {
        Ok(()) => info.hwndFocus,
        Err(_) => HWND::default(),
    }
}

/// Makes `top` the foreground window and gives `focus` (a child of it, or null) the keyboard
/// focus. Must run on the thread that owns this process's windows.
pub fn activate(top: HWND, focus: HWND) -> bool {
    if !is_window(top) {
        return false;
    }
    unsafe {
        if IsIconic(top).as_bool() {
            let _ = ShowWindow(top, SW_RESTORE);
        }
        let current = GetCurrentThreadId();
        let (foreground_thread, _) = owner(GetForegroundWindow());
        let attached = foreground_thread != 0
            && foreground_thread != current
            && AttachThreadInput(foreground_thread, current, true).as_bool();
        let raised = SetForegroundWindow(top).as_bool();
        let _ = SetFocus(Some(top));
        if attached {
            let _ = AttachThreadInput(foreground_thread, current, false);
        }
        if is_window(focus) && focus != top {
            let (thread, _) = owner(focus);
            let inner = thread != 0
                && thread != current
                && AttachThreadInput(thread, current, true).as_bool();
            let _ = SetFocus(Some(focus));
            if inner {
                let _ = AttachThreadInput(thread, current, false);
            }
        }
        raised
    }
}
