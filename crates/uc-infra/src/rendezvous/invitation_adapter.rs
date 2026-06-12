//! Sponsor-side port adapter for [`PairingInvitationPort`].
//!
//! Drives two discovery channels concurrently per invitation:
//!
//! 1. **Cloud channel** — `RendezvousClient` POST. Best-effort; failure
//!    here means "cross-network joiners can't resolve via this code,"
//!    not "issue fails."
//! 2. **LAN channel** — window-scoped `MdnsPairingPublisher` instance.
//!    Started for every issued code; dropped on `consume_invitation` or
//!    when the adapter is dropped.
//!
//! Code provenance: when the cloud channel returns Ok, we adopt the
//! server-minted code (back-compat with the legacy "server is the
//! issuing authority" flow). When the cloud channel fails, we fall back
//! to local mint — that's the first-pair-no-WAN path. A future
//! migration (path 4-C in plan notes) will add `proposed_code` to the
//! cloud request and always pass the locally minted value, at which
//! point the conditional disappears.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Duration as ChronoDuration, TimeZone, Utc};
use iroh::{Endpoint, EndpointAddr, TransportAddr};
use tokio::runtime::Handle as RuntimeHandle;
use tokio::sync::Mutex;
use tracing::{debug, instrument, warn};

use uc_core::pairing::invitation::InvitationCode;
use uc_core::ports::{
    CodeOrigin, ConsumeInvitationError, DeviceIdentityPort, InvitationError, IssuedInvitation,
    PairingInvitationAddressCandidate, PairingInvitationAddressQueryPort,
    PairingInvitationByAddressPort, PairingInvitationPort, SettingsPort,
};
use uc_core::settings::model::Settings;

use crate::network::iroh::filter_endpoint_addr;
use crate::network::iroh::runtime_consts;
use crate::pairing::{mint_invitation_code, MdnsPairingPublisher, PublisherHandle};

use super::client::{CreatePairingRequest, RendezvousClient, RendezvousHttpError};

/// TTL used when minting a code locally (cloud channel was unreachable).
/// Matches the typical TTL the rendezvous service returns for back-compat.
const LOCAL_MINT_TTL: ChronoDuration = ChronoDuration::seconds(300);

