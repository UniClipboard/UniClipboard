//! Global modifier double-tap trigger for the native panel: tapping a modifier key twice, on its
//! own, opens the panel.
//!
//! The detection logic and the polling worker are shared with the GUI through `uc-desktop`. The
//! platform reader of the keyboard state lives in `platform`.

use std::sync::Arc;

use gpui::Global;
use uc_desktop::modifier_double_tap_monitor::{ModifierDoubleTapMonitor, ModifierKeyStateFactory};

/// The monitor, kept as a global so the panel can apply the persisted setting.
pub struct DoubleTap(pub ModifierDoubleTapMonitor);
impl Global for DoubleTap {}

/// Creates the monitor. `on_trigger` runs on the monitor's worker thread.
pub fn new_monitor(on_trigger: impl Fn() + Send + Sync + 'static) -> ModifierDoubleTapMonitor {
    ModifierDoubleTapMonitor::new(key_state_factory(), on_trigger)
}

fn key_state_factory() -> ModifierKeyStateFactory {
    Arc::new(crate::platform::new_key_state)
}
