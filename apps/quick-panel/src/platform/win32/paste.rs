//! Pasting into the window that was in front before the panel opened.

use std::mem::size_of;
use std::time::{Duration, Instant};

use quick_panel_core::ports::{PasteTarget, PlatformError};
use windows::core::PWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND};
use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
use windows::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, QueryFullProcessImageNameW,
    PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
    KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, VIRTUAL_KEY, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN,
    VK_SHIFT, VK_V,
};

use super::focus;

/// How long to wait for the target to become the foreground window after asking for it.
const FOREGROUND_WAIT: Duration = Duration::from_millis(600);
/// How long to wait for held modifiers to be released before neutralizing them.
const RELEASE_WAIT: Duration = Duration::from_millis(150);
const POLL: Duration = Duration::from_millis(10);
/// Gives the restored window time to take the focus before keys are sent to it.
const SETTLE: Duration = Duration::from_millis(40);

/// The window that was in front when the panel opened.
pub struct ForegroundWindow {
    /// The top-level window, and the child of it that held the keyboard focus.
    target: Option<(HWND, HWND)>,
    /// Runs the focus hand-back after the current GPUI update, see `return_focus`.
    executor: gpui::ForegroundExecutor,
}

impl ForegroundWindow {
    pub fn capture(cx: &gpui::App) -> Self {
        let executor = cx.foreground_executor().clone();
        let top = focus::foreground();
        if top.is_invalid() || focus::is_ours(top) {
            return Self {
                target: None,
                executor,
            };
        }
        Self {
            target: Some((top, focus::focused_child(top))),
            executor,
        }
    }

    fn target(&self) -> Result<(HWND, HWND), PlatformError> {
        self.target.ok_or(PlatformError::NoPasteTarget)
    }

    /// Brings the target back to the foreground and waits until it is there.
    fn bring_front(&self) -> Result<(), PlatformError> {
        let (top, child) = self.target()?;
        if focus::foreground() == top {
            return Ok(());
        }
        focus::activate(top, child);
        let deadline = Instant::now() + FOREGROUND_WAIT;
        while Instant::now() < deadline {
            if focus::foreground() == top {
                std::thread::sleep(SETTLE);
                return Ok(());
            }
            std::thread::sleep(POLL);
        }
        Err(PlatformError::CannotReturnToTarget)
    }
}

impl PasteTarget for ForegroundWindow {
    /// The executable name of the window the paste goes to, without its extension.
    fn name(&self) -> Option<String> {
        let (top, _) = self.target?;
        let (_, process) = focus::owner(top);
        image_name(process)
    }

    fn check(&self) -> Result<(), PlatformError> {
        let (top, _) = self.target()?;
        if !focus::is_window(top) {
            return Err(PlatformError::PasteTargetQuit);
        }
        // Windows' integrity rules (UIPI) drop input injected into a window of a more privileged
        // process, so the paste would silently go nowhere.
        let (_, process) = focus::owner(top);
        if is_elevated(process) && !is_elevated_self() {
            return Err(PlatformError::NoPastePermission);
        }
        // The panel itself is in front while it is open; anything else that is not the target
        // means the user moved on.
        let front = focus::foreground();
        if front != top && !focus::is_ours(front) && focus::owner(front).1 != process {
            return Err(PlatformError::FocusMoved);
        }
        Ok(())
    }

    /// When the panel is in front, hands the focus back to the target. Nothing happens if the
    /// user has already gone to another window.
    ///
    /// Activating another window deactivates ours synchronously, and GPUI handles that by
    /// borrowing the window state the running update already holds, so it runs afterwards.
    fn return_focus(&self) {
        if let Some((top, child)) = self.target {
            self.executor
                .spawn(async move {
                    if focus::is_ours(focus::foreground()) {
                        focus::activate(top, child);
                    }
                })
                .detach();
        }
    }

    fn type_text(&self, text: &str) -> Result<(), PlatformError> {
        // An empty string produces no INPUT entries; sending an empty batch would be reported as a failure.
        if text.is_empty() {
            return Ok(());
        }
        self.check()?;
        self.bring_front()?;
        let mut inputs = Vec::new();
        for unit in text.encode_utf16() {
            inputs.push(unicode_input(unit, false));
            inputs.push(unicode_input(unit, true));
        }
        send(&inputs).map_err(|_| PlatformError::CannotCreateTypingEvent)
    }

