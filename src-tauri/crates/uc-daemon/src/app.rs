//! # DaemonApp
//!
//! Top-level daemon lifecycle: binds the RPC socket, starts workers,
//! waits for shutdown signal, and tears down in reverse order.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tokio::net::UnixListener;
use tokio::sync::RwLock;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use crate::rpc::server::{check_or_remove_stale_socket, run_rpc_accept_loop};
use crate::rpc::types::WorkerStatus;
use crate::state::RuntimeState;
use crate::worker::{DaemonWorker, WorkerHealth};

/// Main daemon application.
///
/// Owns the worker list, RPC state, and cancellation token.
/// Workers use `Arc<dyn DaemonWorker>` (not `Box`) to allow cloning
/// for `tokio::spawn` `'static` requirement.
pub struct DaemonApp {
    workers: Vec<Arc<dyn DaemonWorker>>,
    state: Arc<RwLock<RuntimeState>>,
    socket_path: PathBuf,
    cancel: CancellationToken,
}

impl DaemonApp {
    /// Create a new DaemonApp with the given workers and socket path.
    pub fn new(workers: Vec<Arc<dyn DaemonWorker>>, socket_path: PathBuf) -> Self {
        let initial_statuses: Vec<WorkerStatus> = workers
            .iter()
            .map(|w| WorkerStatus {
                name: w.name().to_string(),
                health: WorkerHealth::Healthy,
            })
            .collect();

        Self {
            workers,
            state: Arc::new(RwLock::new(RuntimeState::new(initial_statuses))),
            socket_path,
            cancel: CancellationToken::new(),
        }
    }

    /// Run the daemon: bind RPC socket, start workers, wait for shutdown, cleanup.
    pub async fn run(&self) -> anyhow::Result<()> {
        info!("uniclipboard-daemon starting");

        // 1. Bind RPC socket FIRST (fail-fast before starting workers)
        check_or_remove_stale_socket(&self.socket_path).await?;
        let listener = UnixListener::bind(&self.socket_path)?;

        info!("uniclipboard-daemon running, RPC at {:?}", self.socket_path);

        // 2. Start workers
        let mut worker_tasks = JoinSet::new();
        for worker in &self.workers {
            let w = Arc::clone(worker);
            let token = self.cancel.child_token();
            worker_tasks.spawn(async move { w.start(token).await });
        }

        // 3. Spawn accept loop and wait for shutdown signal, accept loop crash, or worker crash
        let rpc_state = self.state.clone();
        let rpc_cancel = self.cancel.child_token();
        let mut rpc_handle = tokio::spawn(run_rpc_accept_loop(listener, rpc_state, rpc_cancel));

        tokio::select! {
            _ = wait_for_shutdown_signal() => {
                info!("shutdown signal received");
            }
            result = &mut rpc_handle => {
                warn!("RPC accept loop exited unexpectedly: {:?}", result);
            }
            Some(result) = worker_tasks.join_next() => {
                warn!("worker task exited unexpectedly: {:?}", result);
            }
        }

        // 4. Shutdown sequence
        info!("shutting down...");

        // Cancel all child tokens
        self.cancel.cancel();

        // Drain worker tasks with timeout
        tokio::time::timeout(Duration::from_secs(5), async {
            while worker_tasks.join_next().await.is_some() {}
        })
        .await
        .ok();

        // Await RPC accept loop with timeout
        tokio::time::timeout(Duration::from_secs(5), rpc_handle)
            .await
            .ok();

        // Stop workers in reverse order
        for worker in self.workers.iter().rev() {
            info!(worker = worker.name(), "stopping worker");
            if let Err(e) = worker.stop().await {
                warn!(worker = worker.name(), "error stopping worker: {}", e);
            }
        }

        // Remove socket file
        if let Err(e) = std::fs::remove_file(&self.socket_path) {
            if e.kind() != std::io::ErrorKind::NotFound {
                warn!("failed to remove socket file: {}", e);
            }
        }

        info!("uniclipboard-daemon stopped");
        Ok(())
    }
}

/// Wait for either Ctrl-C or SIGTERM (Unix).
async fn wait_for_shutdown_signal() -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut sigterm = signal(SignalKind::terminate())
            .map_err(|e| anyhow::anyhow!("failed to register SIGTERM handler: {}", e))?;
        tokio::select! {
            result = tokio::signal::ctrl_c() => {
                result.map_err(|e| anyhow::anyhow!("ctrl_c handler error: {}", e))?;
            }
            _ = sigterm.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c()
            .await
            .map_err(|e| anyhow::anyhow!("ctrl_c handler error: {}", e))?;
    }
    Ok(())
}
