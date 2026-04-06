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

use tracing::{debug, warn};
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, SetFocus, INPUT, INPUT_KEYBOARD, KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_CONTROL,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowThreadProcessId, IsIconic, IsWindow, SetForegroundWindow,
    ShowWindow, SW_RESTORE,
};

/// Virtual key code for the `V` key used by Ctrl+V paste simulation.
const VK_V: VIRTUAL_KEY = VIRTUAL_KEY(0x56);

/// How long to wait after re-activating the previous app before sending Ctrl+V.
const RESTORE_SETTLE_MS: std::time::Duration = std::time::Duration::from_millis(40);

/// The last foreground window before the quick panel claimed focus.
static PREVIOUS_FOREGROUND_WINDOW: Mutex<Option<HWND>> = Mutex::new(None);

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

    activate_window(hwnd, "quick panel");
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

        match PREVIOUS_FOREGROUND_WINDOW.lock() {
            Ok(mut guard) => {
                *guard = Some(foreground_hwnd);
                debug!(
                    ?foreground_hwnd,
                    "Captured previous foreground window for quick panel"
                );
            }
            Err(_) => warn!("Previous foreground window lock poisoned"),
        }
    }
}

/// Restore focus to the previously active window.
pub fn restore_previous_foreground() -> Result<(), String> {
    let hwnd = match PREVIOUS_FOREGROUND_WINDOW.lock() {
        Ok(mut guard) => guard.take(),
        Err(_) => return Err("Previous foreground window lock poisoned".into()),
    }
    .ok_or_else(|| "No previous foreground window captured".to_string())?;

    activate_window(hwnd, "previous app")
}

/// Simulate a global Ctrl+V keystroke.
pub fn simulate_paste() -> Result<(), String> {
    std::thread::sleep(RESTORE_SETTLE_MS);

    unsafe {
        let mut inputs: [INPUT; 4] = std::mem::zeroed();
        inputs[0].r#type = INPUT_KEYBOARD;
        inputs[0].Anonymous.ki.wVk = VK_CONTROL;

        inputs[1].r#type = INPUT_KEYBOARD;
        inputs[1].Anonymous.ki.wVk = VK_V;

        inputs[2].r#type = INPUT_KEYBOARD;
        inputs[2].Anonymous.ki.wVk = VK_V;
        inputs[2].Anonymous.ki.dwFlags = KEYEVENTF_KEYUP;

        inputs[3].r#type = INPUT_KEYBOARD;
        inputs[3].Anonymous.ki.wVk = VK_CONTROL;
        inputs[3].Anonymous.ki.dwFlags = KEYEVENTF_KEYUP;

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

fn activate_window(hwnd: HWND, label: &str) -> Result<(), String> {
    unsafe {
        if !IsWindow(Some(hwnd)).as_bool() {
            return Err(format!("Stored {label} window handle is no longer valid"));
        }

        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }

        let foreground_hwnd = GetForegroundWindow();
        let foreground_thread = GetWindowThreadProcessId(foreground_hwnd, None);
        let current_thread = GetCurrentThreadId();
        let attached = foreground_thread != 0 && foreground_thread != current_thread;

        if attached {
            let _ = AttachThreadInput(foreground_thread, current_thread, true);
        }

        let result = SetForegroundWindow(hwnd);
        let _ = SetFocus(Some(hwnd));

        if attached {
            let _ = AttachThreadInput(foreground_thread, current_thread, false);
        }

        if !result.as_bool() {
            return Err(format!(
                "SetForegroundWindow failed while restoring {label}"
            ));
        }

        debug!(?hwnd, label, "Activated window via AttachThreadInput");
    }

    Ok(())
}
