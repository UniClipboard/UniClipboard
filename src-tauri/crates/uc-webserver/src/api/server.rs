//! HTTP server bootstrap for the daemon API.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use axum::body::Body;
use axum::http::header::{
    HeaderName, HeaderValue, ACCESS_CONTROL_ALLOW_HEADERS, ACCESS_CONTROL_ALLOW_METHODS,
    ACCESS_CONTROL_ALLOW_ORIGIN, ACCESS_CONTROL_REQUEST_METHOD, ORIGIN,
};
use axum::http::HeaderMap;
use axum::http::Method;
use axum::http::Request;
use axum::http::StatusCode;
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::Router;
use tokio::sync::{broadcast, Semaphore};
use tokio_util::sync::CancellationToken;
use uc_application::facade::AppFacade;
use uc_observability::analytics::{AnalyticsPort, NoopAnalyticsSink};
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

use crate::api::auth::{build_connection_info, DaemonAuthToken, DaemonConnectionInfo};
use crate::api::control_lease::ControlLeaseRegistry;
use crate::api::dto::error::ApiError;
use crate::api::openapi::ApiDoc;
use crate::api::routes;
use crate::api::types::{
    DaemonResidency, DaemonWsEvent, HealthResponse, PeerSnapshotDto, PresenceRefreshResponse,
    SpaceMemberDto, StatusResponse,
};
use crate::api::ws;
use crate::security::SecurityState;
use crate::socket::{try_resolve_daemon_http_addr, DEFAULT_HTTP_HOST};

#[derive(Clone)]
pub struct DaemonApiState {
    pub auth_token: DaemonAuthToken,
    pub app_facade: Arc<AppFacade>,
    pub event_tx: broadcast::Sender<DaemonWsEvent>,
    pub started_at: Instant,
    /// Gate controlling clipboard capture in the daemon.
    /// When set to true, clipboard monitoring becomes active.
    pub clipboard_capture_gate: Option<Arc<AtomicBool>>,
    /// Notify to trigger deferred service startup (clipboard-watcher, etc.)
    pub deferred_ready_notify: Option<Arc<tokio::sync::Notify>>,
    /// Security state: JWT secret, PID whitelist, and rate limiter.
    /// Wrapped in Arc so middleware (which receives Arc<DaemonApiState>) can share
    /// the same state with the server without cloning the inner fields.
    pub security: Arc<SecurityState>,
    /// Analytics sink — the daemon is the single authoritative product-analytics
    /// sender (ADR-008 D20). `POST /analytics/capture` dispatches GUI UI events
    /// through this. Defaults to a no-op so assembly paths / tests that don't
    /// wire analytics still construct cleanly; the real (gated) sink is injected
    /// via [`Self::with_analytics`] in the daemon runtime.
    pub analytics: Arc<dyn AnalyticsPort>,
    /// Concurrency cap for full-buffer blob pulls (`GET /clipboard/blobs/:id`).
    ///
    /// D6 (ADR-008 P3-d) interim RSS guard: the blob endpoint materializes the
    /// whole payload into a `Vec<u8>` (no streaming `BlobReaderPort` yet), so
    /// concurrent large pulls scale daemon RSS linearly (≈ K × payload, see
    /// [`adr-008-perf-spike-results.md`](../../../../../docs/architecture/adr-008-perf-spike-results.md)
    /// §4). This semaphore pins the worst case to `MAX_CONCURRENT_BLOB_PULLS ×
    /// payload` until the streaming reader supersedes it. Thumbnails are
    /// exempt (small, served via the separate `/clipboard/thumbnails` route).
    pub large_blob_semaphore: Arc<Semaphore>,
    /// Daemon residency mode surfaced in the health/status handshake
    /// (ADR-008 P5-L L1). Mapped from the daemon's `DaemonRunMode` at the
    /// assembly boundary and injected via [`Self::with_residency`]. Defaults to
    /// [`DaemonResidency::Standalone`] so assembly paths / tests that don't wire
    /// it construct cleanly (same defaulting strategy as the analytics no-op).
    pub residency: DaemonResidency,
    /// Control-WS lease registry (ADR-008 P5-L L3). Each authenticated WS
    /// connection holds one connection-bound lease for its lifetime; the active
    /// count is the daemon-side liveness signal that L4 consumes to decide when
    /// an `Oneshot` daemon may self-terminate. The registry is `Arc`-backed, so
    /// every `DaemonApiState` clone shares the same counter. In L3 the count is
    /// observed/logged only — no consumer reads it to drive behaviour yet.
    pub lease_registry: ControlLeaseRegistry,
    /// Controlled-restart quiescing flag (ADR-008 P5-L L8b). While set, admission
    /// gates reject NEW work (new control-WS upgrades + new clipboard dispatch/resend)
    /// with 503 `daemon_restarting` so in-flight leases can drain before a controlled
    /// restart. `Arc`-backed so every `DaemonApiState` clone — and the Oneshot
    /// supervisor that drains on it — shares the same flag. L8b adds NO setter: the
    /// flag is always false in production (the restart control plane that flips it is
    /// a later slice L8c), so this is production-behaviour-neutral.
    pub quiescing: Arc<AtomicBool>,
    /// Controlled-restart coordinator (ADR-008 P5-L L8c) — the SOLE mutator of
    /// [`Self::quiescing`]. It holds the SAME `Arc` as `quiescing` (constructed
    /// together in [`Self::new`]), so its `request()` / `abort()` transitions are
    /// observed by the L8b admission gates that read `quiescing`. The
    /// `/lifecycle/restart` handler drives `request()`; the Oneshot supervisor
    /// drives `abort()` on a drain timeout. `Arc`-backed, so every
    /// `DaemonApiState` clone shares the same arbitration state. Production-neutral
    /// in this slice: only an Oneshot daemon's restart endpoint calls `request()`,
    /// and no Oneshot daemon exists until L8d.
    pub restart: crate::api::restart::RestartCoordinator,
}

