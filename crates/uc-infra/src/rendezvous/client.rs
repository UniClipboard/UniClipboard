//! Rendezvous HTTP gateway.
//!
//! The rendezvous service is a short-TTL meeting point that lets a joiner
//! convert a human-typed code into a sponsor's iroh ticket. This module
//! owns every HTTP round-trip with the service so every port adapter
//! (sponsor-side invitation + joiner-side dial) shares one [`reqwest::Client`],
//! one User-Agent, one timeout, and one error model.
//!
//! Three endpoints correspond to the three lifecycle moments of a code:
//!
//! * `POST /v1/pairings`          — sponsor registers, rendezvous mints a code
//! * `POST /v1/pairings/resolve`  — joiner redeems the code for the sponsor ticket
//! * `POST /v1/pairings/consume`  — sponsor marks the code terminal after a successful handshake
//!
//! The gateway is **not opinionated** about which HTTP status means what
//! business-wise. It surfaces [`RendezvousHttpError`] variants
//! (`NotFound`, `Gone`, `Conflict`, `ServiceUnavailable`, …) and leaves
//! the business-semantic mapping to each port adapter
//! (`invitation_adapter.rs`, `pairing/session.rs`).
//!
//! Wire contract note: the sponsor's iroh [`iroh::EndpointAddr`] is serialized
//! to JSON and sent as the opaque `sponsorTicket` string — the rendezvous
//! server doesn't parse it, joiners deserialize it on their side.

use std::time::Duration;

use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{debug, warn};

/// Production rendezvous service base URL.
pub const RENDEZVOUS_BASE_URL: &str = "https://rendezvous.uniclipboard.app";

/// Hard cap on rendezvous HTTP requests; the service is behind CF edge and
/// typically answers in < 200 ms, so a short timeout keeps callers
/// responsive when the service degrades.
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);

/// Explicit user agent. `reqwest::Client::builder()` leaves the header
/// blank by default, which Cloudflare's bot-management layer at the
/// rendezvous edge 550-resets on some rules. Sending a stable identifier
/// makes the transport behave like curl/browser clients and is also
/// useful on the server side for telemetry.
const USER_AGENT: &str = concat!("uniclipboard-cli/", env!("CARGO_PKG_VERSION"));

// ============================================================================
// Error type
// ============================================================================

/// Errors surfaced by the rendezvous HTTP gateway. Preserves just enough
/// information for port adapters to map each variant onto their own
/// domain error type (see `invitation_adapter.rs` + `pairing/session.rs`).
#[derive(Debug, Error)]
pub enum RendezvousHttpError {
    /// Transport failure — DNS, TCP, TLS, timeout, reqwest internal.
    #[error("rendezvous transport failed: {0}")]
    Transport(String),

    /// HTTP 404 — no entry for this code (typo or never issued).
    #[error("rendezvous entry not found")]
    NotFound,

    /// HTTP 410 — entry exists but its TTL elapsed.
    #[error("rendezvous entry gone (expired)")]
    Gone,

    /// HTTP 409 — conflicting state (e.g. already consumed by another
    /// sponsor client winning the race).
    #[error("rendezvous entry conflict")]
    Conflict,

    /// HTTP 5xx — service-side transient failure.
    #[error("rendezvous service unavailable ({0})")]
    ServiceUnavailable(StatusCode),

    /// Any other non-2xx status. Carries the server-side `error.code` slug
    /// when the envelope is present, otherwise `"unknown"`.
    #[error("rendezvous unexpected status {status} (slug={slug})")]
    Unexpected { status: StatusCode, slug: String },

    /// 2xx with a body that fails to parse into the expected response shape.
    #[error("rendezvous response parse: {0}")]
    Parse(String),
}

// ============================================================================
// Client
// ============================================================================

/// Shared HTTP gateway to the rendezvous service.
///
/// Wrap in `Arc` and hand the same instance to every port adapter that
/// needs rendezvous — there is no reason for multiple adapters in the
/// same process to build their own reqwest clients.
#[derive(Debug)]
pub struct RendezvousClient {
    http: reqwest::Client,
    base_url: String,
}

impl RendezvousClient {
    /// Production client pointed at [`RENDEZVOUS_BASE_URL`].
    pub fn new() -> Self {
        Self::with_base_url(RENDEZVOUS_BASE_URL)
    }

    /// Client pointed at a custom base URL (mock server, staging,
    /// on-prem). Used by integration tests and the `IrohNodeConfig`
    /// override path.
    pub fn with_base_url(base_url: impl Into<String>) -> Self {
        // reqwest's Client::build() only fails on invalid static TLS
        // config; with our workspace-locked features ("rustls-tls-webpki-roots")
        // that path is unreachable at runtime. A panic here is appropriate:
        // it's a programmer error (wrong feature flags) we want loud.
        let http = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(HTTP_TIMEOUT)
            .build()
            .expect("reqwest client build failed — check uc-infra feature flags");
        Self {
            http,
            base_url: base_url.into(),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url.trim_end_matches('/'), path)
    }

