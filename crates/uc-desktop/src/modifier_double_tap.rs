use std::time::{Duration, Instant};

pub const DEFAULT_DOUBLE_TAP_WINDOW: Duration = Duration::from_millis(400);

/// Detects two standalone taps of one logical modifier from keyboard snapshots.
///
/// The adapter supplies whether the selected modifier is down and whether any
/// other key is down. Any other key invalidates the active tap and clears a
/// pending first tap, so ordinary shortcuts cannot complete the gesture.
///
/// This snapshot-based detector assumes the adapter samples often enough to
/// observe every relevant key transition. A complete press and release between
/// two samples is invisible and therefore cannot invalidate a modifier tap.
pub struct ModifierDoubleTapDetector {
    window: Duration,
    initialized: bool,
    was_down: bool,
    active_tap_invalid: bool,
    last_tap_released_at: Option<Instant>,
}

impl Default for ModifierDoubleTapDetector {
    fn default() -> Self {
        Self::new(DEFAULT_DOUBLE_TAP_WINDOW)
    }
}

impl ModifierDoubleTapDetector {
    pub fn new(window: Duration) -> Self {
        Self {
            window,
            initialized: false,
            was_down: false,
            active_tap_invalid: false,
            last_tap_released_at: None,
        }
    }

    pub fn observe(
        &mut self,
        selected_modifier_down: bool,
        any_other_key_down: bool,
        now: Instant,
    ) -> bool {
        if !self.initialized {
            self.initialized = true;
            self.was_down = selected_modifier_down;
            self.active_tap_invalid = selected_modifier_down || any_other_key_down;
            return false;
        }

        if any_other_key_down {
            self.last_tap_released_at = None;
            if selected_modifier_down {
                self.active_tap_invalid = true;
            }
        }

        if selected_modifier_down && !self.was_down {
            self.active_tap_invalid = any_other_key_down;
        }

        let triggered = if !selected_modifier_down && self.was_down {
            let valid_tap = !self.active_tap_invalid && !any_other_key_down;
            self.active_tap_invalid = false;

            if valid_tap {
                let is_double_tap = self
                    .last_tap_released_at
                    .is_some_and(|released_at| now.duration_since(released_at) <= self.window);
                self.last_tap_released_at = if is_double_tap { None } else { Some(now) };
                is_double_tap
            } else {
                false
            }
        } else {
            false
        };

        self.was_down = selected_modifier_down;
        triggered
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn observe(
        detector: &mut ModifierDoubleTapDetector,
        start: Instant,
        millis: u64,
        selected: bool,
        other: bool,
    ) -> bool {
        detector.observe(selected, other, start + Duration::from_millis(millis))
    }

    #[test]
    fn triggers_after_two_clean_taps_inside_window() {
        let start = Instant::now();
        let mut detector = ModifierDoubleTapDetector::default();

        assert!(!observe(&mut detector, start, 0, false, false));
        assert!(!observe(&mut detector, start, 20, true, false));
        assert!(!observe(&mut detector, start, 70, false, false));
        assert!(!observe(&mut detector, start, 180, true, false));
        assert!(observe(&mut detector, start, 230, false, false));
    }

    #[test]
    fn does_not_trigger_when_second_tap_is_too_late() {
        let start = Instant::now();
        let mut detector = ModifierDoubleTapDetector::default();

        assert!(!observe(&mut detector, start, 0, false, false));
        assert!(!observe(&mut detector, start, 20, true, false));
        assert!(!observe(&mut detector, start, 70, false, false));
        assert!(!observe(&mut detector, start, 500, true, false));
        assert!(!observe(&mut detector, start, 550, false, false));
    }

    #[test]
    fn other_key_during_or_between_taps_clears_candidate() {
        let start = Instant::now();
        let mut detector = ModifierDoubleTapDetector::default();

        assert!(!observe(&mut detector, start, 0, false, false));
        assert!(!observe(&mut detector, start, 20, true, false));
        assert!(!observe(&mut detector, start, 40, true, true));
        assert!(!observe(&mut detector, start, 70, false, false));
        assert!(!observe(&mut detector, start, 140, true, false));
        assert!(!observe(&mut detector, start, 190, false, false));

        assert!(!observe(&mut detector, start, 240, false, true));
        assert!(!observe(&mut detector, start, 260, false, false));
        assert!(!observe(&mut detector, start, 300, true, false));
        assert!(!observe(&mut detector, start, 350, false, false));
    }

    #[test]
    fn enabling_while_modifier_is_held_does_not_count_partial_tap() {
        let start = Instant::now();
        let mut detector = ModifierDoubleTapDetector::default();

        assert!(!observe(&mut detector, start, 0, true, false));
        assert!(!observe(&mut detector, start, 40, false, false));
        assert!(!observe(&mut detector, start, 100, true, false));
        assert!(!observe(&mut detector, start, 150, false, false));
    }
}