/// Max concurrent full-buffer blob pulls (D6 interim RSS guard; see
/// [`DaemonApiState::large_blob_semaphore`]). Matches the P0 spike's `concurrent=4`
/// scenario — high enough that one-at-a-time inline previews never queue, low
/// enough to cap the worst-case resident set from concurrent large downloads.
const MAX_CONCURRENT_BLOB_PULLS: usize = 4;

impl DaemonApiState {
    pub fn new(
        app_facade: Arc<AppFacade>,
        auth_token: DaemonAuthToken,
        security: Arc<SecurityState>,
    ) -> Self {
        let (event_tx, _) = broadcast::channel(64);
        // ADR-008 P5-L L8c: the quiescing flag and the restart coordinator must
        // share ONE `Arc` — the coordinator is the sole mutator, the L8b gates
        // read `quiescing`. Construct the flag once and hand the SAME `Arc` to
        // both so a coordinator `request()`/`abort()` is observed by every gate.
        let quiescing = Arc::new(AtomicBool::new(false));
        Self {
            auth_token,
            app_facade,
            event_tx,
            started_at: Instant::now(),
            clipboard_capture_gate: None,
            deferred_ready_notify: None,
            security,
            analytics: Arc::new(NoopAnalyticsSink),
            large_blob_semaphore: Arc::new(Semaphore::new(MAX_CONCURRENT_BLOB_PULLS)),
            residency: DaemonResidency::Standalone,
            lease_registry: ControlLeaseRegistry::new(),
            quiescing: quiescing.clone(),
            restart: crate::api::restart::RestartCoordinator::new(quiescing),
        }
    }

    pub fn with_security(mut self, security: Arc<SecurityState>) -> Self {
        self.security = security;
        self
    }

    /// Inject the daemon's residency mode (ADR-008 P5-L L1) so the
    /// health/status handshake reports whether this daemon is a persistent
    /// member node or a transient `Oneshot`. Mapped from `DaemonRunMode` at the
    /// daemon assembly boundary.
    pub fn with_residency(mut self, residency: DaemonResidency) -> Self {
        self.residency = residency;
        self
    }

