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
    GetForegroundWindow, GetWindowThreadProcessId, IsIconic, IsWindow, SetForegroundWindow,
    ShowWindow, SW_RESTORE,
};

/// How long to wait after re-activating the previous app before sending Ctrl+V.
const RESTORE_SETTLE_MS: Duration = Duration::from_millis(40);
const ALT_RELEASE_WAIT_MS: Duration = Duration::from_millis(120);
const ALT_RELEASE_POLL_MS: Duration = Duration::from_millis(10);

/// The last foreground window before the quick panel claimed focus.
///
/// Stored as `isize` because `HWND` contains `*mut c_void` which is not `Send+Sync`.
static PREVIOUS_FOREGROUND_WINDOW: Mutex<Option<isize>> = Mutex::new(None);

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

    let _ = activate_window(hwnd, "quick panel");
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
                *guard = Some(foreground_hwnd.0 as isize);
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
    .map(|ptr| HWND(ptr as *mut std::ffi::c_void))
    .ok_or_else(|| "No previous foreground window captured".to_string())?;

    activate_window(hwnd, "previous app")
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
