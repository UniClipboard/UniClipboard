//! Oneshot self-termination state machine (ADR-008 P5-L L4).
//!
//! When the daemon runs in `Oneshot` residency it is a transient command-runner:
//! some CLI command spawned it, will open ONE control WebSocket, do its work,
//! then disconnect. Once that lease drains the process has nothing left to do,
//! so it must self-terminate rather than linger as a stray daemon.
//!
//! This supervisor is the trigger. It is **residency-agnostic by construction**
//! — it only ever runs because `DaemonApp::run` chooses to spawn it for the
//! `Oneshot` residency (see `app.rs`). For Standalone / ServerHeadless no
//! supervisor is spawned and the self-terminate run-loop arm is wired to
//! `pending`, so this module is production-behaviour-neutral until a later slice
//! (L8) actually spawns an Oneshot daemon.
//!
//! State machine (only ever reached in Oneshot):
//!
//! - **Startup grace window** [`ONESHOT_NO_CLIENT_GRACE`], measured from
//!   supervisor start (≈ serving-ready). During grace, a never-armed daemon with
//!   zero active leases keeps waiting — the cold-start case where the spawning
//!   command has not yet opened its control WS.
//! - **"armed" latch** = a lease was EVER acquired
//!   ([`ControlLeaseRegistry::total_acquired`]` > 0`). Monotonic, so it survives a
//!   0→1→0 blip inside a single poll: once a client has connected we treat the
//!   daemon as having served, even if the lease already drained again. The
//!   supervisor reads `total_acquired` BEFORE `active_leases`, which (paired with
//!   `acquire` incrementing `active` first) makes `armed && active==0` an
//!   impossible read for a live connection — no spurious first-connect terminate.
//! - **Terminate condition**: `active == 0 && (quiescing || armed || grace_expired)`.
//!   - During grace, never-armed, 0 active → do NOT terminate (wait for the
//!     spawning command to connect).
//!   - Armed, then active→0 → terminate (the command finished).
//!   - Grace expires still never-armed → terminate (hard reclaim: the spawning
//!     CLI died before opening its control WS).
//!
//! # Controlled-restart drain (ADR-008 P5-L L8b)
//!
//! The same supervisor ALSO drives the controlled-restart drain. When the shared
//! `quiescing` flag is set (false→true edge), the supervisor arms a bounded
//! [`CONTROLLED_RESTART_DRAIN_TIMEOUT`] deadline and waits for in-flight leases to
//! drain (`active → 0`), then fires `terminate` so the successor daemon can take
//! over. `quiescing` makes terminate fire even on a never-armed / in-grace daemon
//! (an explicit restart overrides grace). If leases do NOT drain within the
//! timeout the supervisor ABORTS the restart — it clears `quiescing` and keeps
//! running rather than force-killing in-flight work (R8-F3), falling back to the
//! ordinary L4 self-terminate behaviour. L8b adds NO setter for `quiescing` (the
//! restart control plane that flips it is a later slice L8c), so this drain path
//! is unreachable in production — and the whole supervisor is still Oneshot-only,
//! so it is doubly production-behaviour-neutral.

use std::sync::atomic::Ordering;
use std::time::Duration;

use tokio_util::sync::CancellationToken;
use tracing::{debug, warn};
use uc_webserver::api::control_lease::ControlLeaseRegistry;

/// Startup grace window for a never-armed Oneshot daemon (ADR-008 P5-L L4).
///
/// Measured from supervisor start (≈ serving-ready). A never-armed daemon with
/// zero active leases waits out this window for the spawning command to connect;
/// if the window expires still never-armed, the daemon hard-reclaims (the CLI
/// died before opening its control WS).
pub(crate) const ONESHOT_NO_CLIENT_GRACE: Duration = Duration::from_secs(5);

/// How often the supervisor re-checks the lease count (ADR-008 P5-L L4).
///
/// The lease count is an in-process atomic with no change-notification, so the
/// supervisor polls. The interval trades self-terminate latency against idle
/// wakeups; 250ms keeps post-disconnect shutdown snappy without busy-looping.
pub(crate) const LEASE_POLL_INTERVAL: Duration = Duration::from_millis(250);

/// Bounded wait for control leases to drain during a controlled restart
/// (ADR-008 P5-L L8b). If leases do not drain within this window the supervisor
/// ABORTS the restart (clears quiescing, keeps running) rather than force-killing
/// in-flight work (R8-F3). Tunable; L8d end-to-end will validate the value.
pub(crate) const CONTROLLED_RESTART_DRAIN_TIMEOUT: Duration = Duration::from_secs(30);