    /// Inject the daemon's analytics sink so `POST /analytics/capture` reports
    /// through the single authoritative sender (ADR-008 D20).
    pub fn with_analytics(mut self, analytics: Arc<dyn AnalyticsPort>) -> Self {
        self.analytics = analytics;
        self
    }

    pub fn app_facade_or_error(&self) -> Result<Arc<AppFacade>, ApiError> {
        Ok(Arc::clone(&self.app_facade))
    }

    pub fn health_response(&self) -> HealthResponse {
        Self::health_response_for(self.residency)
    }

    /// Build the `GET /health` body for a given residency. Split out from
    /// [`Self::health_response`] (which just forwards `self.residency`) so the
    /// residency-emission contract — every `DaemonRunMode` surfaces its own
    /// residency in the handshake (ADR-008 P5-L L1) — is unit-testable without
    /// composing a full `DaemonApiState` (and thus a full `AppFacade`).
    pub fn health_response_for(residency: DaemonResidency) -> HealthResponse {
        HealthResponse {
            status: "ok".to_string(),
            package_version: env!("CARGO_PKG_VERSION").to_string(),
            api_revision: uc_daemon_contract::DAEMON_API_REVISION.to_string(),
            residency,
        }
    }

    pub fn status_response(&self) -> StatusResponse {
        StatusResponse {
            uptime_seconds: self.started_at.elapsed().as_secs(),
            ..Self::status_response_for(self.residency)
        }
    }

    /// Build the `GET /status` body for a given residency (uptime defaults to
    /// `0`; the live method overrides it from `started_at`). Mirrors
    /// [`Self::health_response_for`] so the residency-emission contract is
    /// unit-testable without a full `DaemonApiState`.
    pub fn status_response_for(residency: DaemonResidency) -> StatusResponse {
        StatusResponse {
            package_version: env!("CARGO_PKG_VERSION").to_string(),
            api_revision: uc_daemon_contract::DAEMON_API_REVISION.to_string(),
            uptime_seconds: 0,
            workers: Vec::new(),
            residency,
        }
    }

    pub async fn peer_snapshots(&self) -> anyhow::Result<Vec<PeerSnapshotDto>> {
        let peers = self.app_facade.list_peer_snapshots().await?;
        Ok(peers
            .into_iter()
            .map(|peer| PeerSnapshotDto {
                channel: uc_application::facade::connection_channel_to_wire(peer.channel)
                    .to_string(),
                peer_id: peer.peer_id,
                device_name: peer.device_name,
                addresses: peer.addresses,
                is_paired: peer.is_paired,
                connected: peer.connected,
                pairing_state: peer.pairing_state,
                connection_address: peer.connection_address,
            })
            .collect())
    }

    /// 主动 probe 所有已配对 peer 的连接性。
    ///
    /// 用途：让"对端断网"场景下的离线检测从 ~60s（QUIC max_idle_timeout）
    /// 缩短到 probe 间隔 + 拨号失败时间。`ensure_reachable_all` 内部对每个
    /// peer 都重新发起一次 iroh 拨号——在线 peer 拨号成功后丢弃新连接保留
    /// 旧的；离线 peer 拨号失败会立刻 `broadcast(Offline)`，进而触发
    /// `peers.changed` 推送、前端重拉 `/paired-devices`、UI 切灰。
    pub async fn refresh_presence(&self) -> anyhow::Result<PresenceRefreshResponse> {
        let report = self.app_facade.refresh_presence().await?;
        Ok(PresenceRefreshResponse {
            total: report.total as u32,
            online: report.online as u32,
            offline: report.offline as u32,
            errors: report.errors.len() as u32,
        })
    }

