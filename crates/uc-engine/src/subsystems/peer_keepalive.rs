use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::task::JoinSet;
use tokio::time::MissedTickBehavior;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};
use uc_application::facade::{
    AppFacade, AppPresenceEvent, AppPresenceSubscription, AppPresenceSubscriptionError,
};
use uc_core::ids::DeviceId;
use uc_core::ports::{PresenceError, ReachabilityState};
use uc_core::TaskRegistry;

use crate::engine::event_stream::EventSender;
use crate::{EngineEvent, PeerPresenceChanged};

const BASE_INTERVAL: Duration = Duration::from_secs(25);
const BACKOFF_LADDER: [Duration; 3] = [
    BASE_INTERVAL,
    Duration::from_secs(60),
    Duration::from_secs(5 * 60),
];
const SLEEP_AFTER_FAILURES: u32 = 3;
const SLEEP_FALLBACK_INTERVAL: Duration = Duration::from_secs(30 * 60);

#[derive(Debug, Clone, PartialEq, Eq)]
enum BackoffState {
    Active {
        consecutive_failures: u32,
        next_dial_at: Instant,
    },
    Sleeping,
}

impl BackoffState {
    fn ready_now(now: Instant) -> Self {
        Self::Active {
            consecutive_failures: 0,
            next_dial_at: now,
        }
    }

    fn ready(&self, now: Instant) -> bool {
        match self {
            Self::Active { next_dial_at, .. } => now >= *next_dial_at,
            Self::Sleeping => false,
        }
    }

    fn is_sleeping(&self) -> bool {
        matches!(self, Self::Sleeping)
    }

    fn on_success(&mut self, now: Instant) {
        *self = Self::Active {
            consecutive_failures: 0,
            next_dial_at: now + BACKOFF_LADDER[0],
        };
    }

    fn on_failure(&mut self, now: Instant) {
        match self {
            Self::Active {
                consecutive_failures,
                ..
            } => {
                let failures = consecutive_failures.saturating_add(1);
                if failures >= SLEEP_AFTER_FAILURES {
                    *self = Self::Sleeping;
                } else {
                    *self = Self::Active {
                        consecutive_failures: failures,
                        next_dial_at: now + BACKOFF_LADDER[failures as usize],
                    };
                }
            }
            Self::Sleeping => {}
        }
    }

    fn reset(&mut self, now: Instant) {
        self.on_success(now);
    }
}

struct PeerKeepaliveRuntime {
    facade: Arc<AppFacade>,
    events: EventSender,
}

impl PeerKeepaliveRuntime {
    async fn run(self, cancel: CancellationToken) {
        let mut ticker = tokio::time::interval(BASE_INTERVAL);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
        ticker.tick().await;

        let mut fallback_ticker = tokio::time::interval(SLEEP_FALLBACK_INTERVAL);
        fallback_ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
        fallback_ticker.tick().await;

        let mut presence = match self.facade.subscribe_peer_presence_events() {
            Ok(subscription) => Some(subscription),
            Err(_) => {
                info!("Presence subscription unavailable; keepalive uses timers only");
                None
            }
        };
        let mut backoff = HashMap::new();
        let mut wake_dials = JoinSet::new();

        info!(
            base_interval_secs = BASE_INTERVAL.as_secs(),
            sleep_after_failures = SLEEP_AFTER_FAILURES,
            fallback_interval_secs = SLEEP_FALLBACK_INTERVAL.as_secs(),
            "Peer keepalive started"
        );

        loop {
            tokio::select! {
                biased;
                _ = cancel.cancelled() => break,
                joined = wake_dials.join_next(), if !wake_dials.is_empty() => {
                    if matches!(joined, Some(Err(_))) {
                        warn!("Peer keepalive wake dial task failed");
                    }
                }
                event = next_presence_event(&mut presence) => {
                    if let Some(event) = event {
                        self.events.send(engine_event_for_presence(&event));
                        if let Some(device) = apply_presence_event(&mut backoff, event) {
                            let facade = Arc::clone(&self.facade);
                            wake_dials.spawn(async move {
                                let _ = facade.ensure_reachable_one(&device).await;
                            });
                        }
                    }
                }
                _ = ticker.tick() => {
                    if !self.dial_due_peers(&mut backoff, &cancel).await {
                        break;
                    }
                }
                _ = fallback_ticker.tick() => {
                    if !self.fan_out_sleeping_peers(&mut backoff, &cancel).await {
                        break;
                    }
                }
            }
        }

        wake_dials.abort_all();
        while wake_dials.join_next().await.is_some() {}
        info!("Peer keepalive stopped");
    }

