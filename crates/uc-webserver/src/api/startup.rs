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
        Arc,
    },
};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use uc_daemon_contract::startup::DaemonStartupStatus;
use uc_daemon_local::socket::{
    remove_daemon_conn_file_at, write_daemon_conn_file_at, DaemonConnFile,
};
use uc_engine::StartupProgress;

#[derive(Clone)]
struct StartupApi {
    progress: StartupProgress,
    token: DaemonAuthToken,
    ready: Arc<AtomicBool>,
    failed: Arc<AtomicBool>,
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
    let progress = serde_json::to_value(state.progress.snapshot()).and_then(serde_json::from_value);
    let Ok(progress) = progress else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
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
}

impl StartupServer {
    pub async fn bind(
        progress: StartupProgress,
        token: DaemonAuthToken,
        path: PathBuf,
    ) -> anyhow::Result<Self> {
        let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let addr = listener.local_addr()?;
        let ready = Arc::new(AtomicBool::new(false));
        let failed = Arc::new(AtomicBool::new(false));
        let router = Router::new()
            .route("/startup", get(snapshot))
            .fallback(|| async { StatusCode::SERVICE_UNAVAILABLE })
            .with_state(StartupApi {
                progress,
                token: token.clone(),
                ready: ready.clone(),
                failed: failed.clone(),
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
        })
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