/// Tunable timings for the Oneshot lifecycle supervisor (grace / drain / poll).
/// Bundled so the supervisor signature stays small; production uses
/// [`SupervisorTimings::production`], tests pass short paused-clock durations.
pub(crate) struct SupervisorTimings {
    pub(crate) grace: Duration,
    pub(crate) drain_timeout: Duration,
    pub(crate) poll_interval: Duration,
}

impl SupervisorTimings {
    pub(crate) const fn production() -> Self {
        Self {
            grace: ONESHOT_NO_CLIENT_GRACE,
            drain_timeout: CONTROLLED_RESTART_DRAIN_TIMEOUT,
            poll_interval: LEASE_POLL_INTERVAL,
        }
    }
}

/// Drive the Oneshot self-termination state machine (ADR-008 P5-L L4) AND the
/// controlled-restart drain (ADR-008 P5-L L8b).
///
/// Polls the control-WS lease registry and fires `terminate` once the leases
/// drain under the [module](self) state machine. Returns early without firing
/// `terminate` if `shutdown` is cancelled first (the daemon is already shutting
/// down for another reason — OS signal, crash — so there is nothing to trigger).
///
/// `quiescing` is the shared controlled-restart flag (L8b): on the false→true
/// edge the supervisor arms a bounded drain deadline ([`SupervisorTimings::drain_timeout`])
/// and terminates once leases drain — even on a never-armed / in-grace daemon. If
/// the deadline fires while leases are still held it ABORTS the restart: clears
/// `quiescing`, keeps running, and falls back to ordinary L4 behaviour (R8-F3).
///
/// `timings` are parameters (not the consts directly) purely so the unit tests can
/// drive a deterministic paused clock; production passes
/// [`SupervisorTimings::production`].
pub(crate) async fn run_oneshot_self_terminate_supervisor(
    lease_registry: ControlLeaseRegistry,
    quiescing: std::sync::Arc<std::sync::atomic::AtomicBool>,
    terminate: CancellationToken,
    shutdown: CancellationToken,
    timings: SupervisorTimings,
) {
    // Pinned grace deadline, armed once from supervisor start. `biased` select
    // below polls it before the poll-interval sleep so the grace transition is
    // observed promptly the moment it fires.
    let grace_deadline = tokio::time::sleep(timings.grace);
    tokio::pin!(grace_deadline);
    let mut grace_expired = false;

    // Controlled-restart drain deadline (ADR-008 P5-L L8b). `reset` to
    // `now + drain_timeout` on the quiescing false→true edge; the `draining` flag
    // arms/disarms it (set on false→true, cleared on true→false), so the
    // `select!` arm below only fires while a drain is actually in progress.
    let drain_deadline = tokio::time::sleep(timings.drain_timeout);
    tokio::pin!(drain_deadline);
    let mut draining = false;
    let mut prev_quiescing = false;

    loop {
        // Read `armed` (total_acquired) BEFORE `active`. This pairs with
        // `ControlLeaseRegistry::acquire` incrementing `active` before `next_id`
        // (both SeqCst): any lease counted in `total_acquired` has already bumped
        // `active`, so we can never read `armed && active==0` for a still-live
        // connection — closing the TOCTOU window on the first-ever connect.
        let armed = lease_registry.total_acquired() > 0;
        let active = lease_registry.active_leases();
        // Independent of the armed/active read order (L8b): the quiescing flag is
        // a separate atomic, not part of the L4 TOCTOU pairing.
        let quiescing_now = quiescing.load(Ordering::SeqCst);

        // Arm / disarm the drain deadline on the quiescing edge.
        if quiescing_now && !prev_quiescing {
            // false→true: a controlled restart began — arm the bounded drain.
            drain_deadline
                .as_mut()
                .reset(tokio::time::Instant::now() + timings.drain_timeout);
            draining = true;
            debug!("controlled-restart drain armed — waiting for leases to quiesce");
        } else if !quiescing_now && prev_quiescing {
            // true→false: restart aborted/cleared externally — disarm.
            draining = false;
        }
        prev_quiescing = quiescing_now;

        if active == 0 && (quiescing_now || armed || grace_expired) {
            debug!(
                armed,
                grace_expired,
                quiescing = quiescing_now,
                "oneshot residency: lease drained — firing self-terminate"
            );
            terminate.cancel();
            return;
        }

        tokio::select! {
            biased;
            // Shutdown for another reason wins: bail without firing terminate.
            _ = shutdown.cancelled() => return,
            // Drain timeout (L8b): leases did not quiesce in time — ABORT the
            // restart instead of force-killing in-flight work. Clear quiescing
            // and fall back to ordinary L4 behaviour on the next turn.
            _ = &mut drain_deadline, if draining => {
                // Disarm UNCONDITIONALLY first: this is a one-shot `Sleep`, so once
                // elapsed it is permanently ready; leaving `draining` set would re-
                // select this arm every loop turn and busy-spin the supervisor. The
                // unconditional clear keeps that safe even if a future change broke
                // the loop-top invariant the guard below relies on.
                draining = false;
                // By that invariant (draining ⟹ quiescing && active>0: a drained or
                // de-quiesced daemon self-terminates / disarms at the loop top before
                // reaching this select), the guard is always true here — it is
                // defensive belt-and-suspenders, only aborting a restart that is
                // genuinely still draining held leases. The reads are the (slightly
                // stale) loop-top snapshot; worst case a lease that drained in this
                // exact tick yields a no-op abort that L4 self-terminate reclaims on
                // the next turn.
                if quiescing_now && active > 0 {
                    warn!(
                        "controlled-restart drain timed out — aborting restart, daemon stays up"
                    );
                    quiescing.store(false, Ordering::SeqCst);
                }
            }
            // Grace boundary: flip the latch, re-evaluate on the next loop turn.
            _ = &mut grace_deadline, if !grace_expired => {
                grace_expired = true;
            }
            // Otherwise re-poll the lease count after the interval.
            _ = tokio::time::sleep(timings.poll_interval) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;

    // Short, distinct test durations so the paused-clock advances below are
    // unambiguous. `start_paused = true` means time only moves on explicit
    // `tokio::time::advance` — never on the real wallclock.
    const TEST_GRACE: Duration = Duration::from_secs(5);
    const TEST_POLL: Duration = Duration::from_millis(250);
    // Distinct from grace so a drain-timeout advance can never be confused with a
    // grace-expiry advance in the L8b tests.
    const TEST_DRAIN: Duration = Duration::from_secs(10);

    /// Spawn the supervisor against a fresh registry, returning the registry, the
    /// shared quiescing flag, the terminate token, the shutdown token, and the
    /// join handle so each test can drive leases + the quiescing flag + clock and
    /// then assert on `terminate.is_cancelled()`.
    fn spawn_supervisor() -> (
        ControlLeaseRegistry,
        Arc<AtomicBool>,
        CancellationToken,
        CancellationToken,
        tokio::task::JoinHandle<()>,
    ) {
        let registry = ControlLeaseRegistry::new();
        let quiescing = Arc::new(AtomicBool::new(false));
        let terminate = CancellationToken::new();
        let shutdown = CancellationToken::new();
        let handle = tokio::spawn(run_oneshot_self_terminate_supervisor(
            registry.clone(),
            quiescing.clone(),
            terminate.clone(),
            shutdown.clone(),
            SupervisorTimings {
                grace: TEST_GRACE,
                drain_timeout: TEST_DRAIN,
                poll_interval: TEST_POLL,
            },
        ));
        (registry, quiescing, terminate, shutdown, handle)
    }

    /// Yield to the runtime so the spawned supervisor task makes progress past
    /// the current `await` point before the test inspects state.
    async fn settle() {
        tokio::task::yield_now().await;
        tokio::task::yield_now().await;
    }

    #[tokio::test(start_paused = true)]
    async fn armed_then_drained_terminates() {
        // A client connects (arms the latch), then disconnects. With 0 active
        // leases AND armed, the supervisor must self-terminate — even well
        // inside the grace window.
        let (registry, _quiescing, terminate, _shutdown, handle) = spawn_supervisor();

        let lease = registry.acquire();
        settle().await;
        assert!(
            !terminate.is_cancelled(),
            "must not terminate while a lease is held"
        );

        drop(lease);
        // Advance one poll so the supervisor re-evaluates and sees active==0.
        tokio::time::advance(TEST_POLL).await;
        settle().await;

        assert!(
            terminate.is_cancelled(),
            "armed + drained must fire self-terminate"
        );
        handle.await.expect("supervisor task must complete cleanly");
    }

    #[tokio::test(start_paused = true)]
    async fn armed_within_grace_terminates_before_grace_expiry() {
        // A client connects AND disconnects entirely inside the grace window. The
        // monotonic `total_acquired` latch keeps `armed` true, so the supervisor
        // must self-terminate on the next poll — WITHOUT waiting for grace to
        // expire. Proves the armed-during-grace path at the supervisor level (not
        // just the registry-level `total_acquired` latch test), and exercises the
        // `total_acquired`-before-`active_leases` read order.
        let (registry, _quiescing, terminate, _shutdown, handle) = spawn_supervisor();

        // Arm + drain while the supervisor is still parked on its first poll, far
        // short of the grace window.
        let lease = registry.acquire();
        settle().await;
        drop(lease);
        settle().await;

        // Advance a SINGLE poll (TEST_POLL << TEST_GRACE): terminate must fire.
        tokio::time::advance(TEST_POLL).await;
        settle().await;
        assert!(
            terminate.is_cancelled(),
            "armed-then-drained inside grace must terminate on the next poll, \
             not wait for grace expiry"
        );
        handle.await.expect("supervisor task must complete cleanly");
    }

    #[tokio::test(start_paused = true)]
    async fn hard_reclaim_when_grace_expires_never_armed() {
        // The spawning CLI died before ever opening its control WS: never armed,
        // 0 active. Once the grace window expires the supervisor must hard-reclaim.
        let (_registry, _quiescing, terminate, _shutdown, handle) = spawn_supervisor();

        settle().await;
        assert!(
            !terminate.is_cancelled(),
            "never-armed daemon must wait out the grace window"
        );

        // Cross the grace boundary; the biased select sees the deadline first,
        // flips grace_expired, and the next loop turn terminates.
        tokio::time::advance(TEST_GRACE).await;
        settle().await;

        assert!(
            terminate.is_cancelled(),
            "grace expiry on a never-armed daemon must hard-reclaim"
        );
        handle.await.expect("supervisor task must complete cleanly");
    }

    #[tokio::test(start_paused = true)]
    async fn no_terminate_during_grace_without_lease() {
        // Cold start: never armed, 0 active, still inside the grace window.
        // The supervisor must keep waiting (do NOT terminate).
        let (_registry, _quiescing, terminate, _shutdown, handle) = spawn_supervisor();

        // Advance most of the way through the grace window — but not past it.
        tokio::time::advance(TEST_GRACE - Duration::from_millis(1)).await;
        settle().await;

        assert!(
            !terminate.is_cancelled(),
            "never-armed daemon must not terminate before the grace window expires"
        );

        // Clean up: push past grace so the task ends, then join.
        tokio::time::advance(Duration::from_millis(1)).await;
        settle().await;
        handle.await.expect("supervisor task must complete cleanly");
    }

    #[tokio::test(start_paused = true)]
    async fn no_terminate_while_lease_held() {
        // A client connects and stays connected past the grace window. As long
        // as the lease is held (active > 0), the supervisor must never terminate.
        let (registry, _quiescing, terminate, _shutdown, handle) = spawn_supervisor();

        let lease = registry.acquire();
        settle().await;

        // Run well past the grace window with the lease still held.
        tokio::time::advance(TEST_GRACE + TEST_POLL * 4).await;
        settle().await;

        assert!(
            !terminate.is_cancelled(),
            "must never terminate while a lease is held, even past the grace window"
        );

        // Now drop it and confirm the supervisor is still live and terminates.
        drop(lease);
        tokio::time::advance(TEST_POLL).await;
        settle().await;
        assert!(
            terminate.is_cancelled(),
            "dropping the held lease past grace must finally self-terminate"
        );
        handle.await.expect("supervisor task must complete cleanly");
    }

    #[tokio::test(start_paused = true)]
    async fn external_shutdown_returns_without_terminating() {
        // The daemon is shutting down for another reason (OS signal / crash):
        // the supervisor must bail out WITHOUT firing the self-terminate token.
        //
        // This exercises the realistic "shutdown while NOT terminate-eligible"
        // path (here: in-grace, never armed). The loop-top terminate check runs
        // BEFORE the select, so once the daemon IS terminate-eligible the
        // self-terminate fires immediately and the shutdown arm is never reached;
        // the shutdown arm therefore only matters precisely when terminate is not
        // yet eligible — exactly the state this test sets up.
        let (_registry, _quiescing, terminate, shutdown, handle) = spawn_supervisor();

        settle().await;
        shutdown.cancel();
        settle().await;

        handle.await.expect("supervisor task must complete cleanly");
        assert!(
            !terminate.is_cancelled(),
            "external shutdown must NOT fire the self-terminate token"
        );
    }

    // ── ADR-008 P5-L L8b: controlled-restart drain ──────────────────────────

    #[tokio::test(start_paused = true)]
    async fn quiescing_with_no_active_leases_terminates_on_next_poll() {
        // A controlled restart begins with 0 active leases (drain already
        // complete): the supervisor must terminate on the next poll. quiescing
        // forces terminate even though the daemon was never armed and is still in
        // grace.
        let (_registry, quiescing, terminate, _shutdown, handle) = spawn_supervisor();

        settle().await;
        assert!(
            !terminate.is_cancelled(),
            "must not terminate before quiescing is set"
        );

        quiescing.store(true, Ordering::SeqCst);
        // Advance one poll so the supervisor re-evaluates with quiescing set.
        tokio::time::advance(TEST_POLL).await;
        settle().await;

        assert!(
            terminate.is_cancelled(),
            "quiescing + 0 active leases must self-terminate on the next poll"
        );
        handle.await.expect("supervisor task must complete cleanly");
    }

    #[tokio::test(start_paused = true)]
    async fn quiescing_drains_held_lease_within_timeout_then_terminates() {
        // A controlled restart begins while a lease is held; the lease is dropped
        // well within the drain timeout. The supervisor must wait for the drain
        // and then terminate.
        let (registry, quiescing, terminate, _shutdown, handle) = spawn_supervisor();

        let lease = registry.acquire();
        settle().await;

        quiescing.store(true, Ordering::SeqCst);
        // One poll: quiescing is set but the lease is still held → no terminate.
        tokio::time::advance(TEST_POLL).await;
        settle().await;
        assert!(
            !terminate.is_cancelled(),
            "must not terminate while a lease is still held, even while quiescing"
        );

        // Drop the lease comfortably before the drain timeout, then advance a poll.
        drop(lease);
        tokio::time::advance(TEST_POLL).await;
        settle().await;
        assert!(
            terminate.is_cancelled(),
            "quiescing + lease drained within timeout must self-terminate"
        );
        handle.await.expect("supervisor task must complete cleanly");
    }

    #[tokio::test(start_paused = true)]
    async fn quiescing_drain_timeout_aborts_restart_then_falls_back_to_l4() {
        // A controlled restart begins while a lease is held and the lease is kept
        // PAST the drain timeout. The supervisor must ABORT: terminate not fired,
        // quiescing cleared back to false, supervisor still live. Then dropping
        // the (now-armed) lease must still self-terminate via the ordinary L4
        // path — proving the fallback.
        let (registry, quiescing, terminate, _shutdown, handle) = spawn_supervisor();

        let lease = registry.acquire();
        settle().await;

        quiescing.store(true, Ordering::SeqCst);
        // Let the supervisor observe the false→true edge and arm the drain.
        tokio::time::advance(TEST_POLL).await;
        settle().await;
        assert!(quiescing.load(Ordering::SeqCst), "quiescing should be set");

        // Hold the lease past the drain timeout: the drain deadline fires → abort.
        tokio::time::advance(TEST_DRAIN).await;
        settle().await;
        assert!(
            !terminate.is_cancelled(),
            "drain timeout with a held lease must NOT force-terminate"
        );
        assert!(
            !quiescing.load(Ordering::SeqCst),
            "drain timeout must clear quiescing (restart aborted)"
        );

        // Fallback: the daemon stays up under ordinary L4 behaviour. The lease is
        // armed, so dropping it must finally self-terminate.
        drop(lease);
        tokio::time::advance(TEST_POLL).await;
        settle().await;
        assert!(
            terminate.is_cancelled(),
            "after abort, dropping the armed lease must self-terminate via L4"
        );
        handle.await.expect("supervisor task must complete cleanly");
    }

    #[tokio::test(start_paused = true)]
    async fn quiescing_overrides_grace_on_never_armed_daemon() {
        // A controlled restart is requested on a never-armed daemon that is still
        // inside the grace window with 0 active leases. An explicit restart must
        // override grace and terminate on the next poll.
        let (_registry, quiescing, terminate, _shutdown, handle) = spawn_supervisor();

        // Stay well inside grace.
        tokio::time::advance(TEST_GRACE - Duration::from_millis(1)).await;
        settle().await;
        assert!(
            !terminate.is_cancelled(),
            "never-armed in-grace daemon must not terminate before quiescing"
        );

        quiescing.store(true, Ordering::SeqCst);
        tokio::time::advance(TEST_POLL).await;
        settle().await;
        assert!(
            terminate.is_cancelled(),
            "quiescing must override the grace window on a never-armed daemon"
        );
        handle.await.expect("supervisor task must complete cleanly");
    }
}