    fn paste(&self) -> Result<(), PlatformError> {
        self.check()?;
        self.bring_front()?;
        // Held modifiers (Shift of Shift+Enter, the Alt of Ctrl+Alt+V) would turn Ctrl+V into a
        // different shortcut in the target. Wait for the release; when a key stays down, release
        // it for the duration of the paste and press it again afterwards.
        let held = release_held_modifiers();
        let mut inputs: Vec<INPUT> = held.iter().map(|key| key_input(*key, true)).collect();
        inputs.extend([
            key_input(VK_CONTROL, false),
            key_input(VK_V, false),
            key_input(VK_V, true),
            key_input(VK_CONTROL, true),
        ]);
        inputs.extend(held.iter().map(|key| key_input(*key, false)));
        send(&inputs).map_err(|_| PlatformError::CannotCreatePasteEvent)
    }
}

const NEUTRALIZED: [VIRTUAL_KEY; 4] = [VK_MENU, VK_SHIFT, VK_LWIN, VK_RWIN];

fn is_down(key: VIRTUAL_KEY) -> bool {
    (unsafe { GetAsyncKeyState(i32::from(key.0)) } as u16) & 0x8000 != 0
}

/// Waits briefly for the modifiers other than Ctrl to come up, and returns those still down.
fn release_held_modifiers() -> Vec<VIRTUAL_KEY> {
    let deadline = Instant::now() + RELEASE_WAIT;
    loop {
        let held: Vec<VIRTUAL_KEY> = NEUTRALIZED
            .into_iter()
            .filter(|key| is_down(*key))
            .collect();
        if held.is_empty() || Instant::now() >= deadline {
            return held;
        }
        std::thread::sleep(POLL);
    }
}

fn key_input(key: VIRTUAL_KEY, up: bool) -> INPUT {
    keyboard(KEYBDINPUT {
        wVk: key,
        dwFlags: if up {
            KEYEVENTF_KEYUP
        } else {
            KEYBD_EVENT_FLAGS(0)
        },
        ..Default::default()
    })
}

fn unicode_input(unit: u16, up: bool) -> INPUT {
    keyboard(KEYBDINPUT {
        wScan: unit,
        dwFlags: if up {
            KEYEVENTF_UNICODE | KEYEVENTF_KEYUP
        } else {
            KEYEVENTF_UNICODE
        },
        ..Default::default()
    })
}

fn keyboard(input: KEYBDINPUT) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 { ki: input },
    }
}

fn send(inputs: &[INPUT]) -> Result<(), ()> {
    if inputs.is_empty() {
        return Err(());
    }
    let sent = unsafe { SendInput(inputs, size_of::<INPUT>() as i32) };
    if sent as usize == inputs.len() {
        Ok(())
    } else {
        Err(())
    }
}

fn image_name(process: u32) -> Option<String> {
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process).ok()?;
        let mut buffer = [0_u16; 1024];
        let mut length = buffer.len() as u32;
        let queried = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut length,
        );
        let _ = CloseHandle(handle);
        queried.ok()?;
        let path = String::from_utf16_lossy(&buffer[..length as usize]);
        std::path::Path::new(&path)
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
    }
}

/// Whether `process` runs elevated. A process whose token cannot be opened is a more privileged
/// one, so that counts as elevated.
fn is_elevated(process: u32) -> bool {
    unsafe {
        let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process) else {
            return true;
        };
        let elevated = token_elevated(handle).unwrap_or(true);
        let _ = CloseHandle(handle);
        elevated
    }
}

fn is_elevated_self() -> bool {
    unsafe { token_elevated(GetCurrentProcess()).unwrap_or(false) }
}

unsafe fn token_elevated(process: HANDLE) -> Option<bool> {
    let mut token = HANDLE::default();
    OpenProcessToken(process, TOKEN_QUERY, &mut token).ok()?;
    let mut elevation = TOKEN_ELEVATION::default();
    let mut returned = 0_u32;
    let queried = GetTokenInformation(
        token,
        TokenElevation,
        Some((&mut elevation as *mut TOKEN_ELEVATION).cast()),
        size_of::<TOKEN_ELEVATION>() as u32,
        &mut returned,
    );
    let _ = CloseHandle(token);
    queried.ok()?;
    Some(elevation.TokenIsElevated != 0)
}
