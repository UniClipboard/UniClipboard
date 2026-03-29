//! Shared background blob-processing tasks.
//!
//! The four core tasks (SpoolScanner, SpoolerTask, BackgroundBlobWorker, SpoolJanitor)
//! are needed by both the GUI and daemon entry points.  This module provides a single
//! `spawn_blob_processing_tasks()` that callers `await` inside whatever spawn mechanism
//! they use (tauri::async_runtime::spawn for GUI, rt.spawn for daemon).

use std::sync::Arc;
use std::time::Duration;

use tracing::{info, warn};

use tokio::sync::mpsc;
use uc_app::task_registry::TaskRegistry;
use uc_app::AppDeps;
use uc_core::ids::RepresentationId;
use uc_core::ports::clipboard::{
    ClipboardRepresentationRepositoryPort, ThumbnailGeneratorPort, ThumbnailRepositoryPort,
};
use uc_core::ports::{BlobWriterPort, ClockPort, ContentHashPort};
use uc_infra::clipboard::{BackgroundBlobWorker, SpoolJanitor, SpoolScanner, SpoolerTask};

use crate::BackgroundRuntimeDeps;

/// Interval between spool janitor sweeps (1 hour).
pub const SPOOL_JANITOR_INTERVAL_SECS: u64 = 60 * 60;

/// Ports extracted from `AppDeps` that the blob processing tasks need.
///
/// Since `AppDeps` is not `Clone` and the spawn boundary requires `'static`,
/// callers clone these `Arc`s before entering the async context.
pub struct BlobProcessingPorts {
    pub representation_repo: Arc<dyn ClipboardRepresentationRepositoryPort>,
    pub worker_tx: mpsc::Sender<RepresentationId>,
    pub blob_writer: Arc<dyn BlobWriterPort>,
    pub hasher: Arc<dyn ContentHashPort>,
    pub clock: Arc<dyn ClockPort>,
    pub thumbnail_repo: Arc<dyn ThumbnailRepositoryPort>,
    pub thumbnail_generator: Arc<dyn ThumbnailGeneratorPort>,
}

impl BlobProcessingPorts {
    /// Clone the relevant ports from `AppDeps`.
    pub fn from_app_deps(deps: &AppDeps) -> Self {
        Self {
            representation_repo: deps.clipboard.representation_repo.clone(),
            worker_tx: deps.clipboard.worker_tx.clone(),
            blob_writer: deps.storage.blob_writer.clone(),
            hasher: deps.system.hash.clone(),
            clock: deps.system.clock.clone(),
            thumbnail_repo: deps.storage.thumbnail_repo.clone(),
            thumbnail_generator: deps.storage.thumbnail_generator.clone(),
        }
    }
}

/// Spawn the four core blob-processing tasks through the provided `TaskRegistry`.
///
/// **Important**: The caller must keep the `BackgroundRuntimeDeps::spool_tx` sender
/// alive for the lifetime of the application — dropping it causes `SpoolerTask` to
/// exit immediately.  This function destructures the deps but does **not** consume
/// `spool_tx`; the caller should hold onto it.
///
/// This is an `async fn`; the caller decides how to enter the async context
/// (e.g. `tauri::async_runtime::spawn` vs `tokio::runtime::Handle::spawn`).
pub async fn spawn_blob_processing_tasks(
    background: BackgroundRuntimeDeps,
    ports: BlobProcessingPorts,
    task_registry: &Arc<TaskRegistry>,
) {
    let BackgroundRuntimeDeps {
        libp2p_network: _,
        representation_cache,
        spool_manager,
        spool_tx: _spool_tx, // Kept alive by the caller — we just need to not drop it here
        spool_rx,
        worker_rx,
        spool_dir,
        file_cache_dir: _,
        spool_ttl_days,
        worker_retry_max_attempts,
        worker_retry_backoff_ms,
        file_transfer_orchestrator: _,
        clipboard_write_coordinator: _,
    } = background;

    info!("Starting background clipboard spooler and blob worker");

    let BlobProcessingPorts {
        representation_repo,
        worker_tx,
        blob_writer,
        hasher,
        clock,
        thumbnail_repo,
        thumbnail_generator,
    } = ports;

    // --- Spool scanner (runs once at startup to recover pending representations) ---
    let scanner = SpoolScanner::new(spool_dir, representation_repo.clone(), worker_tx.clone());
    match scanner.scan_and_recover().await {
        Ok(recovered) => info!("Recovered {} representations from spool", recovered),
        Err(err) => warn!(error = %err, "Spool scan failed; continuing startup"),
    }

    // --- Spooler task (long-lived, channel-driven) ---
    let spooler = SpoolerTask::new(
        spool_rx,
        spool_manager.clone(),
        worker_tx,
        representation_cache.clone(),
    );
    task_registry
        .spawn("spooler", |_token| async move {
            spooler.run().await;
            warn!("SpoolerTask stopped");
        })
        .await;

    // --- Background blob worker (long-lived, channel-driven) ---
    let worker = BackgroundBlobWorker::new(
        worker_rx,
        representation_cache,
        spool_manager.clone(),
        representation_repo.clone(),
        blob_writer,
        hasher,
        thumbnail_repo,
        thumbnail_generator,
        clock.clone(),
        worker_retry_max_attempts,
        Duration::from_millis(worker_retry_backoff_ms),
    );
    task_registry
        .spawn("blob_worker", |_token| async move {
            worker.run().await;
            warn!("BackgroundBlobWorker stopped");
        })
        .await;

    // --- Spool janitor (long-lived, interval-based loop with cooperative cancellation) ---
    let janitor = SpoolJanitor::new(
        spool_manager.clone(),
        representation_repo.clone(),
        clock,
        spool_ttl_days,
    );
    task_registry
        .spawn("spool_janitor", |token| async move {
            let mut interval =
                tokio::time::interval(Duration::from_secs(SPOOL_JANITOR_INTERVAL_SECS));
            loop {
                tokio::select! {
                    _ = token.cancelled() => {
                        info!("Spool janitor shutting down");
                        return;
                    }
                    _ = interval.tick() => {
                        match janitor.run_once().await {
                            Ok(removed) => {
                                if removed > 0 {
                                    info!("Spool janitor removed {} expired entries", removed);
                                }
                            }
                            Err(err) => {
                                warn!(error = %err, "Spool janitor run failed");
                            }
                        }
                    }
                }
            }
        })
        .await;

    info!("Blob processing tasks registered with TaskRegistry");
}
