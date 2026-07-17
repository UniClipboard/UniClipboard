use std::sync::Arc;

use serde::Serialize;
use uc_core::settings::model::QuickPanelDoubleTapModifier;
use uc_desktop::modifier_double_tap_monitor::{ModifierKeyState, ModifierKeyStateFactory};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ModifierDoubleTapAvailability {
    Supported,
    AccessibilityPermissionRequired,
    UnsupportedDisplaySession,
}

impl ModifierDoubleTapAvailability {
    pub fn is_supported(self) -> bool {
        self == Self::Supported
    }
}

pub fn modifier_double_tap_availability() -> ModifierDoubleTapAvailability {
    platform::availability()
}

pub fn modifier_key_state_factory() -> ModifierKeyStateFactory {
    Arc::new(platform::new_key_state)
}

#[cfg(any(target_os = "linux", test))]
fn linux_session_availability(
    display_present: bool,
    wayland_display_present: bool,
) -> ModifierDoubleTapAvailability {
    if wayland_display_present || !display_present {
        ModifierDoubleTapAvailability::UnsupportedDisplaySession
    } else {
        ModifierDoubleTapAvailability::Supported
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use objc2_application_services::AXIsProcessTrusted;
    use objc2_core_graphics::{CGEventFlags, CGEventSource, CGEventSourceStateID};

    use super::*;

    const KEY_CODE_MAX: u16 = 0x7f;
    const MODIFIER_KEY_CODES: &[u16] = &[
        0x36, // Right Command
        0x37, // Command
        0x38, // Shift
        0x39, // Caps Lock
        0x3a, // Option
        0x3b, // Control
        0x3c, // Right Shift
        0x3d, // Right Option
        0x3e, // Right Control
        0x3f, // Function
    ];

    pub fn availability() -> ModifierDoubleTapAvailability {
        if unsafe { AXIsProcessTrusted() } {
            ModifierDoubleTapAvailability::Supported
        } else {
            ModifierDoubleTapAvailability::AccessibilityPermissionRequired
        }
    }

    pub fn new_key_state() -> Result<Box<dyn ModifierKeyState>, String> {
        if !availability().is_supported() {
            return Err("macOS Accessibility permission is required".to_string());
        }
        Ok(Box::new(MacosKeyState))
    }

    struct MacosKeyState;

    impl ModifierKeyState for MacosKeyState {
        fn snapshot(&mut self, modifier: QuickPanelDoubleTapModifier) -> (bool, bool) {
            let state_id = CGEventSourceStateID::CombinedSessionState;
            let flags = CGEventSource::flags_state(state_id);
            let selected_mask = modifier_flag(modifier);
            let selected_down = flags.intersects(selected_mask);
            let other_modifier_down = flags.intersects(other_modifier_flags(selected_mask));
            let other_key_down = (0..=KEY_CODE_MAX).any(|key_code| {
                !MODIFIER_KEY_CODES.contains(&key_code)
                    && CGEventSource::key_state(state_id, key_code)
            });

            (selected_down, other_modifier_down || other_key_down)
        }
    }

    fn modifier_flag(modifier: QuickPanelDoubleTapModifier) -> CGEventFlags {
        match modifier {
            QuickPanelDoubleTapModifier::Disabled => CGEventFlags::empty(),
            QuickPanelDoubleTapModifier::Alt => CGEventFlags::MaskAlternate,
            QuickPanelDoubleTapModifier::Control => CGEventFlags::MaskControl,
            QuickPanelDoubleTapModifier::Meta => CGEventFlags::MaskCommand,
        }
    }

    fn other_modifier_flags(selected: CGEventFlags) -> CGEventFlags {
        let mut flags = CGEventFlags::MaskShift
            | CGEventFlags::MaskControl
            | CGEventFlags::MaskAlternate
            | CGEventFlags::MaskCommand
            | CGEventFlags::MaskSecondaryFn;
        flags.remove(selected);
        flags
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;

    use super::*;

    const VK_LBUTTON: i32 = 0x01;
    const VK_XBUTTON2: i32 = 0x06;
    const VK_CONTROL: i32 = 0x11;
    const VK_MENU: i32 = 0x12;
    const VK_LWIN: i32 = 0x5b;
    const VK_RWIN: i32 = 0x5c;
    const VK_LCONTROL: i32 = 0xa2;
    const VK_RCONTROL: i32 = 0xa3;
    const VK_LMENU: i32 = 0xa4;
    const VK_RMENU: i32 = 0xa5;

    pub fn availability() -> ModifierDoubleTapAvailability {
        ModifierDoubleTapAvailability::Supported
    }

    pub fn new_key_state() -> Result<Box<dyn ModifierKeyState>, String> {
        Ok(Box::new(WindowsKeyState))
    }

    struct WindowsKeyState;

    impl ModifierKeyState for WindowsKeyState {
        fn snapshot(&mut self, modifier: QuickPanelDoubleTapModifier) -> (bool, bool) {
            let selected_keys = selected_keys(modifier);
            let selected_down = selected_keys.iter().copied().any(is_key_down);
            let other_down = (0x07..=0xfe)
                .filter(|key| !selected_keys.contains(key))
                .any(is_key_down);
            (selected_down, other_down)
        }
    }

    fn selected_keys(modifier: QuickPanelDoubleTapModifier) -> &'static [i32] {
        match modifier {
            QuickPanelDoubleTapModifier::Disabled => &[],
            QuickPanelDoubleTapModifier::Alt => &[VK_MENU, VK_LMENU, VK_RMENU],
            QuickPanelDoubleTapModifier::Control => &[VK_CONTROL, VK_LCONTROL, VK_RCONTROL],
            QuickPanelDoubleTapModifier::Meta => &[VK_LWIN, VK_RWIN],
        }
    }

    fn is_key_down(key: i32) -> bool {
        if (VK_LBUTTON..=VK_XBUTTON2).contains(&key) {
            return false;
        }
        unsafe { (GetAsyncKeyState(key) as u16 & 0x8000) != 0 }
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use std::os::raw::c_char;
    use std::ptr;

    use x11_dl::{keysym, xlib};

    use super::*;

    pub fn availability() -> ModifierDoubleTapAvailability {
        linux_session_availability(
            std::env::var_os("DISPLAY").is_some(),
            std::env::var_os("WAYLAND_DISPLAY").is_some(),
        )
    }

    pub fn new_key_state() -> Result<Box<dyn ModifierKeyState>, String> {
        if !availability().is_supported() {
            return Err("modifier double-tap requires a native X11 session".to_string());
        }
        LinuxKeyState::new().map(|state| Box::new(state) as Box<dyn ModifierKeyState>)
    }

    struct LinuxKeyState {
        xlib: xlib::Xlib,
        display: *mut xlib::Display,
    }

    // The display connection is created and consumed by one monitor worker.
    unsafe impl Send for LinuxKeyState {}

    impl LinuxKeyState {
        fn new() -> Result<Self, String> {
            let xlib = xlib::Xlib::open()
                .map_err(|error| format!("failed to load X11 for modifier double-tap: {error}"))?;
            let display = unsafe { (xlib.XOpenDisplay)(ptr::null()) };
            if display.is_null() {
                return Err("failed to open the X11 display for modifier double-tap".to_string());
            }
            Ok(Self { xlib, display })
        }

        fn selected_keycodes(&self, modifier: QuickPanelDoubleTapModifier) -> Vec<u8> {
            let keysyms: &[u32] = match modifier {
                QuickPanelDoubleTapModifier::Disabled => &[],
                QuickPanelDoubleTapModifier::Alt => &[keysym::XK_Alt_L, keysym::XK_Alt_R],
                QuickPanelDoubleTapModifier::Control => {
                    &[keysym::XK_Control_L, keysym::XK_Control_R]
                }
                QuickPanelDoubleTapModifier::Meta => &[
                    keysym::XK_Super_L,
                    keysym::XK_Super_R,
                    keysym::XK_Meta_L,
                    keysym::XK_Meta_R,
                ],
            };

            keysyms
                .iter()
                .map(|keysym| unsafe {
                    (self.xlib.XKeysymToKeycode)(self.display, u64::from(*keysym))
                })
                .filter(|keycode| *keycode != 0)
                .collect()
        }
    }

    impl ModifierKeyState for LinuxKeyState {
        fn snapshot(&mut self, modifier: QuickPanelDoubleTapModifier) -> (bool, bool) {
            let mut keymap = [0 as c_char; 32];
            unsafe {
                (self.xlib.XQueryKeymap)(self.display, keymap.as_mut_ptr());
            }
            let selected = self.selected_keycodes(modifier);
            let selected_down = selected
                .iter()
                .copied()
                .any(|keycode| key_is_down(&keymap, keycode));
            let other_down = (8u8..=u8::MAX)
                .filter(|keycode| !selected.contains(keycode))
                .any(|keycode| key_is_down(&keymap, keycode));
            (selected_down, other_down)
        }
    }

    impl Drop for LinuxKeyState {
        fn drop(&mut self) {
            unsafe {
                (self.xlib.XCloseDisplay)(self.display);
            }
        }
    }

    fn key_is_down(keymap: &[c_char; 32], keycode: u8) -> bool {
        let byte = keymap[usize::from(keycode / 8)] as u8;
        byte & (1 << (keycode % 8)) != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wayland_is_unsupported_even_when_xwayland_sets_display() {
        assert_eq!(
            linux_session_availability(true, true),
            ModifierDoubleTapAvailability::UnsupportedDisplaySession
        );
    }

    #[test]
    fn native_x11_is_supported() {
        assert_eq!(
            linux_session_availability(true, false),
            ModifierDoubleTapAvailability::Supported
        );
    }
}
