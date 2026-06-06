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
//! - **Terminate condition**: `active == 0 && (armed || grace_expired)`.
//!   - During grace, never-armed, 0 active → do NOT terminate (wait for the
//!     spawning command to connect).
//!   - Armed, then active→0 → terminate (the command finished).
//!   - Grace expires still never-armed → terminate (hard reclaim: the spawning
//!     CLI died before opening its control WS).

use std::time::Duration;

use tokio_util::sync::CancellationToken;
use tracing::debug;
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

/// Drive the Oneshot self-termination state machine (ADR-008 P5-L L4).
///
/// Polls the control-WS lease registry and fires `terminate` once the leases
/// drain under the [module](self) state machine. Returns early without firing
/// `terminate` if `shutdown` is cancelled first (the daemon is already shutting
/// down for another reason — OS signal, crash — so there is nothing to trigger).
///
/// `grace` / `poll_interval` are parameters (not the consts directly) purely so
/// the unit tests can drive a deterministic paused clock; production always
/// passes [`ONESHOT_NO_CLIENT_GRACE`] / [`LEASE_POLL_INTERVAL`].
pub(crate) async fn run_oneshot_self_terminate_supervisor(
    lease_registry: ControlLeaseRegistry,
    terminate: CancellationToken,
    shutdown: CancellationToken,
    grace: Duration,
    poll_interval: Duration,
) {
    // Pinned grace deadline, armed once from supervisor start. `biased` select
    // below polls it before the poll-interval sleep so the grace transition is
    // observed promptly the moment it fires.
    let grace_deadline = tokio::time::sleep(grace);
    tokio::pin!(grace_deadline);
    let mut grace_expired = false;

    loop {
        // Read `armed` (total_acquired) BEFORE `active`. This pairs with
        // `ControlLeaseRegistry::acquire` incrementing `active` before `next_id`
        // (both SeqCst): any lease counted in `total_acquired` has already bumped
        // `active`, so we can never read `armed && active==0` for a still-live
        // connection — closing the TOCTOU window on the first-ever connect.
        let armed = lease_registry.total_acquired() > 0;
        let active = lease_registry.active_leases();

        if active == 0 && (armed || grace_expired) {
            debug!(
                armed,
                grace_expired, "oneshot residency: lease drained — firing self-terminate"
            );
            terminate.cancel();
            return;
        }

        tokio::select! {
            biased;
            // Shutdown for another reason wins: bail without firing terminate.
            _ = shutdown.cancelled() => return,
            // Grace boundary: flip the latch, re-evaluate on the next loop turn.
            _ = &mut grace_deadline, if !grace_expired => {
                grace_expired = true;
            }
            // Otherwise re-poll the lease count after the interval.
            _ = tokio::time::sleep(poll_interval) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Short, distinct test durations so the paused-clock advances below are
    // unambiguous. `start_paused = true` means time only moves on explicit
    // `tokio::time::advance` — never on the real wallclock.
    const TEST_GRACE: Duration = Duration::from_secs(5);
    const TEST_POLL: Duration = Duration::from_millis(250);

    /// Spawn the supervisor against a fresh registry, returning the registry,
    /// the terminate token, the shutdown token, and the join handle so each test
    /// can drive leases + clock and then assert on `terminate.is_cancelled()`.
    fn spawn_supervisor() -> (
        ControlLeaseRegistry,
        CancellationToken,
        CancellationToken,
        tokio::task::JoinHandle<()>,
    ) {
        let registry = ControlLeaseRegistry::new();
        let terminate = CancellationToken::new();
        let shutdown = CancellationToken::new();
        let handle = tokio::spawn(run_oneshot_self_terminate_supervisor(
            registry.clone(),
            terminate.clone(),
            shutdown.clone(),
            TEST_GRACE,
            TEST_POLL,
        ));
        (registry, terminate, shutdown, handle)
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
        let (registry, terminate, _shutdown, handle) = spawn_supervisor();

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
        let (registry, terminate, _shutdown, handle) = spawn_supervisor();

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
        let (_registry, terminate, _shutdown, handle) = spawn_supervisor();

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
        let (_registry, terminate, _shutdown, handle) = spawn_supervisor();

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
        let (registry, terminate, _shutdown, handle) = spawn_supervisor();

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
        let (_registry, terminate, shutdown, handle) = spawn_supervisor();

        settle().await;
        shutdown.cancel();
        settle().await;

        handle.await.expect("supervisor task must complete cleanly");
        assert!(
            !terminate.is_cancelled(),
            "external shutdown must NOT fire the self-terminate token"
        );
    }
}