    async fn dial_due_peers(
        &self,
        backoff: &mut HashMap<String, BackoffState>,
        cancel: &CancellationToken,
    ) -> bool {
        let Some(peers) = self.refresh_peer_list(backoff).await else {
            return true;
        };
        let now = Instant::now();
        let due = peers
            .into_iter()
            .filter(|device| {
                backoff
                    .entry(device.as_str().to_string())
                    .or_insert_with(|| BackoffState::ready_now(now))
                    .ready(now)
            })
            .collect::<Vec<_>>();

        if due.is_empty() {
            debug!(tracked = backoff.len(), "No peers due for keepalive");
            return true;
        }
        self.dispatch_dials_and_update(due, backoff, cancel).await
    }

    async fn fan_out_sleeping_peers(
        &self,
        backoff: &mut HashMap<String, BackoffState>,
        cancel: &CancellationToken,
    ) -> bool {
        let Some(peers) = self.refresh_peer_list(backoff).await else {
            return true;
        };
        let sleeping = peers
            .into_iter()
            .filter(|device| {
                backoff
                    .get(device.as_str())
                    .map(BackoffState::is_sleeping)
                    .unwrap_or(false)
            })
            .collect::<Vec<_>>();

        if sleeping.is_empty() {
            debug!("No sleeping peers due for fallback keepalive");
            return true;
        }
        info!(count = sleeping.len(), "Dialing sleeping peers");
        self.dispatch_dials_and_update(sleeping, backoff, cancel)
            .await
    }

    async fn refresh_peer_list(
        &self,
        backoff: &mut HashMap<String, BackoffState>,
    ) -> Option<Vec<DeviceId>> {
        let peers = match self.facade.list_paired_peer_device_ids().await {
            Ok(peers) => peers,
            Err(_) => {
                warn!("Failed to list paired peers for keepalive");
                return None;
            }
        };
        let paired = peers
            .iter()
            .map(|device| device.as_str().to_string())
            .collect::<HashSet<_>>();
        backoff.retain(|device, _| paired.contains(device));
        Some(peers)
    }

    async fn dispatch_dials_and_update(
        &self,
        devices: Vec<DeviceId>,
        backoff: &mut HashMap<String, BackoffState>,
        cancel: &CancellationToken,
    ) -> bool {
        let mut dials: JoinSet<(DeviceId, Result<ReachabilityState, PresenceError>)> =
            JoinSet::new();
        for device in devices {
            let facade = Arc::clone(&self.facade);
            dials.spawn(async move {
                let result = facade.ensure_reachable_one(&device).await;
                (device, result)
            });
        }

        loop {
            tokio::select! {
                biased;
                _ = cancel.cancelled() => {
                    dials.abort_all();
                    while dials.join_next().await.is_some() {}
                    return false;
                }
                joined = dials.join_next() => {
                    let Some(joined) = joined else {
                        return true;
                    };
                    Self::apply_dial_result(backoff, joined);
                }
            }
        }
    }

    fn apply_dial_result(
        backoff: &mut HashMap<String, BackoffState>,
        joined: Result<
            (DeviceId, Result<ReachabilityState, PresenceError>),
            tokio::task::JoinError,
        >,
    ) {
        let observed_at = Instant::now();
        match joined {
            Ok((device, Ok(ReachabilityState::Online))) => {
                if let Some(entry) = backoff.get_mut(device.as_str()) {
                    let was_sleeping = entry.is_sleeping();
                    entry.on_success(observed_at);
                    if was_sleeping {
                        info!("Sleeping peer returned online");
                    } else {
                        debug!("Peer keepalive succeeded");
                    }
                }
            }
            Ok((device, result)) => {
                if let Some(entry) = backoff.get_mut(device.as_str()) {
                    Self::apply_dial_failure(entry, result.is_err(), observed_at);
                }
            }
            Err(_) => warn!("Peer keepalive dial task failed"),
        }
    }

    fn apply_dial_failure(entry: &mut BackoffState, had_error: bool, observed_at: Instant) {
        let was_sleeping = entry.is_sleeping();
        entry.on_failure(observed_at);
        match (was_sleeping, &*entry) {
            (
                false,
                BackoffState::Active {
                    consecutive_failures,
                    ..
                },
            ) => debug!(
                had_error,
                failures = *consecutive_failures,
                "Peer keepalive backoff increased"
            ),
            (false, BackoffState::Sleeping) => info!(
                failures = SLEEP_AFTER_FAILURES,
                "Peer keepalive entered sleeping state"
            ),
            (true, BackoffState::Sleeping) => {
                debug!(had_error, "Sleeping peer remains unreachable")
            }
            (true, BackoffState::Active { .. }) => {
                warn!("Sleeping peer unexpectedly became active after a failed dial")
            }
        }
    }
}