/// Rendezvous-backed adapter for [`PairingInvitationPort`].
///
/// Maintains a per-code map of live mDNS publisher handles so
/// `consume_invitation` can deterministically stop the LAN announce
/// without relying on TTL expiry.
pub struct RendezvousPairingInvitationAdapter {
    endpoint: Arc<Endpoint>,
    device_identity: Arc<dyn DeviceIdentityPort>,
    settings: Arc<dyn SettingsPort>,
    rendezvous: Arc<RendezvousClient>,
    /// Per-code live mDNS publisher handles plus their TTL. Dropping a
    /// value stops the underlying `swarm-discovery` announce thread.
    /// The TTL is used by [`Self::gc_expired_publishers`] (lazy GC,
    /// called on every issue / consume) so a publisher whose code
    /// expired without being consumed still gets released — no
    /// background timer needed.
    publishers: Mutex<HashMap<InvitationCode, (PublisherHandle, DateTime<Utc>)>>,
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
            publishers: Mutex::new(HashMap::new()),
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
            sponsor_endpoint_id: endpoint_id.clone(),
            sponsor_ticket: ticket.clone(),
            ttl_secs: None,
        };

        // ── Cloud channel (best-effort, gated by LAN-only mode) ────────
        // In LAN-only mode the user has explicitly opted out of cloud
        // discovery. We skip the cloud channel entirely (no HTTP request,
        // no metadata leak to the directory service) and mint the code
        // locally — the LAN channel will be the only publish surface.
        if runtime_consts::lan_only() {
            let code = InvitationCode::new(mint_invitation_code());
            let expires_at = Utc::now() + LOCAL_MINT_TTL;
            debug!(
                code = %code.as_str(),
                %expires_at,
                "LAN-only mode: minted invitation locally, skipping cloud channel"
            );
            if let Err(err) = self
                .start_mdns_publisher(&code, &endpoint_id, &ticket, expires_at)
                .await
            {
                // mDNS is the only publish surface in LAN-only mode, so a
                // start failure means zero channels were initiated. The port
                // contract requires `Ok` only when at least one channel is
                // live, so surface the failure instead of returning an
                // undialable code.
                warn!(
                    error = %err,
                    code = %code.as_str(),
                    "mDNS publisher start failed in LAN-only mode; this invitation cannot be discovered",
                );
                return Err(InvitationError::Internal(format!(
                    "mDNS publisher start failed in LAN-only mode: {err}"
                )));
            }
            return Ok(IssuedInvitation {
                code,
                expires_at,
                code_origin: CodeOrigin::LocallyMintedLanOnly,
            });
        }

        let (code, expires_at, cloud_ok) = match self.rendezvous.create_pairing(&req).await {
            Ok(parsed) => {
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
                debug!(%expires_at, "cloud channel issued invitation");
                (code, expires_at, true)
            }
            Err(err) => {
                // Cloud unreachable: local mint + mDNS only.
                // Critical: we only fall back for transient/transport
                // errors. Unexpected status codes still surface as
                // Internal so production anomalies stay loud.
                if !is_cloud_recoverable(&err) {
                    return Err(map_create_err(err));
                }
                let code = InvitationCode::new(mint_invitation_code());
                let expires_at = Utc::now() + LOCAL_MINT_TTL;
                warn!(
                    error = %err,
                    code = %code.as_str(),
                    "cloud channel unreachable; minted invitation locally — only LAN joiners will resolve",
                );
                (code, expires_at, false)
            }
        };

        // ── LAN channel (best-effort, window-scoped) ───────────────────
        if let Err(err) = self
            .start_mdns_publisher(&code, &endpoint_id, &ticket, expires_at)
            .await
        {
            warn!(
                error = %err,
                code = %code.as_str(),
                cloud_ok,
                "mDNS publisher start failed; LAN joiners will not resolve via this code",
            );
            // If the cloud channel also failed (local-mint fallback), mDNS
            // was the only remaining surface — zero channels initiated. The
            // port contract requires `Ok` only when at least one channel is
            // live, so surface the failure. When `cloud_ok` is true the cloud
            // channel still resolves the code, so the warning above suffices.
            if !cloud_ok {
                return Err(InvitationError::Internal(format!(
                    "all discovery channels failed: cloud unreachable and mDNS start failed: {err}"
                )));
            }
        }

        let code_origin = if cloud_ok {
            CodeOrigin::DirectoryIssued
        } else {
            CodeOrigin::LocallyMintedDirectoryUnreachable
        };
        Ok(IssuedInvitation {
            code,
            expires_at,
            code_origin,
        })
    }

    /// Starts a window-scoped mDNS publisher and stores its handle so
    /// `consume_invitation` can drop it later.
    ///
    /// The mDNS ticket is encoded as `hex(postcard(EndpointAddr))`, not
    /// reusing the JSON form fed to the cloud channel: a single TXT
    /// attribute can carry at most 254 bytes including the key prefix,
    /// and JSON-encoded endpoints with 4+ candidate addresses overflow
    /// that limit (observed in the LAN-only e2e test). postcard cuts
    /// the byte count by ~60% and hex doubling still keeps room for
    /// realistic NodeId + LAN IPs.
    async fn start_mdns_publisher(
        &self,
        code: &InvitationCode,
        endpoint_id: &str,
        ticket_json: &str,
        expires_at: DateTime<Utc>,
    ) -> Result<(), String> {
        // Sweep stale handles before inserting; a sponsor that has
        // issued multiple codes without consuming them otherwise leaks
        // multicast sockets until process exit.
        self.gc_expired_publishers(Utc::now()).await;

        // Re-encode the EndpointAddr in the cloud-channel JSON ticket
        // into postcard+hex so it fits in a single TXT attribute. See
        // [`start_mdns_publisher`] doc for the size analysis.
        let ticket_hex = encode_mdns_ticket(ticket_json)?;
        // Pick the iroh endpoint's UDP port for the announce. The LAN IPs
        // we publish come from `if-addrs` inside the publisher, not from
        // iroh's filtered list — that keeps the publisher's "what to put
        // on the wire" identical regardless of which `TransportAddr`
        // variants iroh happens to surface today.
        let port = pick_endpoint_port(&self.endpoint.addr())?;
        let handle = MdnsPairingPublisher::start(
            &RuntimeHandle::current(),
            code.as_str(),
            endpoint_id,
            &ticket_hex,
            expires_at.timestamp_millis(),
            port,
        )
        .map_err(|err| err.to_string())?;
        self.publishers
            .lock()
            .await
            .insert(code.clone(), (handle, expires_at));
        Ok(())
    }

    /// Drops publisher handles whose codes have expired. Called lazily
    /// on every issue / consume — sufficient because a sponsor that
    /// stops issuing also stops needing the GC, and a sponsor that
    /// keeps issuing sweeps as a side effect of normal operation.
    async fn gc_expired_publishers(&self, now: DateTime<Utc>) {
        let mut map = self.publishers.lock().await;
        let before = map.len();
        map.retain(|_code, (_handle, exp)| *exp > now);
        let removed = before.saturating_sub(map.len());
        if removed > 0 {
            debug!(
                removed,
                remaining = map.len(),
                "mDNS publisher GC swept expired handles"
            );
        }
    }
}

