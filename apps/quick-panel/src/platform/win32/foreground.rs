//! Desktop foreground notifications delivered on the installing UI thread.

use std::cell::RefCell;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use windows::Win32::UI::WindowsAndMessaging::{EVENT_SYSTEM_FOREGROUND, WINEVENT_OUTOFCONTEXT};

thread_local! {
    // WinEvent callbacks have no user-data argument. The owning guard installs and clears this.
    static CALLBACK: RefCell<Option<Box<dyn Fn()>>> = RefCell::new(None);
}

pub struct ForegroundObserver {
    hook: HWINEVENTHOOK,
}

impl ForegroundObserver {
    pub fn new(callback: impl Fn() + 'static) -> anyhow::Result<Self> {
        anyhow::ensure!(
            CALLBACK.with(|slot| slot.borrow().is_none()),
            "Foreground observer already installed"
        );
        let hook = unsafe {
            SetWinEventHook(
                EVENT_SYSTEM_FOREGROUND,
                EVENT_SYSTEM_FOREGROUND,
                None,
                Some(changed),
                0,
                0,
                WINEVENT_OUTOFCONTEXT,
            )
        };
        anyhow::ensure!(
            !hook.is_invalid(),
            "Could not observe desktop foreground changes: {}",
            windows::core::Error::from_win32()
        );
        CALLBACK.with(|slot| *slot.borrow_mut() = Some(Box::new(callback)));
        Ok(Self { hook })
    }
}

impl Drop for ForegroundObserver {
    fn drop(&mut self) {
        if !unsafe { UnhookWinEvent(self.hook) }.as_bool() {
            tracing::warn!("Could not release desktop foreground observer");
        }
        CALLBACK.with(|slot| slot.borrow_mut().take());
    }
}

unsafe extern "system" fn changed(
    _: HWINEVENTHOOK,
    _: u32,
    _: HWND,
    _: i32,
    _: i32,
    _: u32,
    _: u32,
) {
    // Queue work only: the callback must never re-enter a GPUI window update.
    CALLBACK.with(|slot| {
        if let Some(callback) = slot.borrow().as_ref() {
            callback();
        }
    });
}
