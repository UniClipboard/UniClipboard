//! Reads the keyboard state for the modifier double-tap trigger.
//!
//! This follows `apps/gui-go/modifier_keys_windows.go`, so the trigger recognizes the same keys as
//! the main window's monitor: the physical state from `GetAsyncKeyState`, whichever window has the
//! focus, and no permission to ask for.

use uc_daemon_contract::api::dto::settings::QuickPanelDoubleTapModifierDto as QuickPanelDoubleTapModifier;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VK_CONTROL, VK_LCONTROL, VK_LMENU, VK_LWIN, VK_MENU, VK_RCONTROL, VK_RMENU,
    VK_RWIN,
};

use crate::app::double_tap::ModifierKeyState;

/// Mouse buttons occupy `0x01..=0x06`; they are not keyboard activity.
const FIRST_KEY: i32 = 0x07;
const LAST_KEY: i32 = 0xfe;

pub fn new_key_state() -> Result<Box<dyn ModifierKeyState>, String> {
    Ok(Box::new(WindowsKeyState))
}

struct WindowsKeyState;

impl ModifierKeyState for WindowsKeyState {
    fn snapshot(&mut self, modifier: QuickPanelDoubleTapModifier) -> (bool, bool) {
        let selected = selected_keys(modifier);
        let selected_down = selected.iter().any(|key| key_down(*key));
        let other_down =
            (FIRST_KEY..=LAST_KEY).any(|key| !selected.contains(&key) && key_down(key));
        (selected_down, other_down)
    }
}

fn selected_keys(modifier: QuickPanelDoubleTapModifier) -> Vec<i32> {
    let keys = match modifier {
        QuickPanelDoubleTapModifier::Disabled => &[][..],
        QuickPanelDoubleTapModifier::Alt => &[VK_MENU, VK_LMENU, VK_RMENU][..],
        QuickPanelDoubleTapModifier::Control => &[VK_CONTROL, VK_LCONTROL, VK_RCONTROL][..],
        QuickPanelDoubleTapModifier::Meta => &[VK_LWIN, VK_RWIN][..],
    };
    keys.iter().map(|key| i32::from(key.0)).collect()
}

fn key_down(key: i32) -> bool {
    (unsafe { GetAsyncKeyState(key) } as u16) & 0x8000 != 0
}
