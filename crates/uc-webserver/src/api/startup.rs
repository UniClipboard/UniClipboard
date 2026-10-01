//! Authenticated, loopback-only startup snapshots before business services exist.
use super::auth::DaemonAuthToken;
use axum::{
    extract::State,
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use std::{
    net::Ipv4Addr,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use uc_daemon_contract::startup::{
    DaemonStartupStatus, StartupFailureDto, StartupFailureReasonDto, StartupSnapshotDto,
    StartupStateDto,
};
use uc_daemon_local::socket::{
    remove_daemon_conn_file_at, write_daemon_conn_file_at, DaemonConnFile,
};
use uc_engine::error_codes::PROFILE_UPGRADE_BACKUP_KEY_MISSING_CODE;
use uc_engine::{EngineError, StartupProgress};

/// Outcome of `Engine::start` as seen by the host. The error code is only meaningful inside this
/// domain: other domains reuse the same numbers (for example invitation failures reuse 1224).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct EngineStartFailure {
    code: u32,
    retryable: bool,
}

impl From<&EngineError> for EngineStartFailure {
    fn from(error: &EngineError) -> Self {
        Self {
            code: error.code(),
            retryable: error.is_retryable(),
        }
    }
}

/// Reconcile Engine's progress snapshot with the structured result of `Engine::start`.
///
/// The progress snapshot can only say where the attempt stopped (for example `backup_failed` with
/// `retryable`), while the start result is authoritative for retryability. A non-retryable start
/// result therefore withdraws the retry action, and the one start failure whose cause the progress
/// snapshot cannot express is mapped to its own reason.
fn apply_engine_start_failure(snapshot: &mut StartupSnapshotDto, failure: EngineStartFailure) {
    if !matches!(snapshot.state, StartupStateDto::Failed) {
        return;
    }
    if !failure.retryable {
        snapshot.allowed_actions.retry = false;
        if let Some(current) = snapshot.failure.as_mut() {
            current.retryable = false;
        }
    }
    if failure.code == PROFILE_UPGRADE_BACKUP_KEY_MISSING_CODE && !failure.retryable {
        snapshot.failure = Some(StartupFailureDto {
            reason: StartupFailureReasonDto::UpgradeBackupKeyMissing,
            retryable: false,
        });
    }
}

#[derive(Clone)]
struct StartupApi {
    started_at: std::time::Instant,
    progress: StartupProgress,
    token: DaemonAuthToken,
    ready: Arc<AtomicBool>,
    failed: Arc<AtomicBool>,
    start_failure: Arc<Mutex<Option<EngineStartFailure>>>,
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
    let Ok(mut progress) = progress else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let start_failure = state.start_failure.lock().ok().and_then(|failure| *failure);
    if let Some(failure) = start_failure {
        apply_engine_start_failure(&mut progress, failure);
    }
    let mut response = Json(DaemonStartupStatus {
        package_version: env!("CARGO_PKG_VERSION").to_owned(),
        service_ready: state.ready.load(Ordering::Acquire),
        service_failed: state.failed.load(Ordering::Acquire),
        progress,
    })
    .into_response();
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-store"),
    );
    response
}

pub struct StartupServer {
    cancel: CancellationToken,
    task: Option<JoinHandle<std::io::Result<()>>>,
    path: PathBuf,
    ready: Arc<AtomicBool>,
    failed: Arc<AtomicBool>,
    start_failure: Arc<Mutex<Option<EngineStartFailure>>>,
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
        let start_failure = Arc::new(Mutex::new(None));
        let router = Router::new()
            .route("/startup", get(snapshot))
            .fallback(|| async { StatusCode::SERVICE_UNAVAILABLE })
            .with_state(StartupApi {
                started_at,
                progress,
                token: token.clone(),
                ready: ready.clone(),
                failed: failed.clone(),
                start_failure: start_failure.clone(),
            });
        let cancel = CancellationToken::new();
        let stop = cancel.clone();
        write_daemon_conn_file_at(
            &path,
            &DaemonConnFile::new("127.0.0.1", addr.port(), token.as_str(), std::process::id()),
        )?;
        let task = tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(stop.cancelled_owned())
                .await
        });
        Ok(Self {
            cancel,
            task: Some(task),
            path,
            ready,
            failed,
            start_failure,
        })
    }

    pub fn ready_flag(&self) -> Arc<AtomicBool> {
        self.ready.clone()
    }

    /// Record the structured result of a failed `Engine::start` so the startup status can report
    /// it. Must only be called with the error returned by Engine startup itself.
    pub fn record_engine_start_failure(&self, error: &EngineError) {
        if let Ok(mut failure) = self.start_failure.lock() {
            *failure = Some(error.into());
        }
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
