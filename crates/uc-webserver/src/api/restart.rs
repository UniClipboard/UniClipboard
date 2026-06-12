//! Controlled-restart coordinator (ADR-008 P5-L L8c).
//!
//! Arbitrates the controlled-restart control plane with three guarantees:
//!
//! - **First-wins arbitration**: the first accepted restart locks in its target
//!   residency; any concurrent or later request returns `Conflict` carrying the
//!   already-locked-in target + generation rather than overwriting it.
//! - **Monotonic generation**: every accepted restart bumps a never-decreasing
//!   counter, even across an abort, so a successor daemon can totally-order
//!   handover records.
//! - **Single quiescing mutator**: this coordinator owns the L8b `quiescing`
//!   flag as the SOLE mutator. `request()` and `abort()` both take the same
//!   mutex, so the raise-quiescing/mark-in_progress and the lower-quiescing/
//!   clear-in_progress transitions serialize on one lock — closing the L8b R1-3
//!   abort-vs-reset race where the supervisor's abort and a fresh request could
//!   interleave and leave the flag and the in-progress state disagreeing.
//!
//! Production-neutral: the ONLY caller of [`RestartCoordinator::request`] is an
//! Oneshot daemon's `/lifecycle/restart` endpoint, and no Oneshot daemon exists
//! in production until L8d. So `quiescing` is still never raised in production —
//! the L8b admission gates keep reading a flag this coordinator never sets.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use uc_daemon_contract::api::types::DaemonResidency;

/// An accepted, in-flight restart request (ADR-008 P5-L L8c).
///
/// `target` is the residency the successor daemon should launch in; `generation`
/// is the monotonic id stamped when the request was accepted (used in the
/// handover record so a successor can totally-order restarts).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RestartRequest {
    /// Residency the successor daemon should be launched in.
    pub target: DaemonResidency,
    /// Monotonic generation stamped at accept time.
    pub generation: u64,
}

/// Outcome of a restart request (ADR-008 P5-L L8c).
///
/// Pure arbitration result — HTTP-agnostic, so the coordinator can be unit-tested
/// without an axum handler. The lifecycle handler maps these onto status codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestartOutcome {
    /// The request was accepted; `generation` is its freshly-bumped id.
    Accepted { generation: u64 },
    /// A restart is already in progress; carries the locked-in target +
    /// generation so the caller can report which restart won.
    Conflict {
        current_target: DaemonResidency,
        generation: u64,
    },
}

/// Mutex-guarded coordinator state.
#[derive(Default)]
struct Inner {
    /// The accepted, in-flight restart, if any. `Some` exactly while quiescing.
    in_progress: Option<RestartRequest>,
    /// Monotonic generation counter; bumped on every accept, never reset.
    generation: u64,
}

/// Controlled-restart coordinator (ADR-008 P5-L L8c).
///
/// Clone is cheap and shares state (both the mutex-guarded `Inner` and the
/// `quiescing` flag are `Arc`-backed), exactly like
/// [`crate::api::control_lease::ControlLeaseRegistry`]: every `DaemonApiState`
/// clone — and the Oneshot supervisor — observes the same arbitration state and
/// the same flag.
#[derive(Clone)]
pub struct RestartCoordinator {
    inner: Arc<Mutex<Inner>>,
    /// The L8b quiescing flag. This coordinator is its SOLE mutator; the L8b
    /// admission gates only ever read it. Shared (same `Arc`) with
    /// `DaemonApiState.quiescing` so a `request()`/`abort()` here is observed by
    /// every gate.
    quiescing: Arc<AtomicBool>,
}

impl RestartCoordinator {
    /// Build a coordinator that owns `quiescing` — pass the SAME `Arc` that
    /// `DaemonApiState.quiescing` holds so the L8b gates observe `request()` /
    /// `abort()` transitions.
    pub fn new(quiescing: Arc<AtomicBool>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner::default())),
            quiescing,
        }
    }

    /// First-wins arbitration (ADR-008 P5-L L8c).
    ///
    /// If a restart is already in progress, returns `Conflict` carrying the
    /// locked-in target + generation and does NOT touch `quiescing`. Otherwise
    /// bumps the monotonic generation, records the in-flight request, raises
    /// `quiescing`, and returns `Accepted`. Mutex-serialized against
    /// [`Self::abort`] so the "mark + raise" and "clear + lower" transitions
    /// never interleave.
    pub fn request(&self, target: DaemonResidency) -> RestartOutcome {
        let mut inner = self
            .inner
            .lock()
            .expect("restart coordinator mutex poisoned");
        if let Some(existing) = inner.in_progress {
            return RestartOutcome::Conflict {
                current_target: existing.target,
                generation: existing.generation,
            };
        }
        inner.generation += 1;
        let generation = inner.generation;
        inner.in_progress = Some(RestartRequest { target, generation });
        // Raise quiescing while holding the mutex so the gate-visible flag and
        // the in-progress state flip together (closes the L8b R1-3 race).
        self.quiescing.store(true, Ordering::SeqCst);
        RestartOutcome::Accepted { generation }
    }

    /// Abort the in-flight restart (ADR-008 P5-L L8c).
    ///
    /// Called by the Oneshot supervisor on a drain timeout: clears `in_progress`
    /// AND lowers `quiescing`, both under the mutex, so a concurrent
    /// [`Self::request`] either fully precedes (and is then aborted) or fully
    /// follows (and re-raises quiescing) — never observing a half-applied abort.
    /// The generation counter is NOT reset, so a subsequent request keeps
    /// climbing.
    pub fn abort(&self) {
        let mut inner = self
            .inner
            .lock()
            .expect("restart coordinator mutex poisoned");
        inner.in_progress = None;
        // Lower quiescing under the same lock that request() raises it under.
        self.quiescing.store(false, Ordering::SeqCst);
    }

    /// The in-flight request, if any (ADR-008 P5-L L8c).
    ///
    /// Read by `app.rs` at supervisor-driven terminate time to decide whether to
    /// persist a handover record (a plain Oneshot self-terminate has no pending
    /// request → no record).
    pub fn pending(&self) -> Option<RestartRequest> {
        self.inner
            .lock()
            .expect("restart coordinator mutex poisoned")
            .in_progress
    }

    /// Whether a controlled restart is draining (ADR-008 P5-L L8c).
    ///
    /// Lock-free read of the shared `quiescing` flag — parity with the L8b
    /// admission-gate predicate and the supervisor's drain-arm trigger.
    pub fn is_quiescing(&self) -> bool {
        self.quiescing.load(Ordering::SeqCst)
    }
}