    /// `POST /v1/pairings` — sponsor registers, rendezvous mints a short code.
    pub async fn create_pairing(
        &self,
        req: &CreatePairingRequest,
    ) -> Result<CreatePairingResponse, RendezvousHttpError> {
        let url = self.url("/v1/pairings");
        let resp = self.http.post(&url).json(req).send().await.map_err(|err| {
            warn!(error = %err, chain = %err_chain(&err), "rendezvous create transport failed");
            RendezvousHttpError::Transport(err.to_string())
        })?;
        let status = resp.status();
        if status.is_success() {
            return resp
                .json::<CreatePairingResponse>()
                .await
                .map_err(|err| RendezvousHttpError::Parse(format!("create_pairing: {err}")));
        }
        Err(classify_status(resp, status).await)
    }

    /// `POST /v1/pairings/resolve` — joiner swaps code for sponsor ticket.
    pub async fn resolve_pairing(
        &self,
        code: &str,
    ) -> Result<ResolvePairingResponse, RendezvousHttpError> {
        let url = self.url("/v1/pairings/resolve");
        let body = CodeRequest {
            code: code.to_string(),
        };
        let resp = self
            .http
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|err| {
                debug!(error = %err, "rendezvous resolve transport failed");
                RendezvousHttpError::Transport(err.to_string())
            })?;
        let status = resp.status();
        if status.is_success() {
            return resp
                .json::<ResolvePairingResponse>()
                .await
                .map_err(|err| RendezvousHttpError::Parse(format!("resolve_pairing: {err}")));
        }
        Err(classify_status(resp, status).await)
    }

    /// `POST /v1/pairings/consume` — sponsor marks the code terminal.
    pub async fn consume_pairing(&self, code: &str) -> Result<(), RendezvousHttpError> {
        let url = self.url("/v1/pairings/consume");
        let body = CodeRequest {
            code: code.to_string(),
        };
        let resp = self
            .http
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|err| {
                warn!(error = %err, "rendezvous consume transport failed");
                RendezvousHttpError::Transport(err.to_string())
            })?;
        let status = resp.status();
        if status.is_success() {
            return Ok(());
        }
        Err(classify_status(resp, status).await)
    }
}

impl Default for RendezvousClient {
    fn default() -> Self {
        Self::new()
    }
}

/// Map a non-success response to a [`RendezvousHttpError`]. Consumes the
/// response because the fallback `Unexpected` branch reads the body to
/// extract the error slug.
async fn classify_status(resp: reqwest::Response, status: StatusCode) -> RendezvousHttpError {
    if status.is_server_error() {
        warn!(%status, "rendezvous 5xx");
        return RendezvousHttpError::ServiceUnavailable(status);
    }
    match status {
        StatusCode::NOT_FOUND => RendezvousHttpError::NotFound,
        StatusCode::GONE => RendezvousHttpError::Gone,
        StatusCode::CONFLICT => RendezvousHttpError::Conflict,
        _ => {
            let slug = parse_error_slug(resp).await;
            warn!(%status, %slug, "rendezvous unexpected status");
            RendezvousHttpError::Unexpected { status, slug }
        }
    }
}

async fn parse_error_slug(resp: reqwest::Response) -> String {
    resp.json::<RendezvousErrorEnvelope>()
        .await
        .ok()
        .and_then(|e| e.error.map(|d| d.code))
        .unwrap_or_else(|| "unknown".to_string())
}

/// Walk an error's `Error::source()` chain and render it into a single
/// `A -> B -> C` string. `reqwest::Error`'s `Display` intentionally hides
/// the inner cause, which makes "error sending request for url"
/// impossible to diagnose without the underlying hyper / rustls / io
/// error — this helper surfaces them.
fn err_chain(err: &dyn std::error::Error) -> String {
    let mut parts = vec![err.to_string()];
    let mut source = err.source();
    while let Some(s) = source {
        parts.push(s.to_string());
        source = s.source();
    }
    parts.join(" -> ")
}

// ============================================================================
// Wire types
// ============================================================================

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatePairingRequest {
    pub sponsor_device_id: String,
    pub sponsor_device_name: String,
    pub sponsor_endpoint_id: String,
    pub sponsor_ticket: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttl_secs: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatePairingResponse {
    pub code: String,
    pub expires_at_ms: i64,
}

/// Response body for `POST /v1/pairings/resolve`. Extra fields the server
/// sends (`sponsorEndpointId`, `expiresAtMs`) are deliberately not
/// mapped — serde drops unknown fields by default. Callers only need
/// `sponsor_ticket` (the opaque iroh endpoint address, JSON-encoded).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvePairingResponse {
    pub sponsor_ticket: String,
}

/// Request body for both `/v1/pairings/resolve` and `/v1/pairings/consume`.
/// The server expects a JSON object with a single `code` string field.
#[derive(Debug, Serialize)]
struct CodeRequest {
    code: String,
}

