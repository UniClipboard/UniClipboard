//! Relay self-healing watchdog.
//!
//! ## Why this exists
//!
//! iroh only rebuilds its DNS resolver + rebinds sockets when the OS network
//! monitor delivers a *major* link-change event (`socket.rs::handle_network_change`
//! → `dns_resolver.reset()`). On Windows the netmon route/address callbacks
//! push into a bounded channel with `try_send`; during a boot-time route storm
//! that channel fills and the notification is dropped (`netwatch` logs
//! `unable to send: route change notification: Full(..)`). If the daemon
//! captured an incomplete system DNS config at that moment, the endpoint's
//! resolver stays wedged: every relay hostname fails to resolve
//! (`Resolve failed, IPv4/IPv6`) and — since this node reaches peers over the
//! relay — every peer shows offline. Observed in the field as "opens offline,
//! a manual restart fixes it": restart rebuilds the resolver against the now-
//! healthy network, but nothing in-process ever did.
//!
//! ## What it does
//!
//! Watches [`Endpoint::home_relay_status`]. When the home relay stays
//! continuously unhealthy past a grace period, it calls
//! [`Endpoint::network_change`] — the public, documented, side-effect-free
//! nudge that runs the same major-link-change path a restart would
//! (`rebind` + `check_relay_connection` + `dns_resolver.reset()` + re-STUN).
//! In other words it re-delivers the notification the OS dropped, in-process,
//! so recovery no longer requires a user restart. Retries escalate with
//! exponential backoff and stop the moment the relay reconnects.
//!
//! This is a root-cause fix for the dropped-notification wedge, not a symptom
//! mask: calling `network_change()` when the network is genuinely down is
//! harmless (iroh's own docs say so), and the resolver reset is lazy (no IO
//! until the next lookup).
//!
//! ## Scope
//!
//! Only spawned when relays are enabled. In LAN-only mode
//! (`disable_relays = true`) there is no home relay by design, so the "no
//! healthy relay" signal is expected and this watchdog is not started.

use std::time::{Duration, Instant};

use iroh::{Endpoint, Watcher as _};
use tokio::task::JoinHandle;
use tracing::{info, warn};

/// Pacing for the relay self-healing watchdog. All values live here so the
/// recovery cadence is defined in one place rather than scattered as literals.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RecoveryPolicy {
    /// How long the home relay must be *continuously* unhealthy before the
    /// first nudge. Sized comfortably above a normal cold-start relay
    /// negotiation so ordinary startup churn never triggers a nudge, while
    /// still recovering the wedged state within ~a minute instead of never.
    grace: Duration,
    /// Delay after the first nudge before the next one, if still unhealthy.
    initial_backoff: Duration,
    /// Upper bound the backoff escalates to. Keeps a genuinely-offline node
    /// (relay really unreachable) probing at a low, steady rate.
    max_backoff: Duration,
}

impl Default for RecoveryPolicy {
    fn default() -> Self {
        Self {
            grace: Duration::from_secs(45),
            initial_backoff: Duration::from_secs(30),
            max_backoff: Duration::from_secs(300),
        }
    }
}

/// What the driver should do after observing the latest relay health.
#[derive(Debug, PartialEq, Eq)]
enum Action {
    /// Relay is healthy. Park until the next status change; no timer.
    Idle,
    /// Unhealthy but it is not yet time to act. Re-check after this delay
    /// (or sooner, if the status changes first).
    Wait(Duration),
    /// Nudge the endpoint now, then re-check after this delay.
    Nudge(Duration),
}

/// Backoff state machine for the watchdog. Pure and time-injectable so the
/// decision logic is unit-tested without a real endpoint or wall clock.
struct RecoveryState {
    policy: RecoveryPolicy,
    /// When the current unhealthy streak began; `None` while healthy.
    unhealthy_since: Option<Instant>,
    /// When we last nudged during the current unhealthy streak.
    last_nudge: Option<Instant>,
    /// Required gap between the most recent nudge and the next one. Starts at
    /// `initial_backoff` and doubles on each *subsequent* nudge up to the cap.
    next_gap: Duration,
}

