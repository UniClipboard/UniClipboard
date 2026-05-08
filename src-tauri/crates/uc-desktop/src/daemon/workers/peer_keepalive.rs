//! Peer keepalive worker — periodically refresh presence so iroh's magicsock
//! NAT binding and path cache never go idle, with per-peer exponential
//! backoff so unreachable peers don't waste UDP probes every 25s.
//!
//! ## Why
//!
//! `IrohBlobTransferAdapter::fetch` opens a fresh QUIC connection to the
//! publisher every time a blob_ref comes in. When that peer hasn't been
//! dialed for ~60s the iroh endpoint's cached path has expired and the
//! connect attempt has to redo a full hole-punch + relay probe round. In
//! practice that takes ~33s and often terminates with `blob unavailable`.
//!
//! Refreshing presence on a short cadence keeps a warm `PRESENCE_ALPN`
//! connection alive per online peer, which in turn keeps the shared
//! magicsock layer warm so the BLOBS connection establishes on a hot path.
//!
//! ## Backoff design
//!
//! Without backoff every paired-but-offline peer eats a ~30s dial every
//! 25s — for a daemon paired with five idle peers that's nontrivial
//! cellular / battery cost. The scheduler maintains a per-peer
//! [`BackoffState`] and only dials peers whose `next_dial_at` has elapsed.
//! Ladder: 25s → 60s → 5min → 15min cap. Reset to 25s happens in two
//! cases:
//!
//! 1. Successful dial (Online) — the peer is healthy.
//! 2. **Inbound `Online` event** routed through `subscribe_peer_presence_events`.
//!    `IrohPresenceHandler` flips a peer to Online when the *peer* dials
//!    *us*, so a recovered offline peer's own keepalive announces them
//!    well before our backoff would have us redial. Without this reset
//!    the worst case for "peer comes back" is the cap (15min); with it,
//!    the peer's own 25s keepalive bounds the window.
//!
//! ## Design (worker mechanics)
//!
//! * Drives off a 25s base ticker (`MissedTickBehavior::Delay`) and an
//!   inbound `AppPresenceSubscription`. Both arms use `tokio::select!`
//!   with `biased` so cancellation and presence resets land before a
//!   ticker fires.
//! * Each tick: `list_paired_peer_device_ids` from the facade, prune
//!   removed peers from the backoff map, default-init new peers as ready,
//!   then dispatch `JoinSet` of `ensure_reachable_one` calls only for
//!   peers whose backoff has elapsed. Result of each dial updates that
//!   peer's `BackoffState`.
//! * `ensure_reachable_one` (vs `verify_reachable`): when our outbound
//!   connection map already holds a live entry for a peer, the fast-path
//!   returns `Online` without a fresh dial — exactly what the scheduler
//!   wants for healthy peers, since burning UDP on a peer that just
//!   answered would defeat the point of backoff.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use tokio::task::JoinSet;
use tokio::time::MissedTickBehavior;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use uc_application::facade::{
    AppFacade, AppPresenceEvent, AppPresenceSubscription, AppPresenceSubscriptionError,
};
use uc_core::ids::DeviceId;
use uc_core::ports::{PresenceError, ReachabilityState};

use crate::daemon::service::{DaemonService, ServiceHealth};

/// Base cadence — peers in good standing redial every 25s, comfortably
/// below iroh's ~60s QUIC idle timeout so the path cache stays warm.
const BASE_INTERVAL: Duration = Duration::from_secs(25);

/// Backoff ladder indexed by `consecutive_failures`. Index 0 is the "no
/// failures" base cadence. Failures escalate: 1×60s → 2×5min → 3+×15min.
/// 15min cap balances "don't burn batteries on a long-offline peer" vs
/// "still re-probe occasionally in case both inbound presence and our
/// connection cache miss the recovery".
const BACKOFF_LADDER: [Duration; 4] = [
    BASE_INTERVAL,
    Duration::from_secs(60),
    Duration::from_secs(5 * 60),
    Duration::from_secs(15 * 60),
];

#[derive(Debug, Clone, PartialEq, Eq)]
struct BackoffState {
    consecutive_failures: u32,
    next_dial_at: Instant,
}

