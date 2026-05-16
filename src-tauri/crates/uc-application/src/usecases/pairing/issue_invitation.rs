//! B1 · `IssuePairingInvitationUseCase`.
//!
//! Sponsor-side flow:
//!
//! 1. Delegate to [`PairingInvitationPort::issue_invitation`] — the
//!    rendezvous adapter owns the code format, TTL and transport.
//! 2. Sample `now` from [`ClockPort`] and construct the domain aggregate
//!    via [`PairingInvitation::issue`] so the invariants (state, events)
//!    stay in core.
//! 3. Park the aggregate in the application-layer
//!    [`InMemoryPairingInvitationHolder`]; P7e's sponsor-side `Incoming`
//!    subscriber will look it up by code and call `consume`.
//!
//! The aggregate's `InvitationEvent::Issued` is intentionally **not**
//! surfaced through an event bus in this slice — no subscriber needs it
//! yet, and §14.3 of `uc-application/AGENTS.md` forbids emitting events
//! with no consumer.
//!
//! [`InMemoryPairingInvitationHolder`]:
//!     crate::pairing_invitation::InMemoryPairingInvitationHolder

use std::net::IpAddr;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use tracing::{debug, info, instrument, warn};

use uc_core::pairing::invitation::PairingInvitation;
use uc_core::ports::pairing_invitation::{
    InvitationError, IssuedInvitation, PairingInvitationAddressCandidate,
    PairingInvitationAddressQueryPort, PairingInvitationByAddressPort, PairingInvitationPort,
};
use uc_core::ports::{ClockPort, DeviceIdentityPort};
use uc_observability::analytics::{AnalyticsFacade, Event, PairingMethod};

use crate::facade::space_setup::{IssuePairingInvitationError, IssuePairingInvitationResult};
use crate::pairing_invitation::InMemoryPairingInvitationHolder;

pub(crate) struct IssuePairingInvitationUseCase {
    pairing_invitation: Arc<dyn PairingInvitationPort>,
    pairing_invitation_addresses: Arc<dyn PairingInvitationAddressQueryPort>,
    /// Dev-only: the by-address variant lives on its own port so the
    /// standard sponsor lifecycle (`PairingInvitationPort`) stays free of
    /// the diagnostic surface.
    pairing_invitation_by_address: Arc<dyn PairingInvitationByAddressPort>,
    device_identity: Arc<dyn DeviceIdentityPort>,
    clock: Arc<dyn ClockPort>,
    holder: Arc<InMemoryPairingInvitationHolder>,
    /// Slice 8b' · sponsor-side `pairing_started` funnel anchor.
    /// Captured at the entry of `execute()` regardless of outcome — even
    /// "early dial failure" (NetworkNotStarted / ServiceUnavailable) leaves
    /// a started signal so PostHog can compute the "tried to invite" cohort.
    analytics: Arc<dyn AnalyticsFacade>,
}

impl IssuePairingInvitationUseCase {
    pub(crate) fn new(
        pairing_invitation: Arc<dyn PairingInvitationPort>,
        pairing_invitation_addresses: Arc<dyn PairingInvitationAddressQueryPort>,
        pairing_invitation_by_address: Arc<dyn PairingInvitationByAddressPort>,
        device_identity: Arc<dyn DeviceIdentityPort>,
        clock: Arc<dyn ClockPort>,
        holder: Arc<InMemoryPairingInvitationHolder>,
        analytics: Arc<dyn AnalyticsFacade>,
    ) -> Self {
        Self {
            pairing_invitation,
            pairing_invitation_addresses,
            pairing_invitation_by_address,
            device_identity,
            clock,
            holder,
            analytics,
        }
    }

    #[instrument(skip_all)]
    pub(crate) async fn execute(
        &self,
    ) -> Result<IssuePairingInvitationResult, IssuePairingInvitationError> {
        self.capture_pairing_started();

        // 1. Ask the rendezvous adapter for a code.
        let issued: IssuedInvitation = self
            .pairing_invitation
            .issue_invitation()
            .await
            .map_err(map_invitation_err)?;
        self.finish_issued_invitation(issued).await
    }

    #[instrument(skip_all, fields(selected_ip = %selected_ip))]
    pub(crate) async fn execute_for_address(
        &self,
        selected_ip: IpAddr,
    ) -> Result<IssuePairingInvitationResult, IssuePairingInvitationError> {
        self.capture_pairing_started();

        let issued: IssuedInvitation = self
            .pairing_invitation_by_address
            .issue_invitation_for_address(selected_ip)
            .await
            .map_err(map_invitation_err)?;
        self.finish_issued_invitation(issued).await
    }

