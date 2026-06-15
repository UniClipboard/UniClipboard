//! Cycle-detection state machine for the auto-sync loop.
//!
//! Pure port of uc-ios `Shared/Models/SyncLoopGuard.swift`. The engine records
//! every successful Apply (`Pulled`) and Push (`Pushed`); when the SAME hash
//! flips between the two more than `flip_threshold` times inside `window`, the
//! guard trips and the engine parks itself until the user acknowledges.
//!
//! Why count flips, not absolute counts: a healthy engine may legitimately see
//! N pushes (or N pulls) of one hash in a row. Only an *alternating* pattern is
//! the real ping-pong (apply → pasteboard echo → push → server echo → pull …).
//!
//! State is held as a plain `Vec<LoopGuardEvent>` owned by the caller (the M5
//! SyncEngine decision core, also Rust): [`record`] returns the next buffer,
//! [`tripped`] is a pure read, and `reset` is just dropping/clearing the vec.
//! Timestamps are epoch-milliseconds (consistent with the rest of the FFI
//! edge); `window` is seconds (Swift `TimeInterval`).

/// Which way a recorded event flowed (a subset of history `Direction` — the
/// loop guard only cares about completed apply/push, never `local`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LoopDirection {
    /// Server → device (apply).
    Pulled,
    /// Device → server (push).
    Pushed,
}

/// One recorded sync event. `hash` is stored uppercased (Swift `Event.init`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoopGuardEvent {
    /// Uppercase content hash.
    pub hash: String,
    /// Flow direction.
    pub direction: LoopDirection,
    /// Event time, epoch-milliseconds.
    pub at_millis: i64,
}

/// Default cycle-detection window (Swift `SyncLoopGuard.window`).
pub const DEFAULT_WINDOW_SECS: f64 = 30.0;
/// Default flip threshold (Swift `SyncLoopGuard.flipThreshold`): 3 flips ⇒ at
/// least 4 alternating events.
pub const DEFAULT_FLIP_THRESHOLD: i64 = 3;

/// Append a sync event, dropping anything older than `window_secs` relative to
/// `at_millis` so the buffer stays bounded by the cadence. Mirrors
/// `SyncLoopGuard.record`: `hash` is uppercased; a `None`/empty hash is ignored
/// entirely (no event added AND no eviction — Swift returns before the cutoff
/// sweep).
pub fn record(
    mut events: Vec<LoopGuardEvent>,
    direction: LoopDirection,
    hash: Option<&str>,
    at_millis: i64,
    window_secs: f64,
) -> Vec<LoopGuardEvent> {
    let hash = match hash {
        Some(h) if !h.is_empty() => h.to_uppercase(),
        _ => return events,
    };
    let cutoff = at_millis - (window_secs * 1000.0) as i64;
    events.retain(|e| e.at_millis >= cutoff);
    events.push(LoopGuardEvent {
        hash,
        direction,
        at_millis,
    });
    events
}