impl RecoveryState {
    fn new(policy: RecoveryPolicy) -> Self {
        Self {
            policy,
            unhealthy_since: None,
            last_nudge: None,
            next_gap: policy.initial_backoff,
        }
    }

    /// Fold in the latest health observation and decide the next action.
    ///
    /// The [`Action::Nudge`] / [`Action::Wait`] duration is the delay until the
    /// next re-check, so a caller that sleeps exactly that long lands right on
    /// the next decision point (no redundant early wakeup).
    fn observe(&mut self, healthy: bool, now: Instant) -> Action {
        if healthy {
            // Recovered (or never broke): drop all streak state so the next
            // outage starts a fresh grace period and backoff ladder.
            self.unhealthy_since = None;
            self.last_nudge = None;
            self.next_gap = self.policy.initial_backoff;
            return Action::Idle;
        }

        let since = *self.unhealthy_since.get_or_insert(now);
        let unhealthy_for = now.saturating_duration_since(since);
        if unhealthy_for < self.policy.grace {
            return Action::Wait(self.policy.grace - unhealthy_for);
        }

        // Past the grace period: nudge on an escalating cadence. The first
        // nudge fires immediately; each later nudge waits `next_gap`, which
        // then doubles (capped). `next_gap` therefore always equals the delay
        // until the *following* nudge — returned so the caller sleeps exactly
        // that long.
        if let Some(last) = self.last_nudge {
            let since_nudge = now.saturating_duration_since(last);
            if since_nudge < self.next_gap {
                return Action::Wait(self.next_gap - since_nudge);
            }
            self.next_gap = (self.next_gap * 2).min(self.policy.max_backoff);
        }

        self.last_nudge = Some(now);
        Action::Nudge(self.next_gap)
    }
}

/// A home relay is "healthy" if at least one entry reports a live connection.
/// An empty set (no relay assigned yet, or all dropped) counts as unhealthy —
/// that is precisely the wedged state we recover from.
fn relay_healthy(statuses: &[iroh::endpoint::RelayStatus]) -> bool {
    statuses.iter().any(|s| s.is_connected())
}

/// Spawn the relay self-healing watchdog and return its [`JoinHandle`].
///
/// The caller (`IrohNode`) retains the handle and aborts + joins it during
/// shutdown, matching the daemon's "explicit handles, deterministic abort,
/// panics stay visible" worker convention rather than detaching the task.
/// [`Endpoint::closed`]'s `run_until` is layered on top purely as a safety
/// net: if the node is dropped without an orderly `shutdown()`, the task
/// still stops when the endpoint closes instead of leaking (it holds its own
/// `Endpoint` clone, so it would otherwise never see the watcher disconnect).
///
/// Call only when relays are enabled — see the module docs.
#[must_use]
pub(crate) fn spawn_net_recovery(endpoint: Endpoint) -> JoinHandle<()> {
    let closed = endpoint.closed();
    tokio::spawn(async move {
        // `run_until` yields `Some(())` on normal return, `None` if the
        // endpoint closed first; either way there is nothing to propagate.
        let _ = closed
            .run_until(run(endpoint, RecoveryPolicy::default()))
            .await;
    })
}

/// Driver loop: observe relay health, act, then wait for the next status
/// change or the re-check timer, whichever comes first.
async fn run(endpoint: Endpoint, policy: RecoveryPolicy) {
    let mut watcher = endpoint.home_relay_status();
    let mut state = RecoveryState::new(policy);
    info!(
        target: "iroh.net_recovery",
        grace_ms = policy.grace.as_millis() as u64,
        "relay self-healing watchdog started"
    );

    loop {
        let healthy = relay_healthy(&watcher.get());
        let wait = match state.observe(healthy, Instant::now()) {
            Action::Idle => None,
            Action::Wait(d) => Some(d),
            Action::Nudge(next) => {
                warn!(
                    target: "iroh.net_recovery",
                    next_recheck_ms = next.as_millis() as u64,
                    "home relay unhealthy past grace; nudging endpoint (network_change) to rebuild DNS resolver + relay connection",
                );
                endpoint.network_change().await;
                Some(next)
            }
        };

        tokio::select! {
            // Early wakeup: react to a status change (e.g. relay recovered)
            // without waiting out the timer. `Err` means every Endpoint clone
            // was dropped — nothing left to heal, so exit.
            changed = watcher.updated() => {
                if changed.is_err() {
                    break;
                }
            }
            // Re-check cadence while unhealthy. Healthy state parks here
            // forever (pending) and is only woken by a status change above.
            () = sleep_opt(wait) => {}
        }
    }

    info!(target: "iroh.net_recovery", "relay self-healing watchdog stopped");
}

