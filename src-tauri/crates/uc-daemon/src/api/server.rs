//! HTTP server bootstrap for the daemon API.

use std::net::SocketAddr;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

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
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
use uc_app::runtime::CoreRuntime;
use uc_app::usecases::SetupOrchestrator;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

use uc_app::usecases::space_access::SpaceAccessOrchestrator;
use uc_core::network::daemon_api_strings::pairing_error_code;

use crate::api::auth::{build_connection_info, DaemonAuthToken, DaemonConnectionInfo};
use crate::api::dto::error::ApiError;
use crate::api::dto::pairing::PairingApiErrorResponse;
use crate::api::openapi::ApiDoc;
use crate::api::query::DaemonQueryService;
use crate::api::routes;
use crate::api::types::DaemonWsEvent;
use crate::api::ws;
use crate::pairing::host::{DaemonPairingHost, DaemonPairingHostError};
use crate::security::SecurityState;
use crate::socket::{try_resolve_daemon_http_addr, DEFAULT_HTTP_HOST};

#[derive(Clone)]
pub struct DaemonApiState {
    pub query_service: Arc<DaemonQueryService>,
    pub auth_token: DaemonAuthToken,
    pub runtime: Option<Arc<CoreRuntime>>,
    pub pairing_host: Option<Arc<DaemonPairingHost>>,
    pub setup_orchestrator: Option<Arc<SetupOrchestrator>>,
    pub space_access_orchestrator: Option<Arc<SpaceAccessOrchestrator>>,
    pub event_tx: broadcast::Sender<DaemonWsEvent>,
    /// Gate controlling clipboard capture in the daemon.
    /// When set to true, clipboard monitoring becomes active.
    pub clipboard_capture_gate: Option<Arc<AtomicBool>>,
    /// Notify to trigger deferred service startup (clipboard-watcher, etc.)
    pub deferred_ready_notify: Option<Arc<tokio::sync::Notify>>,
    /// Security state: JWT secret, PID whitelist, and rate limiter.
    /// Wrapped in Arc so middleware (which receives Arc<DaemonApiState>) can share
    /// the same state with the server without cloning the inner fields.
    pub security: Arc<SecurityState>,
}

impl DaemonApiState {
    pub fn new(
        query_service: Arc<DaemonQueryService>,
        auth_token: DaemonAuthToken,
        runtime: Option<Arc<CoreRuntime>>,
        security: Arc<SecurityState>,
    ) -> Self {
        let (event_tx, _) = broadcast::channel(64);
        Self {
            query_service,
            auth_token,
            runtime,
            pairing_host: None,
            setup_orchestrator: None,
            space_access_orchestrator: None,
            event_tx,
            clipboard_capture_gate: None,
            deferred_ready_notify: None,
            security,
        }
    }

    pub fn with_security(mut self, security: Arc<SecurityState>) -> Self {
        self.security = security;
        self
    }

    pub fn with_pairing_host(mut self, pairing_host: Arc<DaemonPairingHost>) -> Self {
        self.pairing_host = Some(pairing_host);
        self
    }

    pub fn pairing_host(&self) -> Option<Arc<DaemonPairingHost>> {
        self.pairing_host.clone()
    }

    pub fn with_setup(mut self, setup_orchestrator: Arc<SetupOrchestrator>) -> Self {
        self.setup_orchestrator = Some(setup_orchestrator);
        self
    }

    pub fn setup_orchestrator(&self) -> Option<Arc<SetupOrchestrator>> {
        self.setup_orchestrator.clone()
    }

    pub fn with_space_access(
        mut self,
        space_access_orchestrator: Arc<SpaceAccessOrchestrator>,
    ) -> Self {
        self.space_access_orchestrator = Some(space_access_orchestrator);
        self
    }

    pub fn space_access_orchestrator(&self) -> Option<Arc<SpaceAccessOrchestrator>> {
        self.space_access_orchestrator.clone()
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

    /// Extracts the runtime, or returns an ApiError if unavailable.
    ///
    /// Usage: `let runtime = state.runtime_or_error()?;` (for `Result` handlers)
    /// or `let runtime = state.runtime_or_error().map_err(ApiError::into_response)?;`
    /// (for `impl IntoResponse` handlers).
    pub fn runtime_or_error(&self) -> Result<Arc<CoreRuntime>, ApiError> {
        self.runtime
            .clone()
            .ok_or_else(|| ApiError::service_unavailable("daemon runtime unavailable"))
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
        .with_state(state)
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
        HeaderValue::from_static("GET, POST, PUT, DELETE, OPTIONS"),
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

pub(crate) fn map_daemon_pairing_error(
    error: DaemonPairingHostError,
) -> (StatusCode, PairingApiErrorResponse) {
    match error {
        DaemonPairingHostError::ActivePairingSessionExists => (
            StatusCode::CONFLICT,
            PairingApiErrorResponse {
                code: pairing_error_code::ACTIVE_SESSION_EXISTS.to_string(),
                message: "active pairing session exists".to_string(),
            },
        ),
        DaemonPairingHostError::HostNotDiscoverable => (
            StatusCode::BAD_REQUEST,
            PairingApiErrorResponse {
                code: pairing_error_code::HOST_NOT_DISCOVERABLE.to_string(),
                message: "host not discoverable".to_string(),
            },
        ),
        DaemonPairingHostError::NoLocalPairingParticipantReady => (
            StatusCode::BAD_REQUEST,
            PairingApiErrorResponse {
                code: pairing_error_code::NO_LOCAL_PARTICIPANT.to_string(),
                message: "no local pairing participant ready".to_string(),
            },
        ),
        DaemonPairingHostError::SessionNotFound(_) => (
            StatusCode::NOT_FOUND,
            PairingApiErrorResponse {
                code: pairing_error_code::SESSION_NOT_FOUND.to_string(),
                message: "pairing session not found".to_string(),
            },
        ),
        DaemonPairingHostError::Internal(message) => {
            tracing::error!(error = %message, "daemon pairing command failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                PairingApiErrorResponse {
                    code: pairing_error_code::INTERNAL.to_string(),
                    message,
                },
            )
        }
    }
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