impl BackoffState {
    /// New peer — eligible immediately so the very next tick dials it.
    fn ready_now(now: Instant) -> Self {
        Self {
            consecutive_failures: 0,
            next_dial_at: now,
        }
    }

    fn ready(&self, now: Instant) -> bool {
        now >= self.next_dial_at
    }

    fn on_success(&mut self, now: Instant) {
        self.consecutive_failures = 0;
        self.next_dial_at = now + BACKOFF_LADDER[0];
    }

    fn on_failure(&mut self, now: Instant) {
        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        let idx = (self.consecutive_failures as usize).min(BACKOFF_LADDER.len() - 1);
        self.next_dial_at = now + BACKOFF_LADDER[idx];
    }

    /// Reset triggered by an inbound `Online` event — equivalent to
    /// `on_success` semantically (peer is reachable, clear failure count,
    /// schedule next dial at base cadence) but called from a different
    /// site so the log lines stay distinguishable.
    fn reset(&mut self, now: Instant) {
        self.consecutive_failures = 0;
        self.next_dial_at = now + BACKOFF_LADDER[0];
    }
}

pub struct PeerKeepAliveWorker {
    app_facade: Arc<AppFacade>,
}

impl PeerKeepAliveWorker {
    pub fn new(app_facade: Arc<AppFacade>) -> Self {
        Self { app_facade }
    }

    /// Run one scheduling pass: discover peers, prune missing, dial only
    /// peers whose backoff has elapsed, update each peer's state from its
    /// dial result.
    async fn dial_due_peers(&self, backoff: &mut HashMap<String, BackoffState>) {
        let peers = match self.app_facade.list_paired_peer_device_ids().await {
            Ok(ps) => ps,
            Err(err) => {
                warn!(error = %err, "list_paired_peer_device_ids failed; skipping keepalive tick");
                return;
            }
        };

        let now = Instant::now();
        let active: HashSet<String> = peers.iter().map(|d| d.as_str().to_string()).collect();
        backoff.retain(|k, _| active.contains(k));

        let mut due: Vec<DeviceId> = Vec::new();
        for device in peers {
            let key = device.as_str().to_string();
            let is_due = backoff
                .entry(key)
                .or_insert_with(|| BackoffState::ready_now(now))
                .ready(now);
            if is_due {
                due.push(device);
            }
        }

        if due.is_empty() {
            debug!(
                tracked = backoff.len(),
                "keepalive tick: no peers due (all within backoff window)"
            );
            return;
        }

        let mut set: JoinSet<(DeviceId, Result<ReachabilityState, PresenceError>)> = JoinSet::new();
        for device in due {
            let app_facade = Arc::clone(&self.app_facade);
            set.spawn(async move {
                let result = app_facade.ensure_reachable_one(&device).await;
                (device, result)
            });
        }

        while let Some(joined) = set.join_next().await {
            let observed_at = Instant::now();
            match joined {
                Ok((device, Ok(ReachabilityState::Online))) => {
                    let key = device.as_str().to_string();
                    if let Some(entry) = backoff.get_mut(&key) {
                        entry.on_success(observed_at);
                        debug!(device = %key, "keepalive dial → Online; backoff at base");
                    }
                }
                Ok((device, Ok(_))) => {
                    let key = device.as_str().to_string();
                    if let Some(entry) = backoff.get_mut(&key) {
                        entry.on_failure(observed_at);
                        debug!(
                            device = %key,
                            fails = entry.consecutive_failures,
                            "keepalive dial → Offline/Unknown; backoff escalated"
                        );
                    }
                }
                Ok((device, Err(err))) => {
                    let key = device.as_str().to_string();
                    if let Some(entry) = backoff.get_mut(&key) {
                        entry.on_failure(observed_at);
                        debug!(
                            device = %key,
                            error = %err,
                            fails = entry.consecutive_failures,
                            "keepalive dial errored; backoff escalated"
                        );
                    }
                }
                Err(err) => {
                    warn!(error = %err, "keepalive dial task panicked or was cancelled");
                }
            }
        }
    }