    pub async fn paired_devices(&self) -> anyhow::Result<Vec<SpaceMemberDto>> {
        // 通过 list_peer_snapshots() 获取真实在线状态：
        // 该方法走 list_with_presence()，会聚合 PresencePort.current_state()，
        // 反映 IrohPresenceAdapter 中由 ensure_reachable / connection.closed()
        // 维护的 last_state 缓存。list_members() 不查 PresencePort，所以
        // 拿不到 connected。同时 list_peer_snapshots() 已过滤本机。
        let snapshots = self.app_facade.list_peer_snapshots().await?;
        Ok(snapshots
            .into_iter()
            .map(|snapshot| SpaceMemberDto {
                channel: uc_application::facade::connection_channel_to_wire(snapshot.channel)
                    .to_string(),
                peer_id: snapshot.peer_id,
                device_name: snapshot.device_name.unwrap_or_default(),
                pairing_state: snapshot.pairing_state,
                last_seen_at_ms: None,
                connected: snapshot.connected,
                connection_address: snapshot.connection_address,
            })
            .collect())
    }

    pub fn with_clipboard_gate(mut self, gate: Arc<AtomicBool>) -> Self {
        self.clipboard_capture_gate = Some(gate);
        self
    }

    pub fn with_deferred_ready_notify(mut self, notify: Arc<tokio::sync::Notify>) -> Self {
        self.deferred_ready_notify = Some(notify);
        self
    }

    pub fn connection_info_for_addr(
        &self,
        listen_addr: SocketAddr,
        client_pid: u32,
    ) -> DaemonConnectionInfo {
        build_connection_info(
            DEFAULT_HTTP_HOST,
            listen_addr.port(),
            &self.auth_token,
            client_pid,
        )
    }
}

/// Admission gate for new daemon work during a controlled-restart drain
/// (ADR-008 P5-L L8b). Returns `Err(ApiError::restarting)` (503 `daemon_restarting`)
/// while `quiescing` is set, else `Ok(())`. Shared by the control-WS upgrade handler
/// and the clipboard dispatch/resend handlers so the rejection is uniform.
pub(crate) fn ensure_not_quiescing(quiescing: &AtomicBool) -> Result<(), ApiError> {
    if quiescing.load(Ordering::SeqCst) {
        Err(ApiError::restarting("daemon is restarting"))
    } else {
        Ok(())
    }
}

pub fn build_router(state: DaemonApiState) -> Router {
    let swagger = SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", ApiDoc::openapi());

    #[cfg(debug_assertions)]
    let swagger = swagger.url("/api-docs/openapi-dev.json", {
        use crate::api::ApiDocDev;
        ApiDocDev::openapi()
    });

    Router::new()
        .merge(swagger)
        .merge(routes::router_l1(state.clone()))
        .merge(routes::router_l2_plus(state.clone()))
        .merge(crate::security::connect::router())
        .merge(ws::router())
        .layer(middleware::from_fn(cors_middleware))
        // request_tracing wraps cors so we observe every request — including
        // CORS-rejected, auth-rejected, rate-limited, and 404s — at one place.
        // Layer ordering: in axum the LAST `.layer()` is OUTERMOST (runs first
        // on the request, last on the response). Adding tracing after cors
        // makes it the outer ring around everything.
        .layer(middleware::from_fn(request_tracing_middleware))
        .with_state(state)
}

