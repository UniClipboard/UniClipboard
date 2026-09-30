//! Global modifier double-tap trigger for the native panel: tapping a modifier key twice, on its
//! own, opens the panel.
//!
//! The detection logic and the polling worker are shared with the GUI through `uc-desktop`. Only
//! the platform reader of the keyboard state lives here.
//!
//! The macOS reader is a copy of the one in the GUI's `modifier_double_tap_platform`. The GUI copy
//! for macOS and Windows goes away together with the WebView quick panel, which is the only reason
//! for having two.

use std::sync::Arc;

use gpui::Global;
use uc_desktop::modifier_double_tap_monitor::{
    ModifierDoubleTapMonitor, ModifierKeyState, ModifierKeyStateFactory,
};

/// The monitor, kept as a global so the panel can apply the persisted setting.
pub struct DoubleTap(pub ModifierDoubleTapMonitor);
impl Global for DoubleTap {}

/// Creates the monitor. `on_trigger` runs on the monitor's worker thread.
pub fn new_monitor(on_trigger: impl Fn() + Send + Sync + 'static) -> ModifierDoubleTapMonitor {
    ModifierDoubleTapMonitor::new(key_state_factory(), on_trigger)
}

fn key_state_factory() -> ModifierKeyStateFactory {
    Arc::new(platform::new_key_state)
}

#[cfg(target_os = "macos")]
mod platform {
    use objc2_application_services::AXIsProcessTrusted;
    use objc2_core_graphics::{CGEventFlags, CGEventSource, CGEventSourceStateID};
    use uc_daemon_contract::api::dto::settings::QuickPanelDoubleTapModifierDto as QuickPanelDoubleTapModifier;

    use super::ModifierKeyState;

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

    pub fn new_key_state() -> Result<Box<dyn ModifierKeyState>, String> {
        // Reading the global keyboard state needs the Accessibility permission. This only checks
        // it; it never opens the system prompt.
        if !unsafe { AXIsProcessTrusted() } {
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

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn the_selected_modifier_is_not_counted_as_another_modifier() {
            for modifier in [
                QuickPanelDoubleTapModifier::Alt,
                QuickPanelDoubleTapModifier::Control,
                QuickPanelDoubleTapModifier::Meta,
            ] {
                let selected = modifier_flag(modifier);
                assert!(!selected.is_empty());
                assert!(!other_modifier_flags(selected).intersects(selected));
            }
        }

        #[test]
        fn a_disabled_trigger_selects_no_key() {
            assert!(modifier_flag(QuickPanelDoubleTapModifier::Disabled).is_empty());
        }
    }
}

/// Windows is not implemented yet; the panel then simply has no double-tap trigger.
#[cfg(not(target_os = "macos"))]
mod platform {
    use super::ModifierKeyState;

    pub fn new_key_state() -> Result<Box<dyn ModifierKeyState>, String> {
        Err("modifier double-tap is not available on this platform yet".to_string())
    }
}