    /// Apply an inbound presence event to the backoff map. Online events
    /// reset the relevant peer to base cadence; other states are ignored
    /// (Offline is owned by the watchdog, Unknown is uninformative).
    fn apply_presence_event(backoff: &mut HashMap<String, BackoffState>, event: AppPresenceEvent) {
        if event.state != "online" {
            return;
        }
        let now = Instant::now();
        let entry = backoff
            .entry(event.device_id.clone())
            .or_insert_with(|| BackoffState::ready_now(now));
        entry.reset(now);
        debug!(
            device = %event.device_id,
            "inbound presence Online; backoff reset to base cadence",
        );
    }
}

/// Drain one item from the presence subscription. Pending forever when
/// the subscription is unavailable so `tokio::select!` doesn't hot-loop.
/// Closes the subscription (sets it to `None`) on `Closed` so subsequent
/// calls fall into the pending branch instead of busy-looping.
async fn next_presence_event(rx: &mut Option<AppPresenceSubscription>) -> Option<AppPresenceEvent> {
    let r = match rx.as_mut() {
        Some(r) => r,
        None => {
            std::future::pending::<()>().await;
            unreachable!("pending future never resolves");
        }
    };
    match r.recv().await {
        Ok(event) => Some(event),
        Err(AppPresenceSubscriptionError::Lagged(skipped)) => {
            warn!(
                skipped,
                "presence subscription lagged; backoff state may briefly mis-bookkeep",
            );
            None
        }
        Err(AppPresenceSubscriptionError::Closed) => {
            warn!("presence subscription closed; keepalive falling back to time-only mode");
            *rx = None;
            None
        }
    }
}

#[async_trait]
impl DaemonService for PeerKeepAliveWorker {
    fn name(&self) -> &str {
        "peer-keepalive"
    }

    async fn start(&self, cancel: CancellationToken) -> anyhow::Result<()> {
        let mut ticker = tokio::time::interval(BASE_INTERVAL);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
        // Skip the immediate first tick — `auto_prime_presence` already
        // ran one full pass right after `start_network`, so the first
        // 25s sleep is the right cadence boundary.
        ticker.tick().await;

        let mut presence_rx: Option<AppPresenceSubscription> =
            match self.app_facade.subscribe_peer_presence_events() {
                Ok(rx) => Some(rx),
                Err(err) => {
                    warn!(
                        error = %err,
                        "presence subscription unavailable; keepalive degraded to time-only mode",
                    );
                    None
                }
            };

        let mut backoff: HashMap<String, BackoffState> = HashMap::new();

        info!(
            base_interval_secs = BASE_INTERVAL.as_secs(),
            backoff_cap_secs = BACKOFF_LADDER[BACKOFF_LADDER.len() - 1].as_secs(),
            "peer keepalive started",
        );

        loop {
            tokio::select! {
                biased;
                _ = cancel.cancelled() => break,
                event = next_presence_event(&mut presence_rx) => {
                    if let Some(event) = event {
                        Self::apply_presence_event(&mut backoff, event);
                    }
                }
                _ = ticker.tick() => {
                    // CLI bundles without space_setup surface as
                    // `EnsureReachableAllError::Repository("space setup
                    // facade unavailable")` from the AppFacade thin
                    // wrappers — `dial_due_peers` logs and skips.
                    self.dial_due_peers(&mut backoff).await;
                }
            }
        }

        info!("peer keepalive cancelled");
        Ok(())
    }

    async fn stop(&self) -> anyhow::Result<()> {
        info!("peer keepalive stopped");
        Ok(())
    }

    fn health_check(&self) -> ServiceHealth {
        ServiceHealth::Healthy
    }
}