/// `true` when any single hash has alternated direction at least
/// `flip_threshold` times inside the buffer. Pure read (Swift
/// `SyncLoopGuard.tripped`).
pub fn tripped(events: &[LoopGuardEvent], flip_threshold: i64) -> bool {
    use std::collections::HashMap;
    let mut by_hash: HashMap<&str, Vec<&LoopGuardEvent>> = HashMap::new();
    for e in events {
        by_hash.entry(e.hash.as_str()).or_default().push(e);
    }
    for mut group in by_hash.into_values() {
        group.sort_by_key(|e| e.at_millis);
        let mut flips = 0i64;
        let mut last_dir: Option<LoopDirection> = None;
        for ev in group {
            if let Some(prev) = last_dir {
                if prev != ev.direction {
                    flips += 1;
                }
            }
            last_dir = Some(ev.direction);
        }
        if flips >= flip_threshold {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    // Reference instant; events stamp at +N seconds expressed as millis.
    const T0: i64 = 1_000_000;

    fn rec(
        events: Vec<LoopGuardEvent>,
        dir: LoopDirection,
        hash: &str,
        secs: i64,
    ) -> Vec<LoopGuardEvent> {
        record(
            events,
            dir,
            Some(hash),
            T0 + secs * 1000,
            DEFAULT_WINDOW_SECS,
        )
    }

    /// Swift `empty`.
    #[test]
    fn empty_guard_not_tripped() {
        assert!(!tripped(&[], DEFAULT_FLIP_THRESHOLD));
    }

    /// Swift `sameDirectionRepeated`.
    #[test]
    fn same_direction_repeat_not_tripped() {
        let mut events = Vec::new();
        for i in 0..10 {
            events = rec(events, LoopDirection::Pushed, "AABB", i);
        }
        assert!(!tripped(&events, DEFAULT_FLIP_THRESHOLD));
    }

    /// Swift `flipsOnSameHash`: 3 flips trips.
    #[test]
    fn three_flips_trips() {
        let mut events = rec(Vec::new(), LoopDirection::Pulled, "AABB", 0);
        assert!(!tripped(&events, DEFAULT_FLIP_THRESHOLD));
        events = rec(events, LoopDirection::Pushed, "AABB", 1);
        assert!(!tripped(&events, DEFAULT_FLIP_THRESHOLD)); // 1 flip
        events = rec(events, LoopDirection::Pulled, "AABB", 2);
        assert!(!tripped(&events, DEFAULT_FLIP_THRESHOLD)); // 2 flips
        events = rec(events, LoopDirection::Pushed, "AABB", 3);
        assert!(tripped(&events, DEFAULT_FLIP_THRESHOLD)); // 3 flips → trip
    }

    /// Swift `differentHashesDoNotCombine`.
    #[test]
    fn different_hashes_do_not_combine() {
        let mut events = rec(Vec::new(), LoopDirection::Pulled, "AAAA", 0);
        events = rec(events, LoopDirection::Pushed, "BBBB", 1);
        events = rec(events, LoopDirection::Pulled, "CCCC", 2);
        events = rec(events, LoopDirection::Pushed, "DDDD", 3);
        assert!(!tripped(&events, DEFAULT_FLIP_THRESHOLD));
    }

    /// Swift `windowEviction`: old events are dropped at record time.
    #[test]
    fn old_events_evicted_on_record() {
        let mut events = record(Vec::new(), LoopDirection::Pulled, Some("AABB"), T0, 5.0);
        events = record(events, LoopDirection::Pushed, Some("AABB"), T0 + 1000, 5.0);
        events = record(events, LoopDirection::Pulled, Some("AABB"), T0 + 2000, 5.0);
        // New event is > window (5s) away → prior events evicted, leaving one.
        events = record(
            events,
            LoopDirection::Pushed,
            Some("AABB"),
            T0 + 100_000,
            5.0,
        );
        assert!(!tripped(&events, DEFAULT_FLIP_THRESHOLD));
        assert_eq!(events.len(), 1);
    }

    /// Swift `caseInsensitive`: hashes are uppercased on record.
    #[test]
    fn hash_case_normalized() {
        let mut events = rec(Vec::new(), LoopDirection::Pulled, "aabb", 0);
        events = rec(events, LoopDirection::Pushed, "AABB", 1);
        events = rec(events, LoopDirection::Pulled, "AaBb", 2);
        events = rec(events, LoopDirection::Pushed, "aaBB", 3);
        assert!(tripped(&events, DEFAULT_FLIP_THRESHOLD));
    }

    /// Swift `nilAndEmptyHashIgnored`.
    #[test]
    fn nil_and_empty_hash_ignored() {
        let mut events = record(
            Vec::new(),
            LoopDirection::Pulled,
            None,
            T0,
            DEFAULT_WINDOW_SECS,
        );
        events = record(
            events,
            LoopDirection::Pushed,
            Some(""),
            T0 + 1000,
            DEFAULT_WINDOW_SECS,
        );
        events = record(
            events,
            LoopDirection::Pulled,
            None,
            T0 + 2000,
            DEFAULT_WINDOW_SECS,
        );
        assert!(events.is_empty());
        assert!(!tripped(&events, DEFAULT_FLIP_THRESHOLD));
    }

    /// Swift `resetClears`: dropping the buffer untrips.
    #[test]
    fn reset_clears_and_untrips() {
        let mut events = rec(Vec::new(), LoopDirection::Pulled, "AABB", 0);
        events = rec(events, LoopDirection::Pushed, "AABB", 1);
        events = rec(events, LoopDirection::Pulled, "AABB", 2);
        events = rec(events, LoopDirection::Pushed, "AABB", 3);
        assert!(tripped(&events, DEFAULT_FLIP_THRESHOLD));
        events.clear(); // reset == drop the buffer
        assert!(!tripped(&events, DEFAULT_FLIP_THRESHOLD));
        assert!(events.is_empty());
    }
}
