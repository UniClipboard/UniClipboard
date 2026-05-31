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
//!
//! Ladder (Active): 25s → 60s → 5min. After **3 consecutive failures** the
//! peer transitions to `Sleeping`, which means the 25s base ticker stops
//! redialing it entirely. A long-offline peer no longer eats periodic UDP
//! probes — recovery is signalled via one of three paths:
//!
//! 1. **Inbound `Online` event** (the common case). When the peer comes
//!    back its own keepalive dials our `PRESENCE_ALPN`, which the
//!    `IrohPresenceHandler` translates into an Online presence event. The
//!    worker resets the backoff to `Active(0)` and spawns a fire-and-forget
//!    outbound `ensure_reachable_one` so the outbound path is warmed
//!    within ~1s of the peer reappearing instead of waiting for the next
//!    ticker. The spawned dial's result is intentionally not joined back
//!    into the backoff map — if it fails the next ticker pass will record
//!    `Active(1)` like any other failure.
//! 2. **Successful regular dial** — peer is healthy, clear failure count.
//! 3. **30min fallback fan-out ticker** — catch-all for the "both peers
//!    were dead at the same time" case where neither side's keepalive ever
//!    fires. Every 30 minutes the worker dials every `Sleeping` peer once;
//!    success wakes them to `Active(0)`, failure keeps them in `Sleeping`
//!    until the next 30min cycle.
//!
//! ## Design (worker mechanics)
//!
//! * Drives off a 25s base ticker (`MissedTickBehavior::Delay`), a 30min
//!   fallback ticker for `Sleeping` peers, and an inbound
//!   `AppPresenceSubscription`. All arms use `tokio::select!` with
//!   `biased` so cancellation and presence resets land before a ticker
//!   fires.
//! * Regular tick: `list_paired_peer_device_ids` from the facade, prune
//!   removed peers from the backoff map, default-init new peers as ready,
//!   then dispatch `JoinSet` of `ensure_reachable_one` calls only for
//!   `Active` peers whose `next_dial_at` has elapsed.
//! * Fallback tick: same list + prune, then dispatch dials only for
//!   `Sleeping` peers (typically none — the ticker no-ops in steady state).
//! * Result of each dial updates that peer's `BackoffState`.
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

/// Backoff ladder for the `Active` variant, indexed by
/// `consecutive_failures`. Index 0 is the "no failures" base cadence;
/// indexes 1 and 2 are escalating failure windows. A third failure
/// (failures reaches `SLEEP_AFTER_FAILURES`) transitions the peer to
/// `Sleeping` rather than landing back in `Active` — the old 15min cap
/// is gone because in practice it just burned UDP forever on a dead
/// peer; explicit sleep is cheaper and more honest.
const BACKOFF_LADDER: [Duration; 3] = [
    BASE_INTERVAL,
    Duration::from_secs(60),
    Duration::from_secs(5 * 60),
];

/// Number of consecutive failures that flips an `Active` peer to
/// `Sleeping`. With the ladder above this is hit after ~6min of
/// continuous failure (25s + 60s + 5min).
const SLEEP_AFTER_FAILURES: u32 = 3;

/// How often the worker fans out a one-shot dial to every `Sleeping`
/// peer. This is the only path that re-probes long-dead peers without
/// an inbound signal, so the cadence is a tradeoff: too short and
/// `Sleeping` loses its battery-saving point; too long and a "both
/// sides were offline simultaneously" recovery stays invisible for
/// hours. 30min is the lowest value where the per-tick UDP cost is
/// effectively free.
const SLEEP_FALLBACK_INTERVAL: Duration = Duration::from_secs(30 * 60);

#[derive(Debug, Clone, PartialEq, Eq)]
enum BackoffState {
    /// Peer is in the regular keepalive rotation. `consecutive_failures`
    /// indexes the ladder for `next_dial_at`. A failure increment that
    /// reaches `SLEEP_AFTER_FAILURES` transitions to `Sleeping`.
    Active {
        consecutive_failures: u32,
        next_dial_at: Instant,
    },
    /// Peer is parked. The 25s base ticker skips it entirely; only the
    /// 30min fallback ticker and inbound presence events can wake it.
    Sleeping,
}