    #[instrument(skip_all, fields(count = tracing::field::Empty))]
    pub(crate) async fn list_addresses(
        &self,
    ) -> Result<Vec<PairingInvitationAddressCandidate>, IssuePairingInvitationError> {
        let candidates = self
            .pairing_invitation_addresses
            .list_invitation_addresses()
            .await
            .map_err(map_invitation_err)?;
        tracing::Span::current().record("count", candidates.len());
        Ok(candidates)
    }

    fn capture_pairing_started(&self) {
        self.analytics.capture(Event::PairingStarted {
            method: PairingMethod::Code,
        });
    }

    async fn finish_issued_invitation(
        &self,
        issued: IssuedInvitation,
    ) -> Result<IssuePairingInvitationResult, IssuePairingInvitationError> {
        debug!(code = %issued.code.as_str(), expires_at = %issued.expires_at, "invitation issued by rendezvous");

        let issued_at = self.now_utc()?;
        let device_id = self.device_identity.current_device_id();
        let (invitation, _issued_event) =
            PairingInvitation::issue(issued.code.clone(), issued_at, issued.expires_at, device_id);

        self.holder.insert(invitation).await;
        info!(code = %issued.code.as_str(), "pairing invitation parked in holder");

        Ok(IssuePairingInvitationResult {
            code: issued.code,
            expires_at: issued.expires_at,
        })
    }

    fn now_utc(&self) -> Result<DateTime<Utc>, IssuePairingInvitationError> {
        let ms = self.clock.now_ms();
        DateTime::<Utc>::from_timestamp_millis(ms).ok_or_else(|| {
            warn!(ms, "clock returned a timestamp outside chrono's range");
            IssuePairingInvitationError::Internal("clock returned invalid timestamp".into())
        })
    }
}