/// Re-encode the cloud-channel JSON ticket as `hex(postcard(EndpointAddr))`
/// for mDNS publishing. Compact enough to fit in a single TXT attribute
/// (under 254 bytes total including the `tk=` key) for realistic
/// EndpointAddrs (up to ~4 IPs + relay URL).
fn encode_mdns_ticket(ticket_json: &str) -> Result<String, String> {
    let addr: EndpointAddr = serde_json::from_str(ticket_json)
        .map_err(|err| format!("ticket JSON decode for mDNS re-encode: {err}"))?;
    let bytes = postcard::to_allocvec(&addr)
        .map_err(|err| format!("ticket postcard encode for mDNS: {err}"))?;
    Ok(hex::encode(bytes))
}

/// Cloud-side errors we treat as "try LAN-only instead." Transport
/// failures, 5xx responses, and parse errors all qualify — they indicate
/// the service is unreachable or misbehaving, not that the request was
/// semantically wrong. 4xx codes (Conflict / NotFound on create — which
/// shouldn't normally happen) surface as Internal so the anomaly is
/// visible in logs.
fn is_cloud_recoverable(err: &RendezvousHttpError) -> bool {
    matches!(
        err,
        RendezvousHttpError::Transport(_)
            | RendezvousHttpError::ServiceUnavailable(_)
            | RendezvousHttpError::Parse(_)
    )
}