impl BackoffState {
    /// New peer — eligible immediately so the very next tick dials it.
    fn ready_now(now: Instant) -> Self {
        BackoffState::Active {
            consecutive_failures: 0,
            next_dial_at: now,
        }
    }

    /// True iff the 25s base ticker should dial this peer this pass.
    /// `Sleeping` peers always return false — they go through the
    /// 30min fallback path instead.
    fn ready(&self, now: Instant) -> bool {
        match self {
            BackoffState::Active { next_dial_at, .. } => now >= *next_dial_at,
            BackoffState::Sleeping => false,
        }
    }

    fn is_sleeping(&self) -> bool {
        matches!(self, BackoffState::Sleeping)
    }

    fn on_success(&mut self, now: Instant) {
        *self = BackoffState::Active {
            consecutive_failures: 0,
            next_dial_at: now + BACKOFF_LADDER[0],
        };
    }

    /// Failure handling. `Active` walks the ladder until
    /// `consecutive_failures` reaches `SLEEP_AFTER_FAILURES`, then
    /// transitions to `Sleeping`. `Sleeping` stays `Sleeping` — a
    /// fallback fan-out failure just means "still offline, check again
    /// in another 30min".
    fn on_failure(&mut self, now: Instant) {
        match self {
            BackoffState::Active {
                consecutive_failures,
                ..
            } => {
                let new_fails = consecutive_failures.saturating_add(1);
                if new_fails >= SLEEP_AFTER_FAILURES {
                    *self = BackoffState::Sleeping;
                } else {
                    *self = BackoffState::Active {
                        consecutive_failures: new_fails,
                        next_dial_at: now + BACKOFF_LADDER[new_fails as usize],
                    };
                }
            }
            BackoffState::Sleeping => {}
        }
    }

    /// Reset triggered by an inbound `Online` event — equivalent to
    /// `on_success` semantically (peer is reachable, clear failure
    /// count, schedule next dial at base cadence) but valid from any
    /// state including `Sleeping`, which is the whole point: an
    /// inbound presence event from a parked peer must wake it.
    fn reset(&mut self, now: Instant) {
        *self = BackoffState::Active {
            consecutive_failures: 0,
            next_dial_at: now + BACKOFF_LADDER[0],
        };
    }
}

pub struct PeerKeepAliveWorker {
    app_facade: Arc<AppFacade>,
}

impl PeerKeepAliveWorker {
    pub fn new(app_facade: Arc<AppFacade>) -> Self {
        Self { app_facade }
    }