async fn next_presence_event(
    subscription: &mut Option<AppPresenceSubscription>,
) -> Option<AppPresenceEvent> {
    let receiver = match subscription.as_mut() {
        Some(receiver) => receiver,
        None => {
            std::future::pending::<()>().await;
            unreachable!("pending future never resolves")
        }
    };
    match receiver.recv().await {
        Ok(event) => Some(event),
        Err(AppPresenceSubscriptionError::Lagged(skipped)) => {
            debug!(skipped, "Peer presence subscription lagged");
            None
        }
        Err(AppPresenceSubscriptionError::Closed) => {
            info!("Peer presence subscription closed; keepalive uses timers only");
            *subscription = None;
            None
        }
    }
}

fn apply_presence_event(
    backoff: &mut HashMap<String, BackoffState>,
    event: AppPresenceEvent,
) -> Option<DeviceId> {
    if event.state != "online" {
        return None;
    }
    let now = Instant::now();
    backoff
        .entry(event.device_id.clone())
        .or_insert_with(|| BackoffState::ready_now(now))
        .reset(now);
    debug!("Inbound online presence reset peer keepalive backoff");
    Some(DeviceId::new(event.device_id))
}

fn engine_event_for_presence(event: &AppPresenceEvent) -> EngineEvent {
    EngineEvent::PeerPresenceChanged(PeerPresenceChanged {
        device_id: event.device_id.clone(),
        state: event.state.clone(),
        at_ms: event.at_ms,
    })
}

pub(crate) async fn spawn_peer_keepalive_task(
    facade: Arc<AppFacade>,
    tasks: &Arc<TaskRegistry>,
    events: EventSender,
) {
    tasks
        .spawn("peer_keepalive", move |cancel| async move {
            PeerKeepaliveRuntime { facade, events }.run(cancel).await;
        })
        .await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EngineEvent, PeerPresenceChanged};

    fn failures(state: &BackoffState) -> Option<u32> {
        match state {
            BackoffState::Active {
                consecutive_failures,
                ..
            } => Some(*consecutive_failures),
            BackoffState::Sleeping => None,
        }
    }

    #[test]
    fn failures_walk_the_ladder_then_sleep() {
        let now = Instant::now();
        let mut state = BackoffState::ready_now(now);

        state.on_failure(now);
        assert_eq!(failures(&state), Some(1));
        assert!(!state.ready(now + Duration::from_secs(59)));
        assert!(state.ready(now + Duration::from_secs(60)));

        state.on_failure(now);
        assert_eq!(failures(&state), Some(2));
        assert!(!state.ready(now + Duration::from_secs(5 * 60 - 1)));
        assert!(state.ready(now + Duration::from_secs(5 * 60)));

        state.on_failure(now);
        assert!(state.is_sleeping());
    }

    #[test]
    fn sleeping_failure_keeps_the_peer_asleep() {
        let mut state = BackoffState::Sleeping;
        state.on_failure(Instant::now());
        assert!(state.is_sleeping());
    }

    #[test]
    fn online_presence_wakes_a_sleeping_peer() {
        let mut backoff = HashMap::from([("device-a".to_string(), BackoffState::Sleeping)]);

        let wake = apply_presence_event(
            &mut backoff,
            AppPresenceEvent {
                device_id: "device-a".to_string(),
                state: "online".to_string(),
                at_ms: 0,
            },
        );

        assert_eq!(wake.as_ref().map(DeviceId::as_str), Some("device-a"));
        assert_eq!(failures(backoff.get("device-a").expect("peer")), Some(0));
    }

    #[test]
    fn non_online_presence_does_not_change_backoff() {
        let mut backoff = HashMap::new();

        let wake = apply_presence_event(
            &mut backoff,
            AppPresenceEvent {
                device_id: "device-a".to_string(),
                state: "offline".to_string(),
                at_ms: 0,
            },
        );

        assert!(wake.is_none());
        assert!(backoff.is_empty());
    }

    #[test]
    fn presence_event_keeps_the_information_needed_for_peer_refresh() {
        assert_eq!(
            engine_event_for_presence(&AppPresenceEvent {
                device_id: "device-a".to_string(),
                state: "online".to_string(),
                at_ms: 42,
            }),
            EngineEvent::PeerPresenceChanged(PeerPresenceChanged {
                device_id: "device-a".to_string(),
                state: "online".to_string(),
                at_ms: 42,
            })
        );
    }
}