fn map_invitation_err(err: InvitationError) -> IssuePairingInvitationError {
    match err {
        InvitationError::NetworkNotStarted => IssuePairingInvitationError::NetworkNotStarted,
        InvitationError::ServiceUnavailable => IssuePairingInvitationError::ServiceUnavailable,
        InvitationError::AddressNotAvailable(ip) => {
            IssuePairingInvitationError::AddressNotAvailable(ip)
        }
        InvitationError::Internal(m) => IssuePairingInvitationError::Internal(m),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Mutex as StdMutex;

    use async_trait::async_trait;
    use chrono::Duration;

    use uc_core::ids::DeviceId;
    use uc_core::pairing::invitation::{InvitationCode, InvitationState};

    /// Test fake `AnalyticsPort` that records every captured `Event` for
    /// later inspection. Mirrors the joiner-side `CapturingAnalyticsSink`.
    #[derive(Default)]
    struct CapturingAnalyticsSink {
        captured: StdMutex<Vec<Event>>,
    }
    impl CapturingAnalyticsSink {
        fn events(&self) -> Vec<Event> {
            self.captured.lock().unwrap().clone()
        }
    }
    impl uc_observability::analytics::AnalyticsPort for CapturingAnalyticsSink {
        fn capture(&self, event: Event) {
            self.captured.lock().unwrap().push(event);
        }
    }

    fn wrap_facade(sink: Arc<CapturingAnalyticsSink>) -> Arc<dyn AnalyticsFacade> {
        Arc::new(uc_observability::analytics::DefaultAnalyticsFacade::new(
            sink as Arc<dyn uc_observability::analytics::AnalyticsPort>,
            Arc::new(uc_observability::analytics::NoopAnalyticsIdentity),
        ))
    }

    // ---------- Fakes ----------

    struct FakeInvitationPort {
        next: StdMutex<FakeOutcome>,
        calls: StdMutex<u32>,
        selected_calls: StdMutex<Vec<IpAddr>>,
    }

    enum FakeOutcome {
        Ok(IssuedInvitation),
        Err(InvitationError),
    }

    impl FakeInvitationPort {
        fn with_ok(code: &str, expires_at: DateTime<Utc>) -> Self {
            Self {
                next: StdMutex::new(FakeOutcome::Ok(IssuedInvitation {
                    code: InvitationCode::new(code),
                    expires_at,
                })),
                calls: StdMutex::new(0),
                selected_calls: StdMutex::new(Vec::new()),
            }
        }
        fn with_err(err: InvitationError) -> Self {
            Self {
                next: StdMutex::new(FakeOutcome::Err(err)),
                calls: StdMutex::new(0),
                selected_calls: StdMutex::new(Vec::new()),
            }
        }
        fn calls(&self) -> u32 {
            *self.calls.lock().unwrap()
        }
        fn selected_calls(&self) -> Vec<IpAddr> {
            self.selected_calls.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl PairingInvitationPort for FakeInvitationPort {
        async fn issue_invitation(&self) -> Result<IssuedInvitation, InvitationError> {
            *self.calls.lock().unwrap() += 1;
            let out = std::mem::replace(
                &mut *self.next.lock().unwrap(),
                FakeOutcome::Err(InvitationError::Internal("already consumed".into())),
            );
            match out {
                FakeOutcome::Ok(v) => Ok(v),
                FakeOutcome::Err(e) => Err(e),
            }
        }

        async fn consume_invitation(
            &self,
            _code: &InvitationCode,
        ) -> Result<(), uc_core::ports::ConsumeInvitationError> {
            // B1 use case never drives consume directly.
            Ok(())
        }
    }

    #[async_trait]
    impl PairingInvitationAddressQueryPort for FakeInvitationPort {
        async fn list_invitation_addresses(
            &self,
        ) -> Result<Vec<PairingInvitationAddressCandidate>, InvitationError> {
            Ok(Vec::new())
        }
    }

    #[async_trait]
    impl PairingInvitationByAddressPort for FakeInvitationPort {
        async fn issue_invitation_for_address(
            &self,
            selected_ip: IpAddr,
        ) -> Result<IssuedInvitation, InvitationError> {
            self.selected_calls.lock().unwrap().push(selected_ip);
            let out = std::mem::replace(
                &mut *self.next.lock().unwrap(),
                FakeOutcome::Err(InvitationError::Internal("already consumed".into())),
            );
            match out {
                FakeOutcome::Ok(v) => Ok(v),
                FakeOutcome::Err(e) => Err(e),
            }
        }
    }

    struct FixedDeviceIdentity(DeviceId);
    impl DeviceIdentityPort for FixedDeviceIdentity {
        fn current_device_id(&self) -> DeviceId {
            self.0.clone()
        }
    }

    struct FixedClock(i64);
    impl ClockPort for FixedClock {
        fn now_ms(&self) -> i64 {
            self.0
        }
    }

    // ---------- Harness ----------

    struct Harness {
        uc: IssuePairingInvitationUseCase,
        invitation_port: Arc<FakeInvitationPort>,
        holder: Arc<InMemoryPairingInvitationHolder>,
        analytics: Arc<CapturingAnalyticsSink>,
    }

    fn expires_at() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-04-20T10:05:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    fn issued_at_ms() -> i64 {
        DateTime::parse_from_rfc3339("2026-04-20T10:00:00Z")
            .unwrap()
            .timestamp_millis()
    }

    /// Assert that the analytics sink saw exactly one `PairingStarted`
    /// event with the v1 fixed `PairingMethod::Code`. Slice 8b' funnel
    /// anchor — fired from `execute()` entry on every code path.
    fn assert_pairing_started(analytics: &Arc<CapturingAnalyticsSink>) {
        let events = analytics.events();
        assert_eq!(
            events.len(),
            1,
            "execute() must fire exactly one PairingStarted regardless of outcome"
        );
        match &events[0] {
            Event::PairingStarted { method } => {
                assert_eq!(*method, PairingMethod::Code);
            }
            other => panic!("expected PairingStarted, got {other:?}"),
        }
    }

    fn build_harness(port: Arc<FakeInvitationPort>) -> Harness {
        let device_identity: Arc<dyn DeviceIdentityPort> =
            Arc::new(FixedDeviceIdentity(DeviceId::new("sponsor-1")));
        let clock: Arc<dyn ClockPort> = Arc::new(FixedClock(issued_at_ms()));
        let holder = Arc::new(InMemoryPairingInvitationHolder::new());
        let analytics = Arc::new(CapturingAnalyticsSink::default());
        let analytics_facade = wrap_facade(analytics.clone());
        let uc = IssuePairingInvitationUseCase::new(
            port.clone() as Arc<dyn PairingInvitationPort>,
            port.clone() as Arc<dyn PairingInvitationAddressQueryPort>,
            port.clone() as Arc<dyn PairingInvitationByAddressPort>,
            device_identity,
            clock,
            holder.clone(),
            analytics_facade,
        );
        Harness {
            uc,
            invitation_port: port,
            holder,
            analytics,
        }
    }

    // ---------- Tests ----------

    #[tokio::test]
    async fn happy_path_returns_port_result_and_parks_aggregate() {
        let port = Arc::new(FakeInvitationPort::with_ok("ABCD-1234", expires_at()));
        let h = build_harness(port);

        let result = h.uc.execute().await.unwrap();

        assert_eq!(result.code.as_str(), "ABCD-1234");
        assert_eq!(result.expires_at, expires_at());
        assert_eq!(h.invitation_port.calls(), 1);

        let stored = h
            .holder
            .get_for_test(&InvitationCode::new("ABCD-1234"))
            .await
            .expect("aggregate parked");
        assert_eq!(stored.code().as_str(), "ABCD-1234");
        assert_eq!(stored.issuer_device_id().as_str(), "sponsor-1");
        match stored.state() {
            InvitationState::Pending { expires_at: e } => assert_eq!(*e, expires_at()),
            other => panic!("expected Pending, got {other:?}"),
        }
        assert_eq!(stored.issued_at().timestamp_millis(), issued_at_ms());

        // Slice 8b' · funnel anchor fired exactly once.
        assert_pairing_started(&h.analytics);
    }

    #[tokio::test]
    async fn selected_address_path_calls_selected_port_and_parks_aggregate() {
        let selected_ip = "100.79.191.42".parse().unwrap();
        let port = Arc::new(FakeInvitationPort::with_ok("ADDR-0001", expires_at()));
        let h = build_harness(port);

        let result = h.uc.execute_for_address(selected_ip).await.unwrap();

        assert_eq!(result.code.as_str(), "ADDR-0001");
        assert_eq!(h.invitation_port.calls(), 0);
        assert_eq!(h.invitation_port.selected_calls(), vec![selected_ip]);
        assert!(h
            .holder
            .get_for_test(&InvitationCode::new("ADDR-0001"))
            .await
            .is_some());
        assert_pairing_started(&h.analytics);
    }

    #[tokio::test]
    async fn maps_network_not_started_and_does_not_park() {
        let port = Arc::new(FakeInvitationPort::with_err(
            InvitationError::NetworkNotStarted,
        ));
        let h = build_harness(port);

        let err = h.uc.execute().await.unwrap_err();
        assert!(matches!(
            err,
            IssuePairingInvitationError::NetworkNotStarted
        ));
        assert_eq!(
            h.holder.len().await,
            0,
            "failure path must not park anything"
        );
        // Even an early-dial failure leaves the funnel anchor.
        assert_pairing_started(&h.analytics);
    }

    #[tokio::test]
    async fn maps_service_unavailable() {
        let port = Arc::new(FakeInvitationPort::with_err(
            InvitationError::ServiceUnavailable,
        ));
        let h = build_harness(port);

        let err = h.uc.execute().await.unwrap_err();
        assert!(matches!(
            err,
            IssuePairingInvitationError::ServiceUnavailable
        ));
        assert_pairing_started(&h.analytics);
    }

    #[tokio::test]
    async fn maps_internal_with_message() {
        let port = Arc::new(FakeInvitationPort::with_err(InvitationError::Internal(
            "boom".into(),
        )));
        let h = build_harness(port);

        let err = h.uc.execute().await.unwrap_err();
        match err {
            IssuePairingInvitationError::Internal(m) => assert_eq!(m, "boom"),
            other => panic!("expected Internal, got {other:?}"),
        }
        assert_pairing_started(&h.analytics);
    }

    #[tokio::test]
    async fn second_issue_with_same_code_overwrites_holder_entry() {
        // The fake returns only one Ok; to exercise overwrite we rebuild
        // with two Oks of the same code but different expiries.
        let first_expiry = expires_at();
        let second_expiry = expires_at() + Duration::minutes(5);

        let port = Arc::new(FakeInvitationPort::with_ok("SAME", first_expiry));
        let h = build_harness(port.clone());
        h.uc.execute().await.unwrap();

        // Second issue: reset the fake port's next outcome.
        *port.next.lock().unwrap() = FakeOutcome::Ok(IssuedInvitation {
            code: InvitationCode::new("SAME"),
            expires_at: second_expiry,
        });
        h.uc.execute().await.unwrap();

        assert_eq!(h.holder.len().await, 1, "overwrite, not two entries");
        let stored = h
            .holder
            .get_for_test(&InvitationCode::new("SAME"))
            .await
            .unwrap();
        match stored.state() {
            InvitationState::Pending { expires_at: e } => assert_eq!(*e, second_expiry),
            other => panic!("expected Pending, got {other:?}"),
        }
    }
}