/// Global HTTP request tracing — logs entry, status, and elapsed for every
/// route. Auth query param is redacted so session tokens never appear in logs.
pub(crate) async fn request_tracing_middleware(request: Request<Body>, next: Next) -> Response {
    let method = request.method().clone();
    let path = request.uri().path().to_string();
    let query_redacted = redact_query_secrets(request.uri().query());
    let request_id = format!("{:016x}", rand::random::<u64>());
    let start = Instant::now();

    if method == Method::OPTIONS {
        tracing::debug!(
            request_id = %request_id,
            method = %method,
            path = %path,
            query = %query_redacted,
            "daemon http preflight received"
        );
    } else {
        tracing::info!(
            request_id = %request_id,
            method = %method,
            path = %path,
            query = %query_redacted,
            "daemon http request received"
        );
    }

    let response = next.run(request).await;
    let status = response.status();
    let elapsed_ms = start.elapsed().as_millis() as u64;

    let level_action = if status.is_server_error() {
        "server_error"
    } else if status.is_client_error() {
        "client_error"
    } else {
        "ok"
    };

    match level_action {
        // Access-log echo only — root cause lives in the handler that mapped
        // the facade error to ApiError, not here. Earlier UNICLIPBOARD-RUST-5
        // tried to fingerprint 5xx by status code, but a per-status static
        // template ("daemon http upstream unavailable" etc.) is just a reskin
        // of the HTTP status — it carries no signal beyond `status` itself
        // and crowds out the real ERROR emitted upstream. Keep this at WARN
        // so the Log channel still has a query handle for 5xx rate, but stop
        // creating Sentry Issues from a layer that doesn't know the cause.
        "server_error" => tracing::warn!(
            request_id = %request_id,
            method = %method,
            path = %path,
            status = status.as_u16(),
            elapsed_ms,
            "daemon http response 5xx"
        ),
        "client_error" => tracing::info!(
            request_id = %request_id,
            method = %method,
            path = %path,
            status = status.as_u16(),
            elapsed_ms,
            "daemon http request rejected (client error)"
        ),
        _ => {
            if method == Method::OPTIONS {
                tracing::debug!(
                    request_id = %request_id,
                    method = %method,
                    path = %path,
                    status = status.as_u16(),
                    elapsed_ms,
                    "daemon http preflight completed"
                );
            } else {
                tracing::info!(
                    request_id = %request_id,
                    method = %method,
                    path = %path,
                    status = status.as_u16(),
                    elapsed_ms,
                    "daemon http request completed"
                );
            }
        }
    }

    response
}

/// Redact secrets from a query string before logging. `auth=...` carries
/// session tokens via query (used by `<img src>` blob loads); we replace its
/// value with `<redacted>` so it never reaches log files.
fn redact_query_secrets(query: Option<&str>) -> String {
    let Some(q) = query else {
        return String::new();
    };
    url::form_urlencoded::parse(q.as_bytes())
        .map(|(k, v)| {
            if k == "auth" || k == "token" || k == "session" {
                format!("{k}=<redacted>")
            } else {
                format!("{k}={v}")
            }
        })
        .collect::<Vec<_>>()
        .join("&")
}

pub(crate) async fn cors_middleware(request: Request<Body>, next: Next) -> Response {
    tracing::debug!(
        method = %request.method(),
        uri = %request.uri(),
        has_origin = request.headers().contains_key(ORIGIN),
        has_preflight_method = request.headers().contains_key(ACCESS_CONTROL_REQUEST_METHOD),
        "daemon cors middleware received request"
    );

    let origin = request
        .headers()
        .get(ORIGIN)
        .and_then(|value| value.to_str().ok())
        .filter(|origin| is_allowed_cors_origin(origin))
        .map(str::to_owned);

    if request.method() == Method::OPTIONS
        && request
            .headers()
            .contains_key(ACCESS_CONTROL_REQUEST_METHOD)
    {
        let mut response = Response::new(Body::empty());
        *response.status_mut() = StatusCode::NO_CONTENT;
        apply_cors_headers(response.headers_mut(), origin.as_deref());
        return response;
    }

    let mut response = next.run(request).await;
    apply_cors_headers(response.headers_mut(), origin.as_deref());
    response
}

fn apply_cors_headers(headers: &mut HeaderMap, origin: Option<&str>) {
    let Some(origin) = origin else {
        return;
    };

    let Ok(origin_value) = HeaderValue::from_str(origin) else {
        return;
    };
    headers.insert(ACCESS_CONTROL_ALLOW_ORIGIN, origin_value);

    headers.insert(
        ACCESS_CONTROL_ALLOW_METHODS,
        // PATCH 必须列在这里：`/member/:device_id/sync-preferences` 走 PATCH，
        // 浏览器/webview 在 preflight 响应里看不到 PATCH 就会拦截真请求 ——
        // 现象是 DeviceSettingsSheet 上的发送开关切了又弹回、永远改不动。
        HeaderValue::from_static("GET, POST, PUT, PATCH, DELETE, OPTIONS"),
    );
    headers.insert(
        ACCESS_CONTROL_ALLOW_HEADERS,
        HeaderValue::from_static("authorization, content-type"),
    );
    headers.insert(
        HeaderName::from_static("vary"),
        HeaderValue::from_static(
            "origin, access-control-request-method, access-control-request-headers",
        ),
    );
}

