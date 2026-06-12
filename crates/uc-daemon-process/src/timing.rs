//! Cross-process timing contract for the daemon stop → start handoff.
//!
//! Three independently shipping actors participate in a daemon restart — the
//! exiting daemon, its replacement, and the spawner (CLI/GUI shell) — and
//! their wait budgets are coupled: each actor's budget must cover the phases
//! of the actor it waits on. Those relations used to live only in comments
//! spread across crates ("must exceed the daemon-side drain window",
//! "comfortably covers the two 5s shutdown joins"), which meant changing one
//! base duration silently invalidated budgets in files the editor never
//! opened (this is exactly how the 2026-06-12 restart race shipped). This
//! module makes the relations load-bearing: base durations are named once,
//! every dependent budget is DERIVED, and changing a base moves the whole
//! chain.
//!
//! Dependency chain (top feeds bottom):
//!
//! ```text
//! exiting daemon teardown    = 2 × SHUTDOWN_JOIN_TIMEOUT + iroh close
//! worst graceful teardown    = PREDECESSOR_RELEASE_BUDGET  (covers ↑)
//! replacement's lock wait    = LOCK_ACQUIRE_DEADLINE       (covers ↑, hang protection)
//! spawner's startup wait     = DAEMON_STARTUP_TIMEOUT      (covers ↑ + bootstrap)
//! spawner's promote wait     = PROMOTE_DRAIN_TIMEOUT       (covers drain + teardown)
//! ```

use std::time::Duration;

const fn sum(a: Duration, b: Duration) -> Duration {
    Duration::from_millis(a.as_millis() as u64 + b.as_millis() as u64)
}

const fn double(d: Duration) -> Duration {
    Duration::from_millis(2 * d.as_millis() as u64)
}

/// Exiting-daemon-side: bound on EACH of the two teardown joins in the daemon
/// shutdown path (the service `JoinSet` drain, then the HTTP server task
/// join) — see `DaemonApp::run` in `uc-daemon`.
pub const SHUTDOWN_JOIN_TIMEOUT: Duration = Duration::from_secs(5);

/// Exiting-daemon-side: slack for the iroh `endpoint.close()` teardown that
/// runs AFTER the daemon main loop returns and BEFORE the instance lock is
/// released. The bound is iroh-internal (not ours to derive from), hence a
/// margin: the 2026-06-12 production log shows iroh's own abort firing ~400ms
/// after the main loop stopped, so 5s is comfortable.
pub const IROH_TEARDOWN_MARGIN: Duration = Duration::from_secs(5);

/// Replacement-daemon-side: budget to wait for an exiting predecessor to
/// release the per-profile instance lock.
///
/// The predecessor's `/health` goes absent BEFORE its lock release (HTTP
/// server cancelled first, lock dropped only after iroh unbinds), so a
/// health-probing spawner can launch the replacement while the lock is still
/// held. The replacement must therefore ride out the predecessor's worst-case
/// graceful teardown: two shutdown joins plus the iroh close margin.
pub const PREDECESSOR_RELEASE_BUDGET: Duration =
    sum(double(SHUTDOWN_JOIN_TIMEOUT), IROH_TEARDOWN_MARGIN);

/// Replacement-daemon-side: hard deadline for the event-driven (blocking
/// `flock`) wait on a predecessor's lock release.
///
/// The kernel wakes the waiter the instant the holder releases, so unlike a
/// polling budget this deadline is NOT a tuned estimate of the predecessor's
/// teardown — it is pure protection against a hung predecessor that never
/// exits. Floor: it must exceed [`PREDECESSOR_RELEASE_BUDGET`] (the
/// worst-case GRACEFUL teardown); doubled for slack, since a longer deadline
/// costs nothing when the holder behaves.
pub const LOCK_ACQUIRE_DEADLINE: Duration = double(PREDECESSOR_RELEASE_BUDGET);

/// Spawner-side: how long a fresh daemon may take from process spawn to
/// `/health` answering, EXCLUDING any wait on a predecessor's lock (DB
/// migrations, secure storage, iroh bind, HTTP bind).
pub const DAEMON_BOOTSTRAP_ALLOWANCE: Duration = Duration::from_secs(15);

/// Spawner-side: total budget to wait for a spawned daemon to become healthy.
///
/// Must cover the replacement's worst case: waiting out a predecessor's lock
/// release (up to its full deadline) AND THEN bootstrapping from scratch.
pub const DAEMON_STARTUP_TIMEOUT: Duration = sum(LOCK_ACQUIRE_DEADLINE, DAEMON_BOOTSTRAP_ALLOWANCE);

/// Exiting-daemon-side: bounded wait for control leases to drain during a
/// controlled restart (ADR-008 P5-L L8b). If leases do not drain within this
/// window the supervisor ABORTS the restart (clears quiescing, keeps running)
/// rather than force-killing in-flight work (R8-F3).
pub const CONTROLLED_RESTART_DRAIN_TIMEOUT: Duration = Duration::from_secs(30);

/// Spawner-side: how long to wait for a controlled-restart predecessor's
/// endpoint to go absent before declaring the promotion failed (ADR-008 P5-L
/// L8d-2).
///
/// `POST /lifecycle/restart` returns immediately, but the predecessor keeps
/// `/health` UP for the entire bounded drain and only then cancels — followed
/// by its normal teardown. Doubling the drain window leaves headroom for that
/// teardown so a legitimately slow drain does not hard-fail the promotion
/// before the old daemon exits.
pub const PROMOTE_DRAIN_TIMEOUT: Duration = double(CONTROLLED_RESTART_DRAIN_TIMEOUT);

#[cfg(test)]
mod tests {
    use super::*;

    /// Pin the derived values so a change to a base constant shows up in a
    /// test diff (reviewers see the whole chain move, not just one number).
    #[test]
    fn derived_budgets_cover_their_dependency() {
        assert_eq!(PREDECESSOR_RELEASE_BUDGET, Duration::from_secs(15));
        assert_eq!(LOCK_ACQUIRE_DEADLINE, Duration::from_secs(30));
        assert_eq!(DAEMON_STARTUP_TIMEOUT, Duration::from_secs(45));
        assert_eq!(PROMOTE_DRAIN_TIMEOUT, Duration::from_secs(60));

        // The relations themselves, independent of the concrete numbers.
        assert!(PREDECESSOR_RELEASE_BUDGET >= double(SHUTDOWN_JOIN_TIMEOUT));
        assert!(LOCK_ACQUIRE_DEADLINE > PREDECESSOR_RELEASE_BUDGET);
        assert!(DAEMON_STARTUP_TIMEOUT > LOCK_ACQUIRE_DEADLINE);
        assert!(PROMOTE_DRAIN_TIMEOUT > CONTROLLED_RESTART_DRAIN_TIMEOUT);
    }
}