    /// Regular 25s tick: discover peers, prune missing, dial only `Active`
    /// peers whose `next_dial_at` has elapsed. `Sleeping` peers are
    /// always skipped here — they recover via inbound presence events or
    /// the 30min fallback ticker.
    async fn dial_due_peers(&self, backoff: &mut HashMap<String, BackoffState>) {
        let Some(peers) = self.refresh_peer_list(backoff).await else {
            return;
        };

        let now = Instant::now();
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
                "keepalive tick: no peers due (all within backoff window or Sleeping)"
            );
            return;
        }

        self.dispatch_dials_and_update(due, backoff).await;
    }

    /// 30min fallback fan-out: one-shot dial for every `Sleeping` peer.
    /// Steady-state no-op when no peers are sleeping. The only path that
    /// re-probes long-dead peers when neither side's presence keepalive
    /// can fire (e.g. both daemons crashed simultaneously).
    async fn fan_out_sleeping_peers(&self, backoff: &mut HashMap<String, BackoffState>) {
        let Some(peers) = self.refresh_peer_list(backoff).await else {
            return;
        };

        let sleeping: Vec<DeviceId> = peers
            .into_iter()
            .filter(|d| {
                backoff
                    .get(d.as_str())
                    .map(BackoffState::is_sleeping)
                    .unwrap_or(false)
            })
            .collect();

        if sleeping.is_empty() {
            debug!("fallback fan-out: no Sleeping peers");
            return;
        }

        info!(
            count = sleeping.len(),
            "fallback fan-out: dialing Sleeping peers"
        );
        self.dispatch_dials_and_update(sleeping, backoff).await;
    }

    /// List paired peers from the facade and prune the backoff map of
    /// entries whose peer is no longer paired. Returns `None` when the
    /// facade lookup fails (caller should skip the tick).
    async fn refresh_peer_list(
        &self,
        backoff: &mut HashMap<String, BackoffState>,
    ) -> Option<Vec<DeviceId>> {
        let peers = match self.app_facade.list_paired_peer_device_ids().await {
            Ok(ps) => ps,
            Err(err) => {
                warn!(error = %err, "list_paired_peer_device_ids failed; skipping keepalive tick");
                return None;
            }
        };
        let active: HashSet<String> = peers.iter().map(|d| d.as_str().to_string()).collect();
        backoff.retain(|k, _| active.contains(k));
        Some(peers)
    }

    /// Shared dial dispatch + result handler used by both the regular
    /// ticker and the fallback fan-out. Updates each peer's
    /// `BackoffState` from the dial outcome and emits state-transition
    /// log lines.
    async fn dispatch_dials_and_update(
        &self,
        devices: Vec<DeviceId>,
        backoff: &mut HashMap<String, BackoffState>,
    ) {
        let mut set: JoinSet<(DeviceId, Result<ReachabilityState, PresenceError>)> = JoinSet::new();
        for device in devices {
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
                        let was_sleeping = entry.is_sleeping();
                        entry.on_success(observed_at);
                        if was_sleeping {
                            info!(
                                device = %key,
                                "fallback dial → Online; peer waking from Sleeping",
                            );
                        } else {
                            debug!(device = %key, "keepalive dial → Online; backoff at base");
                        }
                    }
                }
                Ok((device, Ok(_))) => {
                    let key = device.as_str().to_string();
                    if let Some(entry) = backoff.get_mut(&key) {
                        Self::log_dial_failure(entry, &key, observed_at, None);
                    }
                }
                Ok((device, Err(err))) => {
                    let key = device.as_str().to_string();
                    if let Some(entry) = backoff.get_mut(&key) {
                        Self::log_dial_failure(entry, &key, observed_at, Some(&err));
                    }
                }
                Err(err) => {
                    warn!(error = %err, "keepalive dial task panicked or was cancelled");
                }
            }
        }
    }

    /// Apply `on_failure` to the entry and emit a log line that
    /// distinguishes the four observable transitions: ladder step,
    /// Active→Sleeping, fallback failure (stays Sleeping), and the
    /// impossible Sleeping→Active-on-failure path (defensive).
    fn log_dial_failure(
        entry: &mut BackoffState,
        key: &str,
        observed_at: Instant,
        err: Option<&PresenceError>,
    ) {
        let was_sleeping = entry.is_sleeping();
        entry.on_failure(observed_at);
        match (was_sleeping, &*entry) {
            (
                false,
                BackoffState::Active {
                    consecutive_failures,
                    ..
                },
            ) => match err {
                Some(e) => debug!(
                    device = %key,
                    error = %e,
                    fails = *consecutive_failures,
                    "keepalive dial errored; backoff escalated",
                ),
                None => debug!(
                    device = %key,
                    fails = *consecutive_failures,
                    "keepalive dial → Offline/Unknown; backoff escalated",
                ),
            },
            (false, BackoffState::Sleeping) => info!(
                device = %key,
                fails = SLEEP_AFTER_FAILURES,
                "keepalive dial failed; peer transitioning to Sleeping",
            ),
            (true, BackoffState::Sleeping) => match err {
                Some(e) => debug!(
                    device = %key,
                    error = %e,
                    "fallback dial errored; peer stays in Sleeping",
                ),
                None => debug!(
                    device = %key,
                    "fallback dial → Offline/Unknown; peer stays in Sleeping",
                ),
            },
            (true, BackoffState::Active { .. }) => warn!(
                device = %key,
                "unexpected: Sleeping peer transitioned to Active on failure",
            ),
        }
    }

    /// Apply an inbound presence event to the backoff map. Online events
    /// reset the relevant peer to base cadence and return the `DeviceId`
    /// so the caller can spawn an immediate outbound dial; other states
    /// are ignored (Offline is owned by the watchdog, Unknown is
    /// uninformative) and return `None`.
    ///
    /// Split out from spawning so the pure backoff logic stays testable
    /// without standing up an `AppFacade`.
    fn apply_presence_event(
        backoff: &mut HashMap<String, BackoffState>,
        event: AppPresenceEvent,
    ) -> Option<DeviceId> {
        if event.state != "online" {
            return None;
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
        Some(DeviceId::new(&event.device_id))
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

        // Fallback fan-out for Sleeping peers. Same first-tick burn
        // pattern: no point firing instantly at startup when there are
        // no Sleeping peers yet.
        let mut fallback_ticker = tokio::time::interval(SLEEP_FALLBACK_INTERVAL);
        fallback_ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
        fallback_ticker.tick().await;

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
            sleep_after_failures = SLEEP_AFTER_FAILURES,
            fallback_interval_secs = SLEEP_FALLBACK_INTERVAL.as_secs(),
            "peer keepalive started",
        );

        loop {
            tokio::select! {
                biased;
                _ = cancel.cancelled() => break,
                event = next_presence_event(&mut presence_rx) => {
                    if let Some(event) = event {
                        if let Some(device) = Self::apply_presence_event(&mut backoff, event) {
                            // Fire-and-forget: warm the outbound PRESENCE
                            // path right now instead of waiting up to 25s
                            // for the next ticker. Fast-path returns
                            // instantly if a live connection already
                            // exists, so duplicate spawns from a burst of
                            // online events are effectively free. The
                            // result is intentionally dropped — failures
                            // are observed on the next tick's dial.
                            let app_facade = Arc::clone(&self.app_facade);
                            tokio::spawn(async move {
                                debug!(
                                    device = %device.as_str(),
                                    "inbound presence Online; spawning outbound dial",
                                );
                                let _ = app_facade.ensure_reachable_one(&device).await;
                            });
                        }
                    }
                }
                _ = ticker.tick() => {
                    // CLI bundles without space_setup surface as
                    // `EnsureReachableAllError::Repository("space setup
                    // facade unavailable")` from the AppFacade thin
                    // wrappers — `dial_due_peers` logs and skips.
                    self.dial_due_peers(&mut backoff).await;
                }
                _ = fallback_ticker.tick() => {
                    // 30min fan-out for Sleeping peers. Catches the
                    // "both sides offline simultaneously, neither
                    // side's PRESENCE keepalive ever fires" case.
                    // No-op (debug log only) when no peers are sleeping.
                    self.fan_out_sleeping_peers(&mut backoff).await;
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

    /// Convenience accessor for tests — pattern-matching `Active.consecutive_failures`
    /// every assertion adds noise without value.
    fn fails_of(s: &BackoffState) -> Option<u32> {
        match s {
            BackoffState::Active {
                consecutive_failures,
                ..
            } => Some(*consecutive_failures),
            BackoffState::Sleeping => None,
        }
    }

    #[test]
    fn ready_now_dials_immediately() {
        let now = Instant::now();
        let s = BackoffState::ready_now(now);
        assert!(s.ready(now));
        assert!(s.ready(at(now, Duration::from_millis(1))));
        assert_eq!(fails_of(&s), Some(0));
    }

    #[test]
    fn on_success_resets_to_base_cadence() {
        let now = Instant::now();
        let mut s = BackoffState::Active {
            consecutive_failures: 2,
            next_dial_at: now + Duration::from_secs(5 * 60),
        };

        s.on_success(now);

        assert_eq!(fails_of(&s), Some(0));
        assert!(!s.ready(now));
        assert!(s.ready(at(now, BASE_INTERVAL)));
    }

    #[test]
    fn on_failure_walks_ladder_then_transitions_to_sleeping() {
        let now = Instant::now();
        let mut s = BackoffState::ready_now(now);

        // 1st failure → Active(1), 60s window.
        s.on_failure(now);
        assert_eq!(fails_of(&s), Some(1));
        assert!(s.ready(at(now, Duration::from_secs(60))));
        assert!(!s.ready(at(now, Duration::from_secs(59))));

        // 2nd failure → Active(2), 5min window.
        s.on_failure(now);
        assert_eq!(fails_of(&s), Some(2));
        assert!(s.ready(at(now, Duration::from_secs(5 * 60))));
        assert!(!s.ready(at(now, Duration::from_secs(5 * 60 - 1))));

        // 3rd failure → Sleeping. The 25s ticker now permanently skips it.
        s.on_failure(now);
        assert!(s.is_sleeping(), "third failure must transition to Sleeping");
        assert!(!s.ready(now));
        assert!(!s.ready(at(now, Duration::from_secs(60 * 60))));
    }

    #[test]
    fn sleeping_stays_sleeping_on_failure() {
        // Fallback fan-out dial against a Sleeping peer that's still
        // offline must not flip the state — next chance is the next
        // fallback cycle (30min later), not a fresh ladder walk.
        let now = Instant::now();
        let mut s = BackoffState::Sleeping;
        s.on_failure(now);
        assert!(s.is_sleeping());
    }

    #[test]
    fn reset_clears_failure_count_from_any_state() {
        let now = Instant::now();

        // From Active(n>0).
        let mut s = BackoffState::ready_now(now);
        s.on_failure(now);
        s.on_failure(now);
        assert_eq!(fails_of(&s), Some(2));
        s.reset(now);
        assert_eq!(fails_of(&s), Some(0));
        assert!(!s.ready(now));
        assert!(s.ready(at(now, BASE_INTERVAL)));

        // From Sleeping — the whole point of the variant is that an
        // inbound Online event can rescue it.
        let mut s = BackoffState::Sleeping;
        s.reset(now);
        assert_eq!(fails_of(&s), Some(0));
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
        let wake = PeerKeepAliveWorker::apply_presence_event(&mut backoff, event);

        let entry = backoff.get("device-a").expect("entry retained");
        assert_eq!(fails_of(entry), Some(0));
        assert_eq!(
            wake.as_ref().map(|d| d.as_str()),
            Some("device-a"),
            "online event must signal a wake-up dial",
        );
    }

    #[test]
    fn apply_presence_event_wakes_sleeping_peer() {
        // The cornerstone of Phase 3: a Sleeping peer hears its
        // counterpart dial in and immediately rejoins the rotation.
        let mut backoff: HashMap<String, BackoffState> = HashMap::new();
        backoff.insert("dorm".into(), BackoffState::Sleeping);

        let event = AppPresenceEvent {
            device_id: "dorm".into(),
            state: "online".into(),
            at_ms: 0,
        };
        let wake = PeerKeepAliveWorker::apply_presence_event(&mut backoff, event);

        let entry = backoff.get("dorm").expect("entry retained");
        assert!(
            !entry.is_sleeping(),
            "Sleeping peer must wake on inbound Online event",
        );
        assert_eq!(fails_of(entry), Some(0));
        assert_eq!(
            wake.as_ref().map(|d| d.as_str()),
            Some("dorm"),
            "waking event must also trigger a fire-and-forget outbound dial",
        );
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

        let wake = PeerKeepAliveWorker::apply_presence_event(&mut backoff, event);
        assert!(backoff.contains_key("ghost"));
        assert_eq!(
            wake.as_ref().map(|d| d.as_str()),
            Some("ghost"),
            "online event must signal a wake-up dial even for unknown peers",
        );
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
            let wake = PeerKeepAliveWorker::apply_presence_event(&mut backoff, event);
            assert!(
                wake.is_none(),
                "non-online events must not trigger a wake-up dial",
            );
        }
        assert!(
            backoff.is_empty(),
            "non-online events must not pollute the backoff map",
        );
    }
}
