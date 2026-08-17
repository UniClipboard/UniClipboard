#[cfg(target_os = "windows")]
use std::{
    sync::{mpsc::SyncSender, Arc, Mutex, OnceLock},
    thread::JoinHandle,
};

#[cfg(target_os = "windows")]
use tracing::{debug, error, info, warn};

#[cfg(any(target_os = "windows", test))]
const VK_LWIN: u32 = 0x5B;
#[cfg(any(target_os = "windows", test))]
const VK_RWIN: u32 = 0x5C;
#[cfg(any(target_os = "windows", test))]
const VK_V: u32 = b'V' as u32;

#[cfg(any(target_os = "windows", test))]
#[derive(Default)]
struct WinVKeyState {
    left_windows_key_down: bool,
    right_windows_key_down: bool,
    v_key_down: bool,
    suppressed_v_key_down: bool,
    windows_menu_mask_pending: bool,
}

#[cfg(any(target_os = "windows", test))]
#[derive(Debug, Eq, PartialEq)]
enum KeyDownAction {
    PassThrough,
    Suppress,
    TriggerPanel,
}

#[cfg(any(target_os = "windows", test))]
impl WinVKeyState {
    fn handle_key_down(&mut self, virtual_key: u32) -> KeyDownAction {
        match virtual_key {
            VK_LWIN => {
                self.left_windows_key_down = true;
                return KeyDownAction::PassThrough;
            }
            VK_RWIN => {
                self.right_windows_key_down = true;
                return KeyDownAction::PassThrough;
            }
            _ => {}
        }

        if virtual_key == VK_V {
            if self.v_key_down {
                return if self.suppressed_v_key_down {
                    KeyDownAction::Suppress
                } else {
                    KeyDownAction::PassThrough
                };
            }

            let should_intercept = self.windows_key_down();
            self.v_key_down = true;
            self.suppressed_v_key_down = should_intercept;
            self.windows_menu_mask_pending = should_intercept;
            return if should_intercept {
                KeyDownAction::TriggerPanel
            } else {
                KeyDownAction::PassThrough
            };
        }

        KeyDownAction::PassThrough
    }

    fn handle_key_up(&mut self, virtual_key: u32) -> bool {
        match virtual_key {
            VK_LWIN => self.left_windows_key_down = false,
            VK_RWIN => self.right_windows_key_down = false,
            VK_V => {
                self.v_key_down = false;
                return std::mem::take(&mut self.suppressed_v_key_down);
            }
            _ => {}
        }

        false
    }

    fn cancel_v_interception(&mut self) {
        self.suppressed_v_key_down = false;
        self.windows_menu_mask_pending = false;
    }

    fn windows_key_down(&self) -> bool {
        self.left_windows_key_down || self.right_windows_key_down
    }

    fn take_windows_menu_mask(&mut self) -> bool {
        std::mem::take(&mut self.windows_menu_mask_pending)
    }
}

#[cfg(any(target_os = "windows", test))]
/// Owns the Windows-only interception of `Win+V`.
///
/// The hook callback does only key recognition plus a non-blocking signal. The
/// panel toggle runs on a separate dispatcher thread, keeping Windows input
/// delivery fast enough that the OS does not remove the hook for timing out.
pub struct WinVInterceptor {
    #[cfg(target_os = "windows")]
    state: Mutex<Option<HookThread>>,
    #[cfg(target_os = "windows")]
    context: Arc<HookContext>,
}

impl WinVInterceptor {
    pub(crate) fn new(on_pressed: impl Fn() + Send + Sync + 'static) -> Self {
        #[cfg(target_os = "windows")]
        {
            let (sender, receiver) = std::sync::mpsc::sync_channel(4);
            let on_pressed = Arc::new(on_pressed);
            std::thread::spawn(move || {
                while receiver.recv().is_ok() {
                    on_pressed();
                }
            });

            Self {
                state: Mutex::new(None),
                context: Arc::new(HookContext {
                    sender,
                    key_state: Mutex::new(WinVKeyState::default()),
                }),
            }
        }

        #[cfg(not(target_os = "windows"))]
        {
            let _ = on_pressed;
            Self {}
        }
    }

