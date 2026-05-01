//! Sponsor-side port adapter for [`PairingInvitationPort`].
//!
//! Owns zero HTTP concerns — delegates every round-trip to the shared
//! [`RendezvousClient`] gateway. The adapter's only job is:
//!
//! 1. Gather sponsor-side inputs (`DeviceId`, `device_name`, iroh
//!    [`EndpointAddr`]) from `uc-core` ports.
//! 2. Call the gateway.
//! 3. Map [`RendezvousHttpError`] onto the domain error types defined in
//!    `uc_core::ports::pairing_invitation`.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{TimeZone, Utc};
use iroh::Endpoint;
use tracing::{debug, instrument};

use uc_core::pairing::invitation::InvitationCode;
use uc_core::ports::{
    ConsumeInvitationError, DeviceIdentityPort, InvitationError, IssuedInvitation,
    PairingInvitationPort, SettingsPort,
};

use super::client::{CreatePairingRequest, RendezvousClient, RendezvousHttpError};

/// Rendezvous-backed adapter for [`PairingInvitationPort`].
pub struct RendezvousPairingInvitationAdapter {
    endpoint: Arc<Endpoint>,
    device_identity: Arc<dyn DeviceIdentityPort>,
    settings: Arc<dyn SettingsPort>,
    rendezvous: Arc<RendezvousClient>,
}

impl RendezvousPairingInvitationAdapter {
    pub fn new(
        endpoint: Arc<Endpoint>,
        device_identity: Arc<dyn DeviceIdentityPort>,
        settings: Arc<dyn SettingsPort>,
        rendezvous: Arc<RendezvousClient>,
    ) -> Self {
        Self {
            endpoint,
            device_identity,
            settings,
            rendezvous,
        }
    }

    async fn resolve_device_name(&self) -> Result<String, InvitationError> {
        let settings = self
            .settings
            .load()
            .await
            .map_err(|err| InvitationError::Internal(format!("settings load failed: {err}")))?;
        settings
            .general
            .device_name
            .filter(|n| !n.trim().is_empty())
            .ok_or_else(|| {
                InvitationError::Internal(
                    "device_name missing from settings; user must set it before pairing"
                        .to_string(),
                )
            })
    }

    fn serialize_ticket(&self) -> Result<(String, String), InvitationError> {
        let addr = self.endpoint.addr();
        if addr.addrs.is_empty() {
            // No relay, no direct addrs — endpoint is bound but has no way
            // to be contacted. Surface as NetworkNotStarted so UI tells the
            // user to wait / retry.
            return Err(InvitationError::NetworkNotStarted);
        }
        let endpoint_id = addr.id.to_string();
        let ticket = serde_json::to_string(&addr)
            .map_err(|err| InvitationError::Internal(format!("endpoint addr serialize: {err}")))?;
        Ok((endpoint_id, ticket))
    }
}

#[async_trait]
impl PairingInvitationPort for RendezvousPairingInvitationAdapter {
    #[instrument(skip_all)]
    async fn issue_invitation(&self) -> Result<IssuedInvitation, InvitationError> {
        let (endpoint_id, ticket) = self.serialize_ticket()?;
        let device_name = self.resolve_device_name().await?;
        let device_id = self.device_identity.current_device_id();

        let req = CreatePairingRequest {
            sponsor_device_id: device_id.as_str().to_string(),
            sponsor_device_name: device_name,
            sponsor_endpoint_id: endpoint_id,
            sponsor_ticket: ticket,
            ttl_secs: None,
        };

        let parsed = self
            .rendezvous
            .create_pairing(&req)
            .await
            .map_err(map_create_err)?;

        let code = InvitationCode::new(parsed.code);
        let expires_at = Utc
            .timestamp_millis_opt(parsed.expires_at_ms)
            .single()
            .ok_or_else(|| {
                InvitationError::Internal(format!(
                    "rendezvous returned invalid expires_at_ms: {}",
                    parsed.expires_at_ms
                ))
            })?;

        debug!(%expires_at, "rendezvous invitation issued");
        Ok(IssuedInvitation { code, expires_at })
    }

    #[instrument(skip(self), fields(code = %code.as_str()))]
    async fn consume_invitation(
        &self,
        code: &InvitationCode,
    ) -> Result<(), ConsumeInvitationError> {
        match self.rendezvous.consume_pairing(code.as_str()).await {
            Ok(()) => {
                debug!("rendezvous invitation consumed");
                Ok(())
            }
            Err(err) => Err(map_consume_err(err)),
        }
    }
}

