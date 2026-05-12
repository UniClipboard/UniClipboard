//! HTTP server bootstrap for the daemon API.

use std::net::SocketAddr;
use std::sync::atomic::AtomicBool;
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
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
use uc_application::facade::AppFacade;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

use crate::api::auth::{build_connection_info, DaemonAuthToken, DaemonConnectionInfo};
use crate::api::dto::error::ApiError;
use crate::api::openapi::ApiDoc;
use crate::api::routes;
use crate::api::types::{
    DaemonWsEvent, HealthResponse, PeerSnapshotDto, PresenceRefreshResponse, SpaceMemberDto,
    StatusResponse,
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
}

impl DaemonApiState {
    pub fn new(
        app_facade: Arc<AppFacade>,
        auth_token: DaemonAuthToken,
        security: Arc<SecurityState>,
    ) -> Self {
        let (event_tx, _) = broadcast::channel(64);
        Self {
            auth_token,
            app_facade,
            event_tx,
            started_at: Instant::now(),
            clipboard_capture_gate: None,
            deferred_ready_notify: None,
            security,
        }
    }

    pub fn with_security(mut self, security: Arc<SecurityState>) -> Self {
        self.security = security;
        self
    }

    pub fn app_facade_or_error(&self) -> Result<Arc<AppFacade>, ApiError> {
        Ok(Arc::clone(&self.app_facade))
    }

    pub fn health_response(&self) -> HealthResponse {
        HealthResponse {
            status: "ok".to_string(),
            package_version: env!("CARGO_PKG_VERSION").to_string(),
            api_revision: uc_daemon_contract::DAEMON_API_REVISION.to_string(),
        }
    }

    pub fn status_response(&self) -> StatusResponse {
        StatusResponse {
            package_version: env!("CARGO_PKG_VERSION").to_string(),
            api_revision: uc_daemon_contract::DAEMON_API_REVISION.to_string(),
            uptime_seconds: self.started_at.elapsed().as_secs(),
            workers: Vec::new(),
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
        // 5xx splitted by status — Sentry groups events by message template, so
        // emitting a per-status static message+error_kind lets timeout, upstream-
        // unavailable, and internal-error 5xx fingerprint into separate issues
        // instead of drowning in one mega-group (see UNICLIPBOARD-RUST-5: 61
        // events that turned out to mix 500 client bugs and 503 backend outage).
        "server_error" => match status.as_u16() {
            503 => tracing::error!(
                request_id = %request_id,
                method = %method,
                path = %path,
                status = 503u16,
                elapsed_ms,
                error_kind = "upstream_unavailable",
                "daemon http upstream unavailable"
            ),
            504 => tracing::error!(
                request_id = %request_id,
                method = %method,
                path = %path,
                status = 504u16,
                elapsed_ms,
                error_kind = "gateway_timeout",
                "daemon http gateway timeout"
            ),
            500 => tracing::error!(
                request_id = %request_id,
                method = %method,
                path = %path,
                status = 500u16,
                elapsed_ms,
                error_kind = "internal_error",
                "daemon http handler internal error"
            ),
            code => tracing::error!(
                request_id = %request_id,
                method = %method,
                path = %path,
                status = code,
                elapsed_ms,
                error_kind = "server_error_other",
                "daemon http server error (other 5xx)"
            ),
        },
        "client_error" => tracing::warn!(
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
