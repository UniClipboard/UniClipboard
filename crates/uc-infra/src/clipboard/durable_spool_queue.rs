//! Durable spool queue that writes bytes to disk before returning.
//!
//! `enqueue()` writes synchronously to the spool directory before notifying
//! the background blob worker, so bytes survive a process exit between
//! capture and blob materialisation.
//!
//! ## Durability guarantee
//!
//! `enqueue()` writes the bytes to the spool directory (via `SpoolManager`)
//! before returning. Only then does it notify the background blob worker.
//! If the process exits after `enqueue()` returns, `SpoolScanner` will find
//! the spool file on next startup and re-queue it to the worker.
//!
//! ## Failure semantics
//!
//! If the spool write fails (e.g., disk full), `enqueue()` returns `Err`.
//! The caller (`CaptureClipboardUseCase`) propagates the error, which means
//! the clipboard entry is still persisted in DB with `Staged` state but will
//! not be viewable until the spool write succeeds on a subsequent capture.
//! This preserves the previous error behaviour for disk-full scenarios.
//!
//! ## Worker notification
//!
//! After writing the spool file, `enqueue()` attempts a `try_send` to the
//! worker channel so the background blob worker can immediately begin
//! materialising the blob without waiting for the next startup scan.
//! A failed `try_send` (channel full) is logged but not treated as an error;
//! the spool scanner will recover the entry on next startup.

use std::sync::Arc;

use tokio::sync::mpsc;
use tracing::warn;
use uc_core::ids::RepresentationId;
use uc_core::ports::clipboard::{SpoolQueuePort, SpoolRequest};

use crate::clipboard::SpoolManager;

/// Durable spool queue: writes to disk synchronously, then notifies the worker.
pub struct DurableSpoolQueue {
    spool_manager: Arc<SpoolManager>,
    worker_tx: mpsc::Sender<RepresentationId>,
}

impl DurableSpoolQueue {
    pub fn new(
        spool_manager: Arc<SpoolManager>,
        worker_tx: mpsc::Sender<RepresentationId>,
    ) -> Self {
        Self {
            spool_manager,
            worker_tx,
        }
    }
}

#[async_trait::async_trait]
impl SpoolQueuePort for DurableSpoolQueue {
    async fn enqueue(&self, request: SpoolRequest) -> anyhow::Result<()> {
        // Write bytes to disk first — this is the durability guarantee.
        // If this fails, we return Err so the caller knows bytes are not safe.
        self.spool_manager
            .write(&request.rep_id, &request.bytes)
            .await
            .map_err(|err| {
                anyhow::anyhow!("failed to write spool file for {}: {}", request.rep_id, err)
            })?;

        // Notify the background worker to immediately process this entry.
        // A failure here is non-fatal: the spool file is on disk and will be
        // recovered by SpoolScanner on the next application startup.
        if let Err(err) = self.worker_tx.try_send(request.rep_id.clone()) {
            warn!(
                representation_id = %request.rep_id,
                error = %err,
                "Failed to notify worker after spool write; will be recovered on next startup"
            );
        }

        Ok(())
    }
}