// ── Error mappers ───────────────────────────────────────────────────────────

fn map_create_err(err: RendezvousHttpError) -> InvitationError {
    match err {
        // Transport + 5xx both mean "try again later" from the caller's POV.
        RendezvousHttpError::Transport(_) | RendezvousHttpError::ServiceUnavailable(_) => {
            InvitationError::ServiceUnavailable
        }
        // A create call hitting 404/410/409 would mean the rendezvous API
        // contract broke (create isn't keyed by a code on the client side).
        // Report as Internal so the anomaly is visible in logs.
        RendezvousHttpError::NotFound
        | RendezvousHttpError::Gone
        | RendezvousHttpError::Conflict => {
            InvitationError::Internal(format!("rendezvous create: unexpected {err}"))
        }
        RendezvousHttpError::Unexpected { status, slug } => InvitationError::Internal(format!(
            "rendezvous rejected create ({status}, slug={slug})"
        )),
        RendezvousHttpError::Parse(msg) => {
            InvitationError::Internal(format!("rendezvous response parse: {msg}"))
        }
    }
}

fn map_consume_err(err: RendezvousHttpError) -> ConsumeInvitationError {
    match err {
        // Server groups `pairing_not_found` + `pairing_expired` under 404,
        // and `pairing_already_consumed` under 409. Caller treats all three
        // as benign (the code's lifecycle is already terminal), so they
        // collapse to NotFound.
        RendezvousHttpError::NotFound
        | RendezvousHttpError::Gone
        | RendezvousHttpError::Conflict => ConsumeInvitationError::NotFound,
        RendezvousHttpError::Transport(_) | RendezvousHttpError::ServiceUnavailable(_) => {
            ConsumeInvitationError::ServiceUnavailable
        }
        RendezvousHttpError::Unexpected { status, slug } => ConsumeInvitationError::Internal(
            format!("rendezvous rejected consume ({status}, slug={slug})"),
        ),
        RendezvousHttpError::Parse(msg) => {
            ConsumeInvitationError::Internal(format!("rendezvous response parse: {msg}"))
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;

    use async_trait::async_trait;
    use chrono::DateTime;
    use serde_json::json;
    use uc_core::ids::DeviceId;
    use uc_core::settings::model::Settings;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn utc_from_ms(ms: i64) -> DateTime<Utc> {
        Utc.timestamp_millis_opt(ms).single().unwrap()
    }

    struct FakeDeviceIdentity(DeviceId);
    impl DeviceIdentityPort for FakeDeviceIdentity {
        fn current_device_id(&self) -> DeviceId {
            self.0.clone()
        }
    }

    struct InMemorySettings {
        inner: StdMutex<Settings>,
    }
    impl InMemorySettings {
        fn with_device_name(name: Option<&str>) -> Arc<Self> {
            let mut s = Settings::default();
            s.general.device_name = name.map(String::from);
            Arc::new(Self {
                inner: StdMutex::new(s),
            })
        }
    }
    #[async_trait]
    impl SettingsPort for InMemorySettings {
        async fn load(&self) -> anyhow::Result<Settings> {
            Ok(self.inner.lock().unwrap().clone())
        }
        async fn save(&self, s: &Settings) -> anyhow::Result<()> {
            *self.inner.lock().unwrap() = s.clone();
            Ok(())
        }
    }

    async fn loopback_endpoint() -> Arc<Endpoint> {
        // Loopback-only endpoint: no relay, no external discovery. That
        // leaves a single direct IPv4 address in `EndpointAddr.addrs`, which
        // is enough to satisfy `serialize_ticket`'s "addrs non-empty" guard.
        let ep = Endpoint::builder(iroh::endpoint::presets::N0)
            .relay_mode(iroh::RelayMode::Disabled)
            .bind()
            .await
            .expect("bind loopback endpoint");
        Arc::new(ep)
    }

    fn make_adapter(
        endpoint: Arc<Endpoint>,
        settings: Arc<dyn SettingsPort>,
        base_url: impl Into<String>,
    ) -> RendezvousPairingInvitationAdapter {
        RendezvousPairingInvitationAdapter::new(
            endpoint,
            Arc::new(FakeDeviceIdentity(DeviceId::new("device-a"))),
            settings,
            Arc::new(RendezvousClient::with_base_url(base_url)),
        )
    }

    // ── issue_invitation ─────────────────────────────────────────────────

    #[tokio::test]
    async fn issue_invitation_happy_path() {
        let ep = loopback_endpoint().await;
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/pairings"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "code": "ABCD-EFGH",
                "expiresAtMs": 1_700_000_000_000_i64,
            })))
            .expect(1)
            .mount(&server)
            .await;

        let adapter = make_adapter(
            ep,
            InMemorySettings::with_device_name(Some("mac")),
            server.uri(),
        );
        let issued = adapter.issue_invitation().await.expect("happy path");

        assert_eq!(issued.code.as_str(), "ABCD-EFGH");
        assert_eq!(issued.expires_at, utc_from_ms(1_700_000_000_000));
    }

    #[tokio::test]
    async fn issue_invitation_includes_required_body_fields() {
        use wiremock::matchers::body_partial_json;

        let ep = loopback_endpoint().await;
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/pairings"))
            .and(body_partial_json(json!({
                "sponsorDeviceId": "device-a",
                "sponsorDeviceName": "mac",
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "code": "ABCD-EFGH",
                "expiresAtMs": 1_700_000_000_000_i64,
            })))
            .expect(1)
            .mount(&server)
            .await;

        let adapter = make_adapter(
            ep,
            InMemorySettings::with_device_name(Some("mac")),
            server.uri(),
        );
        adapter.issue_invitation().await.expect("body matches");
    }

    #[tokio::test]
    async fn issue_invitation_maps_5xx_to_service_unavailable() {
        let ep = loopback_endpoint().await;
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/pairings"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;

        let adapter = make_adapter(
            ep,
            InMemorySettings::with_device_name(Some("mac")),
            server.uri(),
        );
        let err = adapter.issue_invitation().await.unwrap_err();
        assert!(matches!(err, InvitationError::ServiceUnavailable));
    }

    #[tokio::test]
    async fn issue_invitation_maps_4xx_to_internal_with_slug() {
        let ep = loopback_endpoint().await;
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/pairings"))
            .respond_with(ResponseTemplate::new(400).set_body_json(json!({
                "error": { "code": "invalid_request" }
            })))
            .mount(&server)
            .await;

        let adapter = make_adapter(
            ep,
            InMemorySettings::with_device_name(Some("mac")),
            server.uri(),
        );
        let err = adapter.issue_invitation().await.unwrap_err();
        let msg = match err {
            InvitationError::Internal(m) => m,
            other => panic!("expected Internal, got {other:?}"),
        };
        assert!(msg.contains("invalid_request"), "msg was {msg}");
        assert!(msg.contains("400"));
    }

    #[tokio::test]
    async fn issue_invitation_maps_malformed_response_to_internal() {
        let ep = loopback_endpoint().await;
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/pairings"))
            .respond_with(ResponseTemplate::new(200).set_body_string("not-json"))
            .mount(&server)
            .await;

        let adapter = make_adapter(
            ep,
            InMemorySettings::with_device_name(Some("mac")),
            server.uri(),
        );
        let err = adapter.issue_invitation().await.unwrap_err();
        assert!(
            matches!(err, InvitationError::Internal(ref m) if m.contains("parse")),
            "got {err:?}"
        );
    }

    #[tokio::test]
    async fn issue_invitation_maps_transport_failure_to_service_unavailable() {
        let ep = loopback_endpoint().await;
        // Point at a port guaranteed to reject — no server running there.
        let adapter = make_adapter(
            ep,
            InMemorySettings::with_device_name(Some("mac")),
            "http://127.0.0.1:1",
        );
        let err = adapter.issue_invitation().await.unwrap_err();
        assert!(matches!(err, InvitationError::ServiceUnavailable));
    }

    #[tokio::test]
    async fn issue_invitation_rejects_missing_device_name() {
        let ep = loopback_endpoint().await;
        let server = MockServer::start().await;
        // Even if the server would accept, we should short-circuit before
        // sending — assert no request by leaving no `.mount(&server)`.
        let adapter = make_adapter(ep, InMemorySettings::with_device_name(None), server.uri());
        let err = adapter.issue_invitation().await.unwrap_err();
        let msg = match err {
            InvitationError::Internal(m) => m,
            other => panic!("expected Internal, got {other:?}"),
        };
        assert!(msg.contains("device_name"), "msg was {msg}");
    }

    #[tokio::test]
    async fn issue_invitation_maps_invalid_expires_at_to_internal() {
        let ep = loopback_endpoint().await;
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/pairings"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "code": "ABCD-EFGH",
                "expiresAtMs": i64::MAX,  // out-of-range for chrono
            })))
            .mount(&server)
            .await;

        let adapter = make_adapter(
            ep,
            InMemorySettings::with_device_name(Some("mac")),
            server.uri(),
        );
        let err = adapter.issue_invitation().await.unwrap_err();
        assert!(
            matches!(err, InvitationError::Internal(ref m) if m.contains("expires_at_ms")),
            "got {err:?}"
        );
    }

    // ── consume_invitation ───────────────────────────────────────────────

    #[tokio::test]
    async fn consume_invitation_happy_path_is_204_ok() {
        use wiremock::matchers::body_partial_json;
        let ep = loopback_endpoint().await;
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/pairings/consume"))
            .and(body_partial_json(json!({ "code": "ABCD-EFGH" })))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&server)
            .await;

        let adapter = make_adapter(
            ep,
            InMemorySettings::with_device_name(Some("mac")),
            server.uri(),
        );
        adapter
            .consume_invitation(&InvitationCode::new("ABCD-EFGH"))
            .await
            .expect("consume happy path");
    }

    #[tokio::test]
    async fn consume_invitation_maps_404_to_not_found() {
        let ep = loopback_endpoint().await;
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/pairings/consume"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;

        let adapter = make_adapter(
            ep,
            InMemorySettings::with_device_name(Some("mac")),
            server.uri(),
        );
        let err = adapter
            .consume_invitation(&InvitationCode::new("GONE-CODE"))
            .await
            .unwrap_err();
        assert!(matches!(err, ConsumeInvitationError::NotFound));
    }

    #[tokio::test]
    async fn consume_invitation_maps_409_to_not_found() {
        // Server returns 409 `pairing_already_consumed` when a second
        // sponsor client wins the race. Caller treats as benign, so we
        // collapse to NotFound like the existing expired branch.
        let ep = loopback_endpoint().await;
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/pairings/consume"))
            .respond_with(ResponseTemplate::new(409))
            .mount(&server)
            .await;

        let adapter = make_adapter(
            ep,
            InMemorySettings::with_device_name(Some("mac")),
            server.uri(),
        );
        let err = adapter
            .consume_invitation(&InvitationCode::new("OLD-CODE"))
            .await
            .unwrap_err();
        assert!(matches!(err, ConsumeInvitationError::NotFound));
    }

    #[tokio::test]
    async fn consume_invitation_maps_5xx_to_service_unavailable() {
        let ep = loopback_endpoint().await;
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/pairings/consume"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;

        let adapter = make_adapter(
            ep,
            InMemorySettings::with_device_name(Some("mac")),
            server.uri(),
        );
        let err = adapter
            .consume_invitation(&InvitationCode::new("BUSY-CODE"))
            .await
            .unwrap_err();
        assert!(matches!(err, ConsumeInvitationError::ServiceUnavailable));
    }

    #[tokio::test]
    async fn consume_invitation_maps_transport_failure_to_service_unavailable() {
        let ep = loopback_endpoint().await;
        let adapter = make_adapter(
            ep,
            InMemorySettings::with_device_name(Some("mac")),
            "http://127.0.0.1:1",
        );
        let err = adapter
            .consume_invitation(&InvitationCode::new("ANY"))
            .await
            .unwrap_err();
        assert!(matches!(err, ConsumeInvitationError::ServiceUnavailable));
    }

    #[tokio::test]
    async fn consume_invitation_maps_other_4xx_to_internal_with_slug() {
        let ep = loopback_endpoint().await;
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/pairings/consume"))
            .respond_with(ResponseTemplate::new(400).set_body_json(json!({
                "error": { "code": "malformed_code" }
            })))
            .mount(&server)
            .await;

        let adapter = make_adapter(
            ep,
            InMemorySettings::with_device_name(Some("mac")),
            server.uri(),
        );
        let err = adapter
            .consume_invitation(&InvitationCode::new("WEIRD"))
            .await
            .unwrap_err();
        let msg = match err {
            ConsumeInvitationError::Internal(m) => m,
            other => panic!("expected Internal, got {other:?}"),
        };
        assert!(msg.contains("malformed_code"), "msg was {msg}");
        assert!(msg.contains("400"));
    }
}
