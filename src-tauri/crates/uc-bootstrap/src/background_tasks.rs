//! Shared background blob-processing tasks.
//!
//! The core tasks (SpoolScanner, BackgroundBlobWorker, SpoolJanitor) are needed by both
//! the GUI and daemon entry points.  This module provides a single
//! `spawn_blob_processing_tasks()` that callers `await` inside whatever spawn mechanism
//! they use (tauri::async_runtime::spawn for GUI, rt.spawn for daemon).

use std::sync::Arc;
use std::time::Duration;

use tracing::{info, warn};

use tokio::sync::mpsc;
use uc_application::deps::AppDeps;

use crate::task_registry::TaskRegistry;
use uc_core::ids::RepresentationId;
use uc_core::ports::clipboard::{
    ClipboardRepresentationRepositoryPort, ThumbnailGeneratorPort, ThumbnailRepositoryPort,
};
use uc_core::ports::{ClockPort, ContentHashPort};
use uc_infra::blob::BlobWriterPort;
use uc_infra::clipboard::{BackgroundBlobWorker, SpoolJanitor, SpoolScanner, StagedReconciler};

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

/// Spawn the core blob-processing tasks through the provided `TaskRegistry`.
///
/// This is an `async fn`; the caller decides how to enter the async context
/// (e.g. `tauri::async_runtime::spawn` vs `tokio::runtime::Handle::spawn`).
pub async fn spawn_blob_processing_tasks(
    background: BackgroundRuntimeDeps,
    ports: BlobProcessingPorts,
    task_registry: &Arc<TaskRegistry>,
) {
    let BackgroundRuntimeDeps {
        representation_cache,
        spool_manager,
        worker_rx,
        spool_dir,
        file_cache_dir: _,
        spool_ttl_days,
        worker_retry_max_attempts,
        worker_retry_backoff_ms,
        file_transfer_lifecycle: _,
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
    //
    // SpoolScanner walks the spool directory: spool file → DB. Re-queues any
    // file whose representation is still Staged/Processing; deletes orphaned
    // files whose representation is gone or moved past Staged.
    let scanner = SpoolScanner::new(spool_dir, representation_repo.clone(), worker_tx.clone());
    match scanner.scan_and_recover().await {
        Ok(recovered) => info!("Recovered {} representations from spool", recovered),
        Err(err) => warn!(error = %err, "Spool scan failed; continuing startup"),
    }

    // --- Staged reconciler (runs once at startup to demote orphaned reps) ---
    //
    // Dual of SpoolScanner: walks Staged/Processing reps in DB → spool file.
    // Demotes any rep whose spool file is missing (cache is empty at startup,
    // so spool presence is the sole liveness signal). Cleans up the orphaned-
    // Staged class of bug that produced UNICLIPBOARD-RUST-5/6 on alpha users'
    // machines before P1-4's synchronous-spool fix landed.
    //
    // Must run AFTER SpoolScanner so any spool file SpoolScanner re-queued
    // gets a chance to be observed; running before would race a worker that
    // could promote the rep out of Staged mid-sweep.
    let reconciler = StagedReconciler::new(representation_repo.clone(), spool_manager.clone());
    match reconciler.run_once().await {
        Ok(0) => {}
        Ok(demoted) => info!(
            "Staged reconciler demoted {} orphaned representations",
            demoted
        ),
        Err(err) => warn!(error = %err, "Staged reconciler failed; continuing startup"),
    }

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