#[derive(Debug, Deserialize)]
struct RendezvousErrorEnvelope {
    error: Option<RendezvousErrorDetail>,
}

#[derive(Debug, Deserialize)]
struct RendezvousErrorDetail {
    code: String,
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{body_partial_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    // ── create_pairing ────────────────────────────────────────────────────

    #[tokio::test]
    async fn create_pairing_parses_happy_body() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/pairings"))
            .and(body_partial_json(json!({ "sponsorDeviceId": "d" })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "code": "ABCD-EFGH",
                "expiresAtMs": 1_700_000_000_000_i64,
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = RendezvousClient::with_base_url(server.uri());
        let resp = client
            .create_pairing(&CreatePairingRequest {
                sponsor_device_id: "d".into(),
                sponsor_device_name: "n".into(),
                sponsor_endpoint_id: "e".into(),
                sponsor_ticket: "t".into(),
                ttl_secs: None,
            })
            .await
            .expect("ok");
        assert_eq!(resp.code, "ABCD-EFGH");
        assert_eq!(resp.expires_at_ms, 1_700_000_000_000);
    }

    #[tokio::test]
    async fn create_pairing_5xx_maps_service_unavailable() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/pairings"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;

        let client = RendezvousClient::with_base_url(server.uri());
        let err = client
            .create_pairing(&CreatePairingRequest {
                sponsor_device_id: "d".into(),
                sponsor_device_name: "n".into(),
                sponsor_endpoint_id: "e".into(),
                sponsor_ticket: "t".into(),
                ttl_secs: None,
            })
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            RendezvousHttpError::ServiceUnavailable(s) if s.as_u16() == 503
        ));
    }

    #[tokio::test]
    async fn create_pairing_4xx_carries_error_slug() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/pairings"))
            .respond_with(ResponseTemplate::new(400).set_body_json(json!({
                "error": { "code": "invalid_request" }
            })))
            .mount(&server)
            .await;
        let client = RendezvousClient::with_base_url(server.uri());
        let err = client
            .create_pairing(&CreatePairingRequest {
                sponsor_device_id: "d".into(),
                sponsor_device_name: "n".into(),
                sponsor_endpoint_id: "e".into(),
                sponsor_ticket: "t".into(),
                ttl_secs: None,
            })
            .await
            .unwrap_err();
        match err {
            RendezvousHttpError::Unexpected { status, slug } => {
                assert_eq!(status.as_u16(), 400);
                assert_eq!(slug, "invalid_request");
            }
            other => panic!("expected Unexpected, got {other:?}"),
        }
    }

    // ── resolve_pairing ──────────────────────────────────────────────────

    #[tokio::test]
    async fn resolve_pairing_parses_ticket() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/pairings/resolve"))
            .and(body_partial_json(json!({ "code": "CODE-9999" })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "sponsorTicket": "opaque-ticket-bytes",
                "sponsorEndpointId": "ignored",
                "expiresAtMs": 0,
            })))
            .mount(&server)
            .await;
        let client = RendezvousClient::with_base_url(server.uri());
        let resp = client.resolve_pairing("CODE-9999").await.expect("ok");
        assert_eq!(resp.sponsor_ticket, "opaque-ticket-bytes");
    }

    #[tokio::test]
    async fn resolve_pairing_404_maps_not_found() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/pairings/resolve"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        let client = RendezvousClient::with_base_url(server.uri());
        let err = client.resolve_pairing("X").await.unwrap_err();
        assert!(matches!(err, RendezvousHttpError::NotFound));
    }

    #[tokio::test]
    async fn resolve_pairing_410_maps_gone() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/pairings/resolve"))
            .respond_with(ResponseTemplate::new(410))
            .mount(&server)
            .await;
        let client = RendezvousClient::with_base_url(server.uri());
        let err = client.resolve_pairing("X").await.unwrap_err();
        assert!(matches!(err, RendezvousHttpError::Gone));
    }

    // ── consume_pairing ──────────────────────────────────────────────────

    #[tokio::test]
    async fn consume_pairing_204_returns_ok() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/pairings/consume"))
            .and(body_partial_json(json!({ "code": "X" })))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;
        let client = RendezvousClient::with_base_url(server.uri());
        client.consume_pairing("X").await.expect("ok");
    }

    #[tokio::test]
    async fn consume_pairing_409_maps_conflict() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/pairings/consume"))
            .respond_with(ResponseTemplate::new(409))
            .mount(&server)
            .await;
        let client = RendezvousClient::with_base_url(server.uri());
        let err = client.consume_pairing("X").await.unwrap_err();
        assert!(matches!(err, RendezvousHttpError::Conflict));
    }

    #[tokio::test]
    async fn transport_failure_maps_transport_variant() {
        // Port 1 is guaranteed unbound on every platform.
        let client = RendezvousClient::with_base_url("http://127.0.0.1:1");
        let err = client.consume_pairing("X").await.unwrap_err();
        assert!(matches!(err, RendezvousHttpError::Transport(_)));
    }
}
