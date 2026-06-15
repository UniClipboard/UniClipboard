//! `PerPeerDispatcher` — the per-target dispatch body fanned out by the
//! use case. Owns the four ports the JoinSet task touches 1:1 per peer
//! (wire dispatch + presence preflight + the two telemetry funnels) so the
//! "send to one peer and emit its per-peer analytics" concern has a single
//! home and an independent test surface.
//!
//! ## Load-bearing invariants (Slice 8c funnel)
//!
//! - `sync_attempted` fires BEFORE any dial — on every path, including the
//!   known-offline deferral — so `attempted = succeeded + failed +
//!   deferred` holds and the dashboard can derive user-perceived attempts
//!   as `attempted - deferred`.
//! - A presence preflight of `Offline` short-circuits to
//!   `SyncDeferred(PeerKnownOffline)` + `Err(Offline)` WITHOUT dialing —
//!   redialing a peer the dispatch adapter already concluded unreachable
//!   would only burn the fan-out deadline (see the use-case module doc).
//! - `sync_latency_ms` is recorded only on success; a failure has no
//!   end-to-end timing.
//! - `mark_*` errors are warn-only and never abort the dial.

use std::sync::Arc;
use std::time::Instant;

use tracing::warn;
use uc_core::ids::DeviceId;
use uc_core::ports::{
    ClipboardDispatchError, ClipboardDispatchPort, ClipboardHeader, FirstSyncStatePort,
    PresencePort, ReachabilityState, SyncPayload,
};
use uc_observability::analytics::{
    AnalyticsPort, Direction, Event, PayloadSizeBucket, PayloadType, SyncDeferReason,
    SyncDeferredProps, SyncEventProps, TransportType,
};

use super::{dispatch_failure_stage, map_dispatch_error_to_failure_reason, PeerDispatchResult};

pub(crate) struct PerPeerDispatcher {
    clipboard_dispatch: Arc<dyn ClipboardDispatchPort>,
    presence: Arc<dyn PresencePort>,
    analytics: Arc<dyn AnalyticsPort>,
    first_sync_state: Arc<dyn FirstSyncStatePort>,
}

impl PerPeerDispatcher {
    pub(crate) fn new(
        clipboard_dispatch: Arc<dyn ClipboardDispatchPort>,
        presence: Arc<dyn PresencePort>,
        analytics: Arc<dyn AnalyticsPort>,
        first_sync_state: Arc<dyn FirstSyncStatePort>,
    ) -> Self {
        Self {
            clipboard_dispatch,
            presence,
            analytics,
            first_sync_state,
        }
    }

    /// Fire `sync_attempted` (+ first-attempt funnel). ALWAYS called before
    /// any dial so funnel parity holds on every path; the deferred path
    /// calls it too. `mark_*` failure is warn-only.
    async fn capture_attempted(
        &self,
        payload_type: PayloadType,
        payload_size_bucket: PayloadSizeBucket,
    ) {
        self.analytics.capture(Event::SyncAttempted(SyncEventProps {
            direction: Direction::Outbound,
            payload_type,
            payload_size_bucket,
            transport_type: TransportType::P2pDirect,
            peer_os: None,
            sync_latency_ms: None,
            failure_reason: None,
            failure_stage: None,
        }));
        match self.first_sync_state.mark_first_sync_attempted().await {
            Ok(true) => self.analytics.capture(Event::FirstClipboardSyncAttempted {
                direction: Direction::Outbound,
            }),
            Ok(false) => {}
            Err(err) => warn!(
                error = %err,
                "first_sync_state.mark_first_sync_attempted failed; skipping fire",
            ),
        }
    }