    pub(crate) fn set_enabled(&self, enabled: bool) -> Result<(), String> {
        #[cfg(target_os = "windows")]
        {
            if enabled {
                self.enable()
            } else {
                self.disable()
            }
        }

        #[cfg(not(target_os = "windows"))]
        {
            let _ = enabled;
            Ok(())
        }
    }

    pub(crate) fn is_enabled(&self) -> bool {
        #[cfg(target_os = "windows")]
        {
            lock_or_recover(&self.state).is_some()
        }

        #[cfg(not(target_os = "windows"))]
        {
            false
        }
    }
}

#[cfg(target_os = "windows")]
struct HookContext {
    sender: SyncSender<()>,
    key_state: Mutex<WinVKeyState>,
}

#[cfg(target_os = "windows")]
struct HookThread {
    thread_id: u32,
    join: JoinHandle<()>,
}

#[cfg(target_os = "windows")]
static ACTIVE_HOOK_CONTEXT: OnceLock<Mutex<Option<Arc<HookContext>>>> = OnceLock::new();

#[cfg(target_os = "windows")]
fn active_hook_context() -> &'static Mutex<Option<Arc<HookContext>>> {
    ACTIVE_HOOK_CONTEXT.get_or_init(|| Mutex::new(None))
}

#[cfg(target_os = "windows")]
fn lock_or_recover<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

#[cfg(target_os = "windows")]
impl WinVInterceptor {
    fn enable(&self) -> Result<(), String> {
        let mut state = lock_or_recover(&self.state);
        if state.is_some() {
            return Ok(());
        }

        *lock_or_recover(active_hook_context()) = Some(Arc::clone(&self.context));

        let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(1);
        let join = std::thread::Builder::new()
            .name("win-v-quick-panel-hook".into())
            .spawn(move || run_hook_thread(ready_tx))
            .map_err(|error| {
                *lock_or_recover(active_hook_context()) = None;
                format!("failed to start Win+V input listener: {error}")
            })?;

        match ready_rx.recv() {
            Ok(Ok(thread_id)) => {
                *state = Some(HookThread { thread_id, join });
                Ok(())
            }
            Ok(Err(error)) => {
                *lock_or_recover(active_hook_context()) = None;
                let _ = join.join();
                Err(error)
            }
            Err(_) => {
                *lock_or_recover(active_hook_context()) = None;
                let _ = join.join();
                Err("Win+V input listener stopped before it became ready".into())
            }
        }
    }

    fn disable(&self) -> Result<(), String> {
        let hook = lock_or_recover(&self.state).take();
        let Some(hook) = hook else {
            return Ok(());
        };

        *lock_or_recover(active_hook_context()) = None;
        use windows::Win32::{
            Foundation::{LPARAM, WPARAM},
            UI::WindowsAndMessaging::{PostThreadMessageW, WM_QUIT},
        };

        unsafe { PostThreadMessageW(hook.thread_id, WM_QUIT, WPARAM(0), LPARAM(0)) }
            .map_err(|error| format!("failed to stop Win+V input listener: {error}"))?;
        hook.join
            .join()
            .map_err(|_| "Win+V input listener thread panicked".to_string())
    }
}

#[cfg(target_os = "windows")]
impl Drop for WinVInterceptor {
    fn drop(&mut self) {
        if let Err(error) = self.disable() {
            warn!(error = %error, "Failed to stop Win+V input listener during shutdown");
        }
    }
}

