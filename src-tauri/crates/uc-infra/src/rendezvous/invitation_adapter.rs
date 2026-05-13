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

use std::net::IpAddr;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{TimeZone, Utc};
use iroh::{Endpoint, EndpointAddr, TransportAddr};
use tracing::{debug, instrument};

use uc_core::pairing::invitation::InvitationCode;
use uc_core::ports::{
    ConsumeInvitationError, DeviceIdentityPort, InvitationError, IssuedInvitation,
    PairingInvitationAddressCandidate, PairingInvitationAddressQueryPort,
    PairingInvitationByAddressPort, PairingInvitationPort, SettingsPort,
};
use uc_core::settings::model::Settings;

use crate::network::iroh::filter_endpoint_addr;

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

    async fn load_settings(&self) -> Result<Settings, InvitationError> {
        self.settings
            .load()
            .await
            .map_err(|err| InvitationError::Internal(format!("settings load failed: {err}")))
    }

    fn resolve_device_name(settings: &Settings) -> Result<String, InvitationError> {
        settings
            .general
            .device_name
            .as_ref()
            .filter(|n| !n.trim().is_empty())
            .cloned()
            .ok_or_else(|| {
                InvitationError::Internal(
                    "device_name missing from settings; user must set it before pairing"
                        .to_string(),
                )
            })
    }

    fn serialize_ticket(&self, allow_overlay: bool) -> Result<(String, String), InvitationError> {
        serialize_filtered_endpoint_ticket(self.endpoint.addr(), allow_overlay)
    }

    fn serialize_ticket_for_ip(
        &self,
        allow_overlay: bool,
        selected_ip: IpAddr,
    ) -> Result<(String, String), InvitationError> {
        serialize_endpoint_ticket_for_ip(self.endpoint.addr(), allow_overlay, selected_ip)
    }

    async fn create_pairing_with_ticket(
        &self,
        settings: Settings,
        endpoint_id: String,
        ticket: String,
    ) -> Result<IssuedInvitation, InvitationError> {
        let device_name = Self::resolve_device_name(&settings)?;
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
}

fn serialize_filtered_endpoint_ticket(
    addr: EndpointAddr,
    allow_overlay: bool,
) -> Result<(String, String), InvitationError> {
    let addr = filter_endpoint_addr(addr, allow_overlay);
    if addr.addrs.is_empty() {
        return Err(InvitationError::NetworkNotStarted);
    }
    serialize_endpoint_addr(addr)
}

/// Build a ticket that only carries the single address matching
/// `selected_ip`.
///
/// **Ordering matters**: the product address filter runs *first*, so an
/// IP that the filter drops (overlay-network rules with
/// `allow_overlay=false`, link-local, Clash fake-ip 198.18.0.0/15) will
/// surface as `AddressNotAvailable` rather than slipping into the ticket.
/// This is intentional — the dev tool reuses the product filter so
/// observing "what gets published if we pick this IP" stays aligned with
/// the runtime that production peers see.
fn serialize_endpoint_ticket_for_ip(
    addr: EndpointAddr,
    allow_overlay: bool,
    selected_ip: IpAddr,
) -> Result<(String, String), InvitationError> {
    let addr = filter_endpoint_addr(addr, allow_overlay);
    let EndpointAddr { id, addrs } = addr;
    let selected: Vec<TransportAddr> = addrs
        .into_iter()
        .filter(|addr| match addr {
            TransportAddr::Ip(socket) => socket.ip() == selected_ip,
            _ => false,
        })
        .collect();
    if selected.is_empty() {
        return Err(InvitationError::AddressNotAvailable(selected_ip));
    }
    serialize_endpoint_addr(EndpointAddr::from_parts(id, selected))
}

fn list_invitation_address_candidates(
    addr: EndpointAddr,
    allow_overlay: bool,
) -> Result<Vec<PairingInvitationAddressCandidate>, InvitationError> {
    let addr = filter_endpoint_addr(addr, allow_overlay);
    let candidates: Vec<PairingInvitationAddressCandidate> = addr
        .ip_addrs()
        .map(|socket| PairingInvitationAddressCandidate {
            ip: socket.ip(),
            port: socket.port(),
        })
        .collect();
    if candidates.is_empty() {
        return Err(InvitationError::NetworkNotStarted);
    }
    Ok(candidates)
}

fn serialize_endpoint_addr(addr: EndpointAddr) -> Result<(String, String), InvitationError> {
    let endpoint_id = addr.id.to_string();
    let ticket = serde_json::to_string(&addr)
        .map_err(|err| InvitationError::Internal(format!("endpoint addr serialize: {err}")))?;
    Ok((endpoint_id, ticket))
}

