//! Authenticated, loopback-only startup snapshots before business services exist.
use super::auth::DaemonAuthToken;
use axum::{
    extract::State,
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use std::{
    net::Ipv4Addr,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::Instrument;
use uc_daemon_contract::startup::DaemonStartupStatus;
use uc_daemon_local::socket::{
    remove_daemon_conn_file_at, write_daemon_conn_file_at, DaemonConnFile,
};
use uc_engine::StartupProgress;

#[derive(Clone)]
struct StartupApi {
    started_at: std::time::Instant,
    progress: StartupProgress,
    token: DaemonAuthToken,
    ready: Arc<AtomicBool>,
    failed: Arc<AtomicBool>,
    instance_id: String,
    retry: CancellationToken,
    stop: CancellationToken,
}

async fn snapshot(State(state): State<StartupApi>, headers: HeaderMap) -> Response {
    let bearer = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));
    if !bearer.is_some_and(|candidate| state.token.verify(candidate)) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    // Serialize the public Engine contract only; schema drift fails closed and is tested.
    let mut current = state.progress.snapshot();
    if !state.ready.load(Ordering::Acquire)
        && !state.failed.load(Ordering::Acquire)
        && !matches!(
            current.state,
            uc_engine::StartupState::Failed | uc_engine::StartupState::Interrupted
        )
    {
        // Progress events are sparse. A newly attached window needs a current clock sample.
        current.elapsed_ms = current
            .elapsed_ms
            .max(state.started_at.elapsed().as_millis() as u64);
    }
    let progress = serde_json::to_value(current).and_then(serde_json::from_value);
    let Ok(progress) = progress else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    #[derive(serde::Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Snapshot {
        instance_id: String,
        pid: u32,
        #[serde(flatten)]
        status: DaemonStartupStatus,
    }
    let mut response = Json(Snapshot {
        instance_id: state.instance_id.clone(),
        pid: std::process::id(),
        status: DaemonStartupStatus {
            package_version: env!("CARGO_PKG_VERSION").to_owned(),
            service_ready: state.ready.load(Ordering::Acquire),
            service_failed: state.failed.load(Ordering::Acquire),
            progress,
        },
    })
    .into_response();
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-store"),
    );
    response
}

// Exit only a terminal startup. The spawning host waits for the exact child
// before retry or quit; this route cannot stop a starting or ready Engine.
#[tracing::instrument(name = "daemon.startup.exit_for_retry", skip_all, fields(trace_id = %uuid::Uuid::new_v4()))]
async fn exit_for_retry(State(state): State<StartupApi>, headers: HeaderMap) -> StatusCode {
    let bearer = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));
    let instance = headers
        .get("X-UniClipboard-Startup-Instance")
        .and_then(|v| v.to_str().ok());
    if !bearer.is_some_and(|token| state.token.verify(token))
        || instance != Some(state.instance_id.as_str())
    {
        tracing::warn!(outcome = "unauthorized", "startup retry refused");
        return StatusCode::UNAUTHORIZED;
    }
    let snapshot = state.progress.snapshot();
    if state.ready.load(Ordering::Acquire)
        || !matches!(
            snapshot.state,
            uc_engine::StartupState::Failed | uc_engine::StartupState::Interrupted
        )
    {
        tracing::warn!(outcome = "not_terminal", "startup retry refused");
        return StatusCode::CONFLICT;
    }
    tracing::info!(
        outcome = "accepted",
        "failed startup will exit at its owner request"
    );
    state.retry.cancel();
    StatusCode::ACCEPTED
}

#[tracing::instrument(name = "daemon.startup.stop", skip_all, fields(trace_id = %uuid::Uuid::new_v4()))]
async fn stop_owned_run(State(state): State<StartupApi>, headers: HeaderMap) -> StatusCode {
    let bearer = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));
    let instance = headers
        .get("X-UniClipboard-Startup-Instance")
        .and_then(|value| value.to_str().ok());
    if !bearer.is_some_and(|token| state.token.verify(token))
        || instance != Some(state.instance_id.as_str())
    {
        tracing::warn!(outcome = "unauthorized", "startup stop refused");
        return StatusCode::UNAUTHORIZED;
    }
    state.stop.cancel();
    tracing::info!(outcome = "accepted", "owned daemon stop requested");
    StatusCode::ACCEPTED
}

pub struct StartupServer {
    cancel: CancellationToken,
    task: Option<JoinHandle<std::io::Result<()>>>,
    path: PathBuf,
    ready: Arc<AtomicBool>,
    failed: Arc<AtomicBool>,
    retry: CancellationToken,
    stop: CancellationToken,
}

impl StartupServer {
    pub async fn bind(
        progress: StartupProgress,
        token: DaemonAuthToken,
        path: PathBuf,
    ) -> anyhow::Result<Self> {
        let started_at = std::time::Instant::now();
        let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let addr = listener.local_addr()?;
        let ready = Arc::new(AtomicBool::new(false));
        let failed = Arc::new(AtomicBool::new(false));
        let instance_id = std::env::var("UC_DAEMON_STARTUP_ID")
            .ok()
            .filter(|value| value.len() == 32 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
            .unwrap_or_else(|| uuid::Uuid::new_v4().simple().to_string());
        let retry = CancellationToken::new();
        let stop_request = CancellationToken::new();
        let router = Router::new()
            .route("/startup", get(snapshot))
            .route("/startup/exit", post(exit_for_retry))
            .route("/startup/stop", post(stop_owned_run))
            .fallback(|| async { StatusCode::SERVICE_UNAVAILABLE })
            .with_state(StartupApi {
                started_at,
                progress,
                token: token.clone(),
                ready: ready.clone(),
                failed: failed.clone(),
                instance_id: instance_id.clone(),
                retry: retry.clone(),
                stop: stop_request.clone(),
            });
        let cancel = CancellationToken::new();
        let stop = cancel.clone();
        let mut conn =
            DaemonConnFile::new("127.0.0.1", addr.port(), token.as_str(), std::process::id());
        conn.instance_id = Some(instance_id.clone());
        write_daemon_conn_file_at(&path, &conn)?;
        let task = tokio::spawn(
            async move {
                axum::serve(listener, router)
                    .with_graceful_shutdown(stop.cancelled_owned())
                    .await
            }
            .in_current_span(),
        );
        Ok(Self {
            cancel,
            task: Some(task),
            path,
            ready,
            failed,
            retry,
            stop: stop_request,
        })
    }

    pub async fn retry_requested(&self) {
        self.retry.cancelled().await;
    }

    pub fn stop_token(&self) -> CancellationToken {
        self.stop.clone()
    }

    pub fn ready_flag(&self) -> Arc<AtomicBool> {
        self.ready.clone()
    }

    pub fn mark_service_failed(&self) {
        self.ready.store(false, Ordering::Release);
        self.failed.store(true, Ordering::Release);
    }

    pub async fn shutdown(mut self) -> anyhow::Result<()> {
        self.cancel.cancel();
        if let Some(task) = self.task.take() {
            task.await??;
        }
        remove_daemon_conn_file_at(&self.path)?;
        Ok(())
    }
}
impl Drop for StartupServer {
    fn drop(&mut self) {
        self.cancel.cancel();
        if let Some(task) = &self.task {
            task.abort();
        }
        if remove_daemon_conn_file_at(&self.path).is_err() {
            tracing::warn!("failed to remove startup connection metadata");
        }
    }
}