/// Extract the first IP-bound port the endpoint surfaces. Used by the
/// mDNS publisher as the announced service port — joiners will connect
/// to whatever port maps to whichever IP they pick.
///
/// Real iroh endpoints always have at least one IP `TransportAddr`
/// online by the time we're issuing invitations, so the `None` case is
/// a defensive guard for tests / very early init.
fn pick_endpoint_port(addr: &EndpointAddr) -> Result<u16, String> {
    addr.addrs
        .iter()
        .find_map(|a| match a {
            TransportAddr::Ip(sa) => Some(sa.port()),
            _ => None,
        })
        .ok_or_else(|| "endpoint exposes no IP transport addresses".to_string())
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
        // Stop the LAN announce immediately (deterministic, not TTL-bound).
        // Dropping the handle stops `swarm-discovery`'s actor; this also
        // releases the multicast socket so a follow-up `issue_invitation`
        // can bind fresh. Also sweep any stale entries piggy-backed on
        // this call.
        self.gc_expired_publishers(Utc::now()).await;
        if self.publishers.lock().await.remove(code).is_some() {
            debug!("mDNS publisher stopped for consumed invitation");
        }

        match self.rendezvous.consume_pairing(code.as_str()).await {
            Ok(()) => {
                debug!("cloud channel invitation consumed");
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

    /// Asserts an invitation came from the local-mint fallback path (cloud
    /// channel unreachable / misbehaving) rather than being adopted from a
    /// server response. Two signals distinguish them:
    ///
    /// 1. Shape — `mint_invitation_code` emits `XXXX-XXXX` (9 chars).
    /// 2. TTL — the local path sets `expires_at = now + LOCAL_MINT_TTL`, so
    ///    the value must land in the window bracketed by the call. A
    ///    server-minted expiry would carry the response's own timestamp.
    fn assert_locally_minted(
        issued: &IssuedInvitation,
        before: DateTime<Utc>,
        after: DateTime<Utc>,
    ) {
        let code = issued.code.as_str();
        assert_eq!(code.len(), 9, "local-mint code is XXXX-XXXX, got {code:?}");
        let (left, right) = code.split_once('-').expect("local-mint code has a hyphen");
        assert_eq!(left.len(), 4, "left group of {code:?}");
        assert_eq!(right.len(), 4, "right group of {code:?}");
        assert!(
            issued.expires_at >= before + LOCAL_MINT_TTL
                && issued.expires_at <= after + LOCAL_MINT_TTL,
            "expires_at {} outside local-mint window [{}, {}]",
            issued.expires_at,
            before + LOCAL_MINT_TTL,
            after + LOCAL_MINT_TTL,
        );
        // These tests exercise the recoverable-cloud-failure fallback, so the
        // code's provenance must reflect a directory outage (not LAN-only).
        assert_eq!(
            issued.code_origin,
            CodeOrigin::LocallyMintedDirectoryUnreachable,
            "fallback mint should record a directory-unreachable origin"
        );
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
    async fn issue_invitation_falls_back_to_local_mint_on_5xx() {
        // A 5xx from the cloud channel is recoverable (`is_cloud_recoverable`):
        // the sponsor mints a code locally and announces it on LAN instead of
        // failing. This is the first-pair-no-WAN path — the cloud directory is
        // down, but a same-LAN joiner still resolves the code via mDNS.
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
        let before = Utc::now();
        let issued = adapter
            .issue_invitation()
            .await
            .expect("5xx is recoverable: falls back to local mint");
        let after = Utc::now();

        assert_locally_minted(&issued, before, after);
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
    async fn issue_invitation_falls_back_to_local_mint_on_malformed_response() {
        // A malformed 2xx body surfaces as a Parse error, which
        // `is_cloud_recoverable` treats as "service misbehaving" → fall back
        // to local mint rather than failing the issue. (A 4xx with a slug
        // still maps to Internal — see
        // `issue_invitation_maps_4xx_to_internal_with_slug`.)
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
        let before = Utc::now();
        let issued = adapter
            .issue_invitation()
            .await
            .expect("malformed response is recoverable: falls back to local mint");
        let after = Utc::now();

        assert_locally_minted(&issued, before, after);
    }

    #[tokio::test]
    async fn issue_invitation_falls_back_to_local_mint_on_transport_failure() {
        let ep = loopback_endpoint().await;
        // Point at a port guaranteed to reject — no server running there.
        // Transport failure is recoverable, so the sponsor mints locally
        // rather than surfacing an error.
        let adapter = make_adapter(
            ep,
            InMemorySettings::with_device_name(Some("mac")),
            "http://127.0.0.1:1",
        );
        let before = Utc::now();
        let issued = adapter
            .issue_invitation()
            .await
            .expect("transport failure is recoverable: falls back to local mint");
        let after = Utc::now();

        assert_locally_minted(&issued, before, after);
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