#[async_trait]
impl PairingInvitationPort for RendezvousPairingInvitationAdapter {
    #[instrument(skip_all)]
    async fn issue_invitation(&self) -> Result<IssuedInvitation, InvitationError> {
        let settings = self.load_settings().await?;
        let (endpoint_id, ticket) =
            self.serialize_ticket(settings.network.allow_overlay_network_addrs)?;
        self.create_pairing_with_ticket(settings, endpoint_id, ticket)
            .await
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

#[async_trait]
impl PairingInvitationAddressQueryPort for RendezvousPairingInvitationAdapter {
    #[instrument(skip_all)]
    async fn list_invitation_addresses(
        &self,
    ) -> Result<Vec<PairingInvitationAddressCandidate>, InvitationError> {
        let settings = self.load_settings().await?;
        list_invitation_address_candidates(
            self.endpoint.addr(),
            settings.network.allow_overlay_network_addrs,
        )
    }
}

#[async_trait]
impl PairingInvitationByAddressPort for RendezvousPairingInvitationAdapter {
    #[instrument(skip_all, fields(selected_ip = %selected_ip))]
    async fn issue_invitation_for_address(
        &self,
        selected_ip: IpAddr,
    ) -> Result<IssuedInvitation, InvitationError> {
        let settings = self.load_settings().await?;
        let (endpoint_id, ticket) = self
            .serialize_ticket_for_ip(settings.network.allow_overlay_network_addrs, selected_ip)?;
        self.create_pairing_with_ticket(settings, endpoint_id, ticket)
            .await
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
    use std::net::SocketAddr;
    use std::sync::Mutex as StdMutex;

    use async_trait::async_trait;
    use chrono::DateTime;
    use iroh::{EndpointAddr, SecretKey, TransportAddr};
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

    #[test]
    fn serialize_ticket_filters_bad_virtual_addrs_but_keeps_allowed_overlay() {
        let addr = EndpointAddr::from_parts(
            SecretKey::generate().public(),
            [
                TransportAddr::Ip("100.79.191.42:61743".parse::<SocketAddr>().unwrap()),
                TransportAddr::Ip("198.18.0.1:61743".parse::<SocketAddr>().unwrap()),
                TransportAddr::Ip("169.254.1.2:61743".parse::<SocketAddr>().unwrap()),
                TransportAddr::Ip("192.168.31.72:61743".parse::<SocketAddr>().unwrap()),
            ],
        );

        let (_, ticket) = serialize_filtered_endpoint_ticket(addr, true).expect("ticket");
        let decoded: EndpointAddr = serde_json::from_str(&ticket).expect("decode ticket");
        let ips: Vec<String> = decoded
            .ip_addrs()
            .map(|addr| addr.ip().to_string())
            .collect();

        assert!(ips.contains(&"100.79.191.42".to_string()));
        assert!(ips.contains(&"192.168.31.72".to_string()));
        assert!(!ips.contains(&"198.18.0.1".to_string()));
        assert!(!ips.contains(&"169.254.1.2".to_string()));
    }

    #[test]
    fn serialize_ticket_for_selected_ip_keeps_only_that_ip() {
        let selected_ip = "100.79.191.42".parse().unwrap();
        let addr = EndpointAddr::from_parts(
            SecretKey::generate().public(),
            [
                TransportAddr::Ip("100.79.191.42:61743".parse::<SocketAddr>().unwrap()),
                TransportAddr::Ip("192.168.31.72:61743".parse::<SocketAddr>().unwrap()),
            ],
        );

        let (_, ticket) =
            serialize_endpoint_ticket_for_ip(addr, true, selected_ip).expect("ticket");
        let decoded: EndpointAddr = serde_json::from_str(&ticket).expect("decode ticket");
        let sockets: Vec<SocketAddr> = decoded.ip_addrs().copied().collect();

        assert_eq!(
            sockets,
            vec!["100.79.191.42:61743".parse::<SocketAddr>().unwrap()]
        );
    }

    #[test]
    fn serialize_ticket_for_selected_ip_rejects_absent_ip() {
        let selected_ip = "100.79.191.42".parse().unwrap();
        let addr = EndpointAddr::from_parts(
            SecretKey::generate().public(),
            [TransportAddr::Ip(
                "192.168.31.72:61743".parse::<SocketAddr>().unwrap(),
            )],
        );

        let err = serialize_endpoint_ticket_for_ip(addr, true, selected_ip).unwrap_err();
        assert!(matches!(
            err,
            InvitationError::AddressNotAvailable(ip) if ip == selected_ip
        ));
    }

    #[test]
    fn list_invitation_address_candidates_uses_ticket_filter() {
        let addr = EndpointAddr::from_parts(
            SecretKey::generate().public(),
            [
                TransportAddr::Ip("100.79.191.42:61743".parse::<SocketAddr>().unwrap()),
                TransportAddr::Ip("198.18.0.1:61743".parse::<SocketAddr>().unwrap()),
                TransportAddr::Ip("192.168.31.72:61743".parse::<SocketAddr>().unwrap()),
            ],
        );

        let candidates = list_invitation_address_candidates(addr, true).expect("candidates");
        let rendered: Vec<String> = candidates
            .iter()
            .map(|candidate| format!("{}:{}", candidate.ip, candidate.port))
            .collect();

        assert_eq!(
            rendered,
            vec![
                "100.79.191.42:61743".to_string(),
                "192.168.31.72:61743".to_string(),
            ]
        );
    }

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