fn is_allowed_cors_origin(origin: &str) -> bool {
    origin == "tauri://localhost"
        || origin == "http://tauri.localhost"
        || origin == "https://tauri.localhost"
        || origin.starts_with("http://localhost:")
        || origin.starts_with("http://127.0.0.1:")
        || origin.starts_with("http://[::1]:")
}

pub async fn run_http_server(
    state: DaemonApiState,
    cancel: CancellationToken,
) -> anyhow::Result<()> {
    let addr = try_resolve_daemon_http_addr()?;
    let connection_info = state.connection_info_for_addr(addr, std::process::id());
    tracing::info!(
        base_url = %connection_info.base_url,
        ws_url = %connection_info.ws_url,
        "daemon HTTP API listening on 127.0.0.1"
    );

    // into_make_service_with_connect_info enables ConnectInfo<SocketAddr> in handlers.
    // This is required for the /auth/connect endpoint's IP-based rate limiting.
    // NOTE on ConnectInfo in tests: In test contexts using tower::ServiceExt::oneshot,
    // the socket address will be a default value (127.0.0.1:0) since there's no real
    // TCP connection. The SlidingWindowRateLimiter unit tests cover rate limiting logic
    // independently. IP-based rate limiting works correctly in production.
    let make_service = build_router(state).into_make_service_with_connect_info::<SocketAddr>();

    let listener = tokio::net::TcpListener::bind(addr).await?;

    axum::serve(listener, make_service)
        .with_graceful_shutdown(cancel.cancelled_owned())
        .await?;

    Ok(())
}

#[cfg(test)]
mod residency_handshake_tests {
    use super::*;

    /// ADR-008 P5-L L1: `GET /health` and `GET /status` must report whichever
    /// residency the daemon was assembled with. `DaemonApiState` is fed
    /// `DaemonRunMode -> DaemonResidency` at the assembly boundary (uc-daemon),
    /// and the handler bodies copy `self.residency` verbatim — exercised here
    /// per variant without composing a full `AppFacade`.
    #[test]
    fn health_and_status_report_each_residency() {
        for residency in [
            DaemonResidency::Standalone,
            DaemonResidency::ServerHeadless,
            DaemonResidency::Oneshot,
        ] {
            let health = DaemonApiState::health_response_for(residency);
            assert_eq!(health.residency, residency);
            assert_eq!(health.status, "ok");

            let status = DaemonApiState::status_response_for(residency);
            assert_eq!(status.residency, residency);
        }
    }
}

#[cfg(test)]
mod quiescing_gate_tests {
    use super::*;

    /// ADR-008 P5-L L8b: while `quiescing` is clear (the production-default — no
    /// setter wired in this slice) the admission gate must admit. Constructed off
    /// a bare `AtomicBool`, no full `DaemonApiState`, so the gate's contract is
    /// unit-testable in isolation.
    #[test]
    fn ensure_not_quiescing_admits_when_clear() {
        let quiescing = AtomicBool::new(false);
        assert!(ensure_not_quiescing(&quiescing).is_ok());
    }

    /// ADR-008 P5-L L8b: while `quiescing` is set the gate must reject with the
    /// distinct 503 `daemon_restarting` surface (not the generic
    /// `runtime_unavailable`) so clients can tell a controlled restart apart from
    /// a generic outage and retry against the successor.
    #[test]
    fn ensure_not_quiescing_rejects_when_set() {
        let quiescing = AtomicBool::new(true);
        let err = ensure_not_quiescing(&quiescing).expect_err("must reject while quiescing");
        assert_eq!(err.status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(err.code, "daemon_restarting");
    }
}