// ============================================================================
// Tests — pure-logic coverage for the BackoffState machine.
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn at(now: Instant, plus: Duration) -> Instant {
        now + plus
    }

    #[test]
    fn ready_now_dials_immediately() {
        let now = Instant::now();
        let s = BackoffState::ready_now(now);
        assert!(s.ready(now));
        assert!(s.ready(at(now, Duration::from_millis(1))));
        assert_eq!(s.consecutive_failures, 0);
    }

    #[test]
    fn on_success_resets_to_base_cadence() {
        let now = Instant::now();
        let mut s = BackoffState::ready_now(now);
        s.consecutive_failures = 5;

        s.on_success(now);

        assert_eq!(s.consecutive_failures, 0);
        assert!(!s.ready(now));
        assert!(s.ready(at(now, BASE_INTERVAL)));
    }

    #[test]
    fn on_failure_walks_the_ladder_and_caps() {
        let now = Instant::now();
        let mut s = BackoffState::ready_now(now);

        // 1st failure → 60s window.
        s.on_failure(now);
        assert_eq!(s.consecutive_failures, 1);
        assert!(s.ready(at(now, Duration::from_secs(60))));
        assert!(!s.ready(at(now, Duration::from_secs(59))));

        // 2nd failure → 5min window from `now`.
        s.on_failure(now);
        assert_eq!(s.consecutive_failures, 2);
        assert!(s.ready(at(now, Duration::from_secs(5 * 60))));
        assert!(!s.ready(at(now, Duration::from_secs(5 * 60 - 1))));

        // 3rd failure → 15min cap.
        s.on_failure(now);
        assert_eq!(s.consecutive_failures, 3);
        assert!(s.ready(at(now, Duration::from_secs(15 * 60))));

        // 4th, 5th, … keep capping at 15min — counter still climbs (for
        // observability) but the window does not.
        s.on_failure(now);
        s.on_failure(now);
        assert_eq!(s.consecutive_failures, 5);
        assert!(s.ready(at(now, Duration::from_secs(15 * 60))));
        assert!(!s.ready(at(now, Duration::from_secs(15 * 60 - 1))));
    }

    #[test]
    fn reset_clears_failure_count_and_returns_to_base() {
        let now = Instant::now();
        let mut s = BackoffState::ready_now(now);
        s.on_failure(now);
        s.on_failure(now);
        s.on_failure(now);
        assert_eq!(s.consecutive_failures, 3);

        s.reset(now);

        assert_eq!(s.consecutive_failures, 0);
        // Even right after reset, we wait one base interval before the
        // next dial — otherwise an inbound Online event would prompt an
        // immediate redundant dial.
        assert!(!s.ready(now));
        assert!(s.ready(at(now, BASE_INTERVAL)));
    }

    #[test]
    fn apply_presence_event_resets_existing_entry() {
        let now = Instant::now();
        let mut backoff: HashMap<String, BackoffState> = HashMap::new();
        let mut s = BackoffState::ready_now(now);
        s.on_failure(now);
        s.on_failure(now);
        backoff.insert("device-a".into(), s);

        let event = AppPresenceEvent {
            device_id: "device-a".into(),
            state: "online".into(),
            at_ms: 0,
        };
        PeerKeepAliveWorker::apply_presence_event(&mut backoff, event);

        let entry = backoff.get("device-a").expect("entry retained");
        assert_eq!(entry.consecutive_failures, 0);
    }

    #[test]
    fn apply_presence_event_inserts_unknown_peer_for_later_pruning() {
        // An Online event for a peer we've never dialed should still
        // create an entry — the next tick's prune pass will drop it if
        // the peer isn't actually paired.
        let mut backoff: HashMap<String, BackoffState> = HashMap::new();
        let event = AppPresenceEvent {
            device_id: "ghost".into(),
            state: "online".into(),
            at_ms: 0,
        };

        PeerKeepAliveWorker::apply_presence_event(&mut backoff, event);
        assert!(backoff.contains_key("ghost"));
    }

    #[test]
    fn apply_presence_event_ignores_offline_and_unknown() {
        let mut backoff: HashMap<String, BackoffState> = HashMap::new();
        for state in ["offline", "unknown", "weird-future-variant"] {
            let event = AppPresenceEvent {
                device_id: "device-x".into(),
                state: state.into(),
                at_ms: 0,
            };
            PeerKeepAliveWorker::apply_presence_event(&mut backoff, event);
        }
        assert!(
            backoff.is_empty(),
            "non-online events must not pollute the backoff map",
        );
    }
}