/// `sleep(d)` for `Some`, an eternally-pending future for `None` — lets the
/// select park with no timer while the relay is healthy.
async fn sleep_opt(d: Option<Duration>) {
    match d {
        Some(d) => tokio::time::sleep(d).await,
        None => std::future::pending::<()>().await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> RecoveryPolicy {
        RecoveryPolicy {
            grace: Duration::from_secs(45),
            initial_backoff: Duration::from_secs(30),
            max_backoff: Duration::from_secs(300),
        }
    }

    const fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    #[test]
    fn healthy_is_idle_and_resets_streak() {
        let mut s = RecoveryState::new(policy());
        let t0 = Instant::now();
        // Build up unhealthy streak state...
        assert_eq!(s.observe(false, t0), Action::Wait(secs(45)));
        // ...then recovering clears it and parks.
        assert_eq!(s.observe(true, t0 + secs(10)), Action::Idle);
        assert!(s.unhealthy_since.is_none());
        assert!(s.last_nudge.is_none());
        assert_eq!(s.next_gap, secs(30));
    }

    #[test]
    fn waits_out_grace_before_first_nudge() {
        let mut s = RecoveryState::new(policy());
        let t0 = Instant::now();
        // First observation starts the streak.
        assert_eq!(s.observe(false, t0), Action::Wait(secs(45)));
        // Still inside grace: shorter remaining wait, no nudge yet.
        assert_eq!(s.observe(false, t0 + secs(30)), Action::Wait(secs(15)));
        // Grace elapsed: first nudge fires, hinting the initial gap.
        assert_eq!(s.observe(false, t0 + secs(45)), Action::Nudge(secs(30)));
    }

    #[test]
    fn backoff_escalates_and_caps() {
        let mut s = RecoveryState::new(policy());
        let t0 = Instant::now();
        // Seed the streak, then cross grace → first nudge (hint 30s).
        assert_eq!(s.observe(false, t0), Action::Wait(secs(45)));
        let mut now = t0 + secs(45);
        assert_eq!(s.observe(false, now), Action::Nudge(secs(30)));

        // Each later nudge waits the previous hint (`gap`) then doubles the
        // hint, capped at 300s: gaps 30→60→120→240→300, hints 60→…→300.
        for (gap, hint) in [(30, 60), (60, 120), (120, 240), (240, 300), (300, 300)] {
            // Mid-gap: not yet time to nudge again.
            assert!(matches!(s.observe(false, now + secs(1)), Action::Wait(_)));
            now += secs(gap);
            assert_eq!(
                s.observe(false, now),
                Action::Nudge(secs(hint)),
                "expected Nudge({hint}s) after a {gap}s gap",
            );
        }
    }

    #[test]
    fn recovery_after_nudges_resets_backoff_ladder() {
        let mut s = RecoveryState::new(policy());
        let t0 = Instant::now();
        assert_eq!(s.observe(false, t0), Action::Wait(secs(45)));
        let now = t0 + secs(45);
        assert_eq!(s.observe(false, now), Action::Nudge(secs(30)));
        // Relay comes back → state resets.
        assert_eq!(s.observe(true, now + secs(5)), Action::Idle);
        // A later outage starts a fresh grace period and backoff ladder.
        let t2 = now + secs(100);
        assert_eq!(s.observe(false, t2), Action::Wait(secs(45)));
        assert_eq!(s.observe(false, t2 + secs(45)), Action::Nudge(secs(30)));
    }
}