impl Default for RestartCoordinator {
    /// Build a coordinator with a fresh, independent `quiescing` flag — for
    /// assembly seams / tests that do not need to share the flag with a
    /// `DaemonApiState`.
    fn default() -> Self {
        Self::new(Arc::new(AtomicBool::new(false)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_coordinator_is_not_quiescing_and_has_no_pending() {
        let coord = RestartCoordinator::default();
        assert!(!coord.is_quiescing());
        assert!(coord.pending().is_none());
    }

    #[test]
    fn first_request_is_accepted_and_raises_quiescing() {
        let coord = RestartCoordinator::default();

        let outcome = coord.request(DaemonResidency::Standalone);
        assert_eq!(outcome, RestartOutcome::Accepted { generation: 1 });
        assert!(coord.is_quiescing());
        assert_eq!(
            coord.pending(),
            Some(RestartRequest {
                target: DaemonResidency::Standalone,
                generation: 1,
            })
        );
    }

    #[test]
    fn second_request_conflicts_with_locked_in_target() {
        let coord = RestartCoordinator::default();
        coord.request(DaemonResidency::Standalone);

        // A second request — even for a different target — loses to the first.
        let outcome = coord.request(DaemonResidency::ServerHeadless);
        assert_eq!(
            outcome,
            RestartOutcome::Conflict {
                current_target: DaemonResidency::Standalone,
                generation: 1,
            }
        );
        // Conflict must not disturb the locked-in request or the flag.
        assert!(coord.is_quiescing());
        assert_eq!(
            coord.pending(),
            Some(RestartRequest {
                target: DaemonResidency::Standalone,
                generation: 1,
            })
        );
    }

    #[test]
    fn abort_clears_pending_and_lowers_quiescing() {
        let coord = RestartCoordinator::default();
        coord.request(DaemonResidency::Standalone);

        coord.abort();
        assert!(coord.pending().is_none());
        assert!(!coord.is_quiescing());
    }

    #[test]
    fn generation_is_monotonic_across_abort() {
        let coord = RestartCoordinator::default();
        assert_eq!(
            coord.request(DaemonResidency::Standalone),
            RestartOutcome::Accepted { generation: 1 }
        );

        coord.abort();

        // The next accepted request must NOT reuse generation 1 — the counter
        // keeps climbing so a successor can totally-order handover records.
        assert_eq!(
            coord.request(DaemonResidency::ServerHeadless),
            RestartOutcome::Accepted { generation: 2 }
        );
        assert!(coord.is_quiescing());
        assert_eq!(
            coord.pending(),
            Some(RestartRequest {
                target: DaemonResidency::ServerHeadless,
                generation: 2,
            })
        );
    }

    #[test]
    fn is_quiescing_tracks_request_and_abort() {
        let coord = RestartCoordinator::default();
        assert!(!coord.is_quiescing());

        coord.request(DaemonResidency::Standalone);
        assert!(coord.is_quiescing());

        coord.abort();
        assert!(!coord.is_quiescing());
    }

    #[test]
    fn clone_shares_state_and_flag() {
        // Proves a `DaemonApiState` clone and the supervisor observe the same
        // arbitration state + the same Arc-backed flag.
        let coord = RestartCoordinator::default();
        let cloned = coord.clone();

        coord.request(DaemonResidency::Standalone);
        assert!(cloned.is_quiescing());
        assert_eq!(
            cloned.pending(),
            Some(RestartRequest {
                target: DaemonResidency::Standalone,
                generation: 1,
            })
        );

        cloned.abort();
        assert!(!coord.is_quiescing());
        assert!(coord.pending().is_none());
    }
}