    /// Dispatch one encrypted payload to one peer, emitting that peer's
    /// per-peer telemetry, and return the device + wire outcome for the
    /// fan-out to fold.
    pub(crate) async fn dispatch_one(
        &self,
        device_id: DeviceId,
        header: Arc<ClipboardHeader>,
        payload: SyncPayload,
        payload_type: PayloadType,
        payload_size_bucket: PayloadSizeBucket,
    ) -> PeerDispatchResult {
        // Preflight presence, then fire attempted (ordering: attempted must
        // precede the dial / deferral so funnel parity holds).
        let preflight_state = self.presence.current_state(&device_id).await;
        let known_offline = matches!(preflight_state, ReachabilityState::Offline);
        self.capture_attempted(payload_type, payload_size_bucket)
            .await;

        // Skip the dial entirely when presence already reports Offline. The
        // dispatch adapter writes presence Offline on its own dial failures
        // and enforces a TTL re-dial, so by the time `known_offline` is true
        // we have first-hand evidence the peer is unreachable. Telemetry
        // fires `sync_deferred` (not `sync_failed`) to preserve
        // attempted+deferred parity. Background recovery is unchanged — the
        // next clipboard event retries and an inbound presence connection
        // flips the peer back to Online.
        if known_offline {
            self.analytics
                .capture(Event::SyncDeferred(SyncDeferredProps {
                    direction: Direction::Outbound,
                    payload_type,
                    payload_size_bucket,
                    peer_os: None,
                    defer_reason: SyncDeferReason::PeerKnownOffline,
                }));
            return (device_id, Err(ClipboardDispatchError::Offline));
        }

        let started_at = Instant::now();
        let result = self
            .clipboard_dispatch
            .dispatch(&device_id, &header, payload)
            .await;
        let duration_ms = started_at.elapsed().as_millis().min(u32::MAX as u128) as u32;

        let event = match &result {
            Ok(_) => Event::SyncSucceeded(SyncEventProps {
                direction: Direction::Outbound,
                payload_type,
                payload_size_bucket,
                transport_type: TransportType::P2pDirect,
                peer_os: None,
                sync_latency_ms: Some(duration_ms),
                failure_reason: None,
                failure_stage: None,
            }),
            Err(err) => Event::SyncFailed(SyncEventProps {
                direction: Direction::Outbound,
                payload_type,
                payload_size_bucket,
                transport_type: TransportType::P2pDirect,
                peer_os: None,
                sync_latency_ms: None,
                failure_reason: Some(map_dispatch_error_to_failure_reason(err)),
                failure_stage: Some(dispatch_failure_stage(err)),
            }),
        };
        let is_ok = result.is_ok();
        self.analytics.capture(event);

        // First-success funnel: fires the generic clipboard event and (if
        // File) the file-specific event. Both flags dedup independently.
        if is_ok {
            match self.first_sync_state.mark_first_sync_succeeded().await {
                Ok(true) => self.analytics.capture(Event::FirstClipboardSyncSucceeded {
                    direction: Direction::Outbound,
                    peer_os: None,
                    transport_type: TransportType::P2pDirect,
                    duration_ms,
                }),
                Ok(false) => {}
                Err(err) => warn!(
                    error = %err,
                    "first_sync_state.mark_first_sync_succeeded failed; skipping fire",
                ),
            }
            if matches!(payload_type, PayloadType::File) {
                match self.first_sync_state.mark_first_file_sync_succeeded().await {
                    Ok(true) => self.analytics.capture(Event::FirstFileSyncSucceeded {
                        peer_os: None,
                        transport_type: TransportType::P2pDirect,
                        payload_size_bucket,
                    }),
                    Ok(false) => {}
                    Err(err) => warn!(
                        error = %err,
                        "first_sync_state.mark_first_file_sync_succeeded failed; skipping fire",
                    ),
                }
            }
        }

        (device_id, result)
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::*;

    use uc_core::ports::DispatchAck;
    use uc_observability::analytics::{FailureReason, SyncFailureStage};

    fn header() -> Arc<ClipboardHeader> {
        Arc::new(test_header())
    }

    fn bucket() -> PayloadSizeBucket {
        PayloadSizeBucket::from_bytes(11)
    }

    /// Online (Unknown) peer + accepting dispatch: `sync_attempted` fires
    /// before the dial, then `sync_succeeded` carries latency. Result is the
    /// peer's `Ok(ack)`.
    #[tokio::test]
    async fn dispatch_one_success_fires_attempted_then_succeeded_with_latency() {
        let mut dispatch = MockDispatch::new();
        dispatch
            .expect_dispatch()
            .times(1)
            .returning(|_, _, _| Ok(DispatchAck::Accepted));

        let analytics = Arc::new(CapturingAnalyticsSink::default());
        let dispatcher = PerPeerDispatcher::new(
            Arc::new(dispatch),
            Arc::new(StaticPresence(ReachabilityState::Unknown)),
            analytics.clone(),
            Arc::new(AllMarkedFirstSyncState),
        );

        let (device, outcome) = dispatcher
            .dispatch_one(
                dev("peer-a"),
                header(),
                sync_payload(),
                PayloadType::Text,
                bucket(),
            )
            .await;

        assert_eq!(device.as_str(), "peer-a");
        assert!(matches!(outcome, Ok(DispatchAck::Accepted)));

        let events = analytics.events();
        assert_eq!(events.len(), 2, "got {events:?}");
        assert!(matches!(events[0], Event::SyncAttempted(_)));
        match &events[1] {
            Event::SyncSucceeded(p) => {
                assert_eq!(p.direction, Direction::Outbound);
                assert_eq!(p.transport_type, TransportType::P2pDirect);
                assert!(p.sync_latency_ms.is_some());
                assert!(p.failure_reason.is_none());
                assert!(p.failure_stage.is_none());
            }
            other => panic!("expected SyncSucceeded, got {other:?}"),
        }
    }

    /// Presence `Offline` short-circuits: the dispatch port is NEVER touched,
    /// `sync_attempted` still fires (funnel parity), and the deferral is
    /// reported via `sync_deferred(PeerKnownOffline)` + `Err(Offline)`.
    #[tokio::test]
    async fn dispatch_one_known_offline_skips_dial_and_fires_deferred() {
        let mut dispatch = MockDispatch::new();
        dispatch.expect_dispatch().times(0);

        let analytics = Arc::new(CapturingAnalyticsSink::default());
        let dispatcher = PerPeerDispatcher::new(
            Arc::new(dispatch),
            Arc::new(StaticPresence(ReachabilityState::Offline)),
            analytics.clone(),
            Arc::new(AllMarkedFirstSyncState),
        );

        let (device, outcome) = dispatcher
            .dispatch_one(
                dev("peer-off"),
                header(),
                sync_payload(),
                PayloadType::Text,
                bucket(),
            )
            .await;

        assert_eq!(device.as_str(), "peer-off");
        assert!(matches!(outcome, Err(ClipboardDispatchError::Offline)));

        let events = analytics.events();
        assert_eq!(events.len(), 2, "got {events:?}");
        assert!(matches!(events[0], Event::SyncAttempted(_)));
        match &events[1] {
            Event::SyncDeferred(p) => {
                assert_eq!(p.defer_reason, SyncDeferReason::PeerKnownOffline);
                assert_eq!(p.direction, Direction::Outbound);
            }
            other => panic!("expected SyncDeferred, got {other:?}"),
        }
    }

    /// A failing dispatch fires `sync_failed` with a mapped reason/stage and
    /// NO latency (a failure has no end-to-end timing).
    #[tokio::test]
    async fn dispatch_one_failure_fires_failed_without_latency() {
        let mut dispatch = MockDispatch::new();
        dispatch
            .expect_dispatch()
            .times(1)
            .returning(|_, _, _| Err(ClipboardDispatchError::PeerRejected("nope".to_string())));

        let analytics = Arc::new(CapturingAnalyticsSink::default());
        let dispatcher = PerPeerDispatcher::new(
            Arc::new(dispatch),
            Arc::new(StaticPresence(ReachabilityState::Unknown)),
            analytics.clone(),
            Arc::new(AllMarkedFirstSyncState),
        );

        let (device, outcome) = dispatcher
            .dispatch_one(
                dev("peer-rej"),
                header(),
                sync_payload(),
                PayloadType::Text,
                bucket(),
            )
            .await;

        assert_eq!(device.as_str(), "peer-rej");
        assert!(matches!(
            outcome,
            Err(ClipboardDispatchError::PeerRejected(_))
        ));

        let events = analytics.events();
        assert_eq!(events.len(), 2, "got {events:?}");
        assert!(matches!(events[0], Event::SyncAttempted(_)));
        match &events[1] {
            Event::SyncFailed(p) => {
                assert!(p.sync_latency_ms.is_none());
                assert_eq!(p.failure_reason, Some(FailureReason::NetworkError));
                assert_eq!(p.failure_stage, Some(SyncFailureStage::ImmediateSend));
            }
            other => panic!("expected SyncFailed, got {other:?}"),
        }
    }

    /// First-success funnel: a fresh `first_sync_state` + a `File` payload
    /// fires each `first_*` event exactly once (generic clipboard + file).
    #[tokio::test]
    async fn dispatch_one_first_file_success_fires_each_first_event_once() {
        let mut dispatch = MockDispatch::new();
        dispatch
            .expect_dispatch()
            .times(1)
            .returning(|_, _, _| Ok(DispatchAck::Accepted));

        let analytics = Arc::new(CapturingAnalyticsSink::default());
        let dispatcher = PerPeerDispatcher::new(
            Arc::new(dispatch),
            Arc::new(StaticPresence(ReachabilityState::Unknown)),
            analytics.clone(),
            Arc::new(InMemoryFirstSyncState::default()),
        );

        let _ = dispatcher
            .dispatch_one(
                dev("peer-a"),
                header(),
                sync_payload(),
                PayloadType::File,
                bucket(),
            )
            .await;

        let events = analytics.events();
        let first_attempted = events
            .iter()
            .filter(|e| matches!(e, Event::FirstClipboardSyncAttempted { .. }))
            .count();
        let first_succeeded = events
            .iter()
            .filter(|e| matches!(e, Event::FirstClipboardSyncSucceeded { .. }))
            .count();
        let first_file = events
            .iter()
            .filter(|e| matches!(e, Event::FirstFileSyncSucceeded { .. }))
            .count();
        assert_eq!(
            (first_attempted, first_succeeded, first_file),
            (1, 1, 1),
            "got {events:?}"
        );
    }
}
