//! Reads the keyboard state for the modifier double-tap trigger.
//!
//! This is a copy of the reader in the GUI's `modifier_double_tap_platform`. The GUI copy for
//! macOS and Windows goes away together with the WebView quick panel.

use objc2_application_services::AXIsProcessTrusted;
use objc2_core_graphics::{CGEventFlags, CGEventSource, CGEventSourceStateID};
use uc_daemon_contract::api::dto::settings::QuickPanelDoubleTapModifierDto as QuickPanelDoubleTapModifier;

use crate::app::double_tap::ModifierKeyState;

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
    // Some systems report a key code (observed: 0, "A") as down while nothing is pressed. Keys
    // that are already down now are not part of any tap and are ignored until they are released;
    // otherwise the detector would see another key held forever and never trigger.
    let stuck = (0..=KEY_CODE_MAX)
        .filter(|&key_code| {
            !MODIFIER_KEY_CODES.contains(&key_code) && CGEventSource::key_state(STATE_ID, key_code)
        })
        .collect();
    Ok(Box::new(MacosKeyState { stuck }))
}

const STATE_ID: CGEventSourceStateID = CGEventSourceStateID::CombinedSessionState;

struct MacosKeyState {
    /// Key codes that were down when the reader was created and have not been released since.
    stuck: Vec<u16>,
}

impl ModifierKeyState for MacosKeyState {
    fn snapshot(&mut self, modifier: QuickPanelDoubleTapModifier) -> (bool, bool) {
        let state_id = STATE_ID;
        let flags = CGEventSource::flags_state(state_id);
        let selected_mask = modifier_flag(modifier);
        let selected_down = flags.intersects(selected_mask);
        let other_modifier_down = flags.intersects(other_modifier_flags(selected_mask));
        self.stuck
            .retain(|&key_code| CGEventSource::key_state(state_id, key_code));
        let stuck = &self.stuck;
        let other_key_down = (0..=KEY_CODE_MAX).any(|key_code| {
            !MODIFIER_KEY_CODES.contains(&key_code)
                && !stuck.contains(&key_code)
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