#[cfg(target_os = "windows")]
fn run_hook_thread(ready: SyncSender<Result<u32, String>>) {
    use windows::Win32::{
        Foundation::HINSTANCE,
        System::Threading::GetCurrentThreadId,
        UI::WindowsAndMessaging::{
            GetMessageW, PeekMessageW, SetWindowsHookExW, UnhookWindowsHookEx, MSG, PM_NOREMOVE,
            WH_KEYBOARD_LL,
        },
    };

    let mut message = MSG::default();
    // Create the thread's message queue before publishing its ID, so shutdown
    // can reliably post WM_QUIT immediately after startup.
    let _ = unsafe { PeekMessageW(&mut message, None, 0, 0, PM_NOREMOVE) };

    let hook = match unsafe {
        SetWindowsHookExW(
            WH_KEYBOARD_LL,
            Some(low_level_keyboard_proc),
            Some(HINSTANCE::default()),
            0,
        )
    } {
        Ok(hook) => hook,
        Err(error) => {
            let _ = ready.send(Err(format!(
                "failed to install Win+V input listener: {error}"
            )));
            return;
        }
    };

    let thread_id = unsafe { GetCurrentThreadId() };
    if ready.send(Ok(thread_id)).is_err() {
        let _ = unsafe { UnhookWindowsHookEx(hook) };
        return;
    }
    info!(thread_id, "Win+V input listener installed");

    while unsafe { GetMessageW(&mut message, None, 0, 0) }.0 > 0 {}

    if let Err(error) = unsafe { UnhookWindowsHookEx(hook) } {
        error!(error = %error, "Failed to remove Win+V input listener");
    }
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn low_level_keyboard_proc(
    code: i32,
    wparam: windows::Win32::Foundation::WPARAM,
    lparam: windows::Win32::Foundation::LPARAM,
) -> windows::Win32::Foundation::LRESULT {
    use windows::Win32::{
        Foundation::LRESULT,
        UI::WindowsAndMessaging::{
            CallNextHookEx, KBDLLHOOKSTRUCT, LLKHF_INJECTED, WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN,
            WM_SYSKEYUP,
        },
    };

    if code < 0 || lparam.0 == 0 {
        return unsafe { CallNextHookEx(None, code, wparam, lparam) };
    }

    let keyboard = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
    if keyboard.flags.contains(LLKHF_INJECTED) {
        return unsafe { CallNextHookEx(None, code, wparam, lparam) };
    }

    let message = wparam.0 as u32;
    let intercepted = match message {
        WM_KEYDOWN | WM_SYSKEYDOWN => active_hook_context()
            .lock()
            .ok()
            .and_then(|context| context.as_ref().cloned())
            .is_some_and(|context| {
                let mut key_state = lock_or_recover(&context.key_state);
                match key_state.handle_key_down(keyboard.vkCode) {
                    KeyDownAction::TriggerPanel => {
                        debug!("Win+V input listener received shortcut");
                        let dispatched = match context.sender.try_send(()) {
                            Ok(()) | Err(std::sync::mpsc::TrySendError::Full(())) => true,
                            Err(std::sync::mpsc::TrySendError::Disconnected(())) => {
                                key_state.cancel_v_interception();
                                warn!(
                                    error_kind = "win_v_dispatch_disconnected",
                                    retryable = false,
                                    "Win+V input listener lost its panel dispatcher"
                                );
                                false
                            }
                        };
                        let should_mask_windows_menu =
                            dispatched && key_state.take_windows_menu_mask();
                        drop(key_state);

                        if should_mask_windows_menu {
                            if let Err(error) = mask_windows_key_release() {
                                warn!(error = %error, "Failed to mask Windows key release after Win+V takeover");
                            }
                        }
                        dispatched
                    }
                    KeyDownAction::Suppress => true,
                    KeyDownAction::PassThrough => false,
                }
            }),
        WM_KEYUP | WM_SYSKEYUP => {
            if let Some(context) = lock_or_recover(active_hook_context()).as_ref().cloned() {
                return if lock_or_recover(&context.key_state).handle_key_up(keyboard.vkCode) {
                    LRESULT(1)
                } else {
                    unsafe { CallNextHookEx(None, code, wparam, lparam) }
                };
            }
            false
        }
        _ => false,
    };

    if intercepted {
        return LRESULT(1);
    }

    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

/// Prevent the shell from treating the later Windows-key release as a standalone press.
#[cfg(target_os = "windows")]
fn mask_windows_key_release() -> Result<(), String> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_KEYBOARD, KEYEVENTF_KEYUP, VIRTUAL_KEY,
    };

    // VK_E8 is an unassigned virtual key. This is the standard menu-mask pattern
    // used by keyboard-hook tools for suppressed Windows-key shortcuts.
    const MENU_MASK_KEY: VIRTUAL_KEY = VIRTUAL_KEY(0xE8);

    let mut key_down: INPUT = unsafe { std::mem::zeroed() };
    key_down.r#type = INPUT_KEYBOARD;
    key_down.Anonymous.ki.wVk = MENU_MASK_KEY;

    let mut key_up: INPUT = unsafe { std::mem::zeroed() };
    key_up.r#type = INPUT_KEYBOARD;
    key_up.Anonymous.ki.wVk = MENU_MASK_KEY;
    key_up.Anonymous.ki.dwFlags = KEYEVENTF_KEYUP;

    let inputs = [key_down, key_up];
    let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as _) };
    if sent == inputs.len() as u32 {
        Ok(())
    } else {
        Err(format!(
            "SendInput sent {sent} menu-mask events, expected {}",
            inputs.len()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intercepts_v_only_while_a_windows_key_is_held() {
        let mut state = WinVKeyState::default();

        assert_eq!(state.handle_key_down(VK_LWIN), KeyDownAction::PassThrough);
        assert_eq!(state.handle_key_down(VK_V), KeyDownAction::TriggerPanel);
    }

    #[test]
    fn does_not_intercept_v_after_the_windows_key_is_released() {
        let mut state = WinVKeyState::default();

        state.handle_key_down(VK_RWIN);
        state.handle_key_up(VK_RWIN);

        assert_eq!(state.handle_key_down(VK_V), KeyDownAction::PassThrough);
    }

    #[test]
    fn keeps_intercepting_while_either_windows_key_is_held() {
        let mut state = WinVKeyState::default();

        state.handle_key_down(VK_LWIN);
        state.handle_key_down(VK_RWIN);
        state.handle_key_up(VK_LWIN);

        assert_eq!(state.handle_key_down(VK_V), KeyDownAction::TriggerPanel);
    }

    #[test]
    fn ignores_other_keys_while_the_windows_key_is_held() {
        let mut state = WinVKeyState::default();

        state.handle_key_down(VK_LWIN);

        assert_eq!(
            state.handle_key_down(b'C' as u32),
            KeyDownAction::PassThrough
        );
    }

    #[test]
    fn triggers_once_until_v_is_released() {
        let mut state = WinVKeyState::default();

        state.handle_key_down(VK_LWIN);

        assert_eq!(state.handle_key_down(VK_V), KeyDownAction::TriggerPanel);
        assert_eq!(state.handle_key_down(VK_V), KeyDownAction::Suppress);

        state.handle_key_up(VK_V);
        assert_eq!(state.handle_key_down(VK_V), KeyDownAction::TriggerPanel);
    }

    #[test]
    fn requests_a_menu_mask_for_an_intercepted_win_v() {
        let mut state = WinVKeyState::default();

        state.handle_key_down(VK_LWIN);
        assert_eq!(state.handle_key_down(VK_V), KeyDownAction::TriggerPanel);

        assert!(state.take_windows_menu_mask());
        assert!(!state.take_windows_menu_mask());
    }

    #[test]
    fn suppresses_the_v_key_release_after_taking_over_win_v() {
        let mut state = WinVKeyState::default();

        state.handle_key_down(VK_LWIN);
        state.handle_key_down(VK_V);

        assert!(state.handle_key_up(VK_V));
    }

    #[test]
    fn passes_through_a_v_key_release_that_was_not_taken_over() {
        let mut state = WinVKeyState::default();

        state.handle_key_down(VK_V);

        assert!(!state.handle_key_up(VK_V));
    }
}
