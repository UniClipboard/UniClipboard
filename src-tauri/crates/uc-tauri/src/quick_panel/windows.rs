//! Windows-specific quick panel helpers.
//!
//! On Windows, `SetForegroundWindow` has strict restrictions — only processes
//! that satisfy certain conditions (e.g. being the foreground process, or
//! having received the last input event) can successfully claim foreground.
//!
//! The standard workaround, used by apps like PowerToys Run, is to attach our
//! thread to the current foreground thread via `AttachThreadInput` before
//! calling `SetForegroundWindow`. This bypasses the restrictions.

use tracing::{debug, warn};
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowThreadProcessId, SetForegroundWindow,
};

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

    unsafe {
        let foreground_hwnd = GetForegroundWindow();
        let foreground_thread = GetWindowThreadProcessId(foreground_hwnd, None);
        let current_thread = GetCurrentThreadId();

        if foreground_thread != current_thread {
            // Attach to the foreground thread so we're allowed to steal focus.
            let _ = AttachThreadInput(foreground_thread, current_thread, true);
            let result = SetForegroundWindow(hwnd);
            let _ = SetFocus(Some(hwnd));
            let _ = AttachThreadInput(foreground_thread, current_thread, false);

            if !result.as_bool() {
                warn!("SetForegroundWindow failed even after AttachThreadInput");
            } else {
                debug!("Quick panel forced to foreground via AttachThreadInput");
            }
        } else {
            // We're already the foreground thread, just set focus.
            let _ = SetForegroundWindow(hwnd);
            let _ = SetFocus(Some(hwnd));
            debug!("Quick panel set as foreground (same thread)");
        }
    }
}
