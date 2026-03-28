//! Durable spool queue that writes bytes to disk before returning.
//!
//! Replaces the in-memory `MpscSpoolQueue` to ensure spool bytes survive
//! process exits. The original `MpscSpoolQueue` only enqueued bytes into
//! an in-memory channel; if the app exited before `SpoolerTask` processed
//! the message, the bytes were permanently lost.
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

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use tokio::time::{timeout, Duration};
    use uc_core::ids::RepresentationId;
    use uc_core::ports::clipboard::{SpoolQueuePort, SpoolRequest};

    #[tokio::test]
    async fn enqueue_writes_to_disk_and_notifies_worker() -> Result<()> {
        let temp_dir = tempfile::tempdir()?;
        let spool_manager = Arc::new(SpoolManager::new(temp_dir.path(), 1_000_000)?);
        let (worker_tx, mut worker_rx) = mpsc::channel(4);
        let queue = DurableSpoolQueue::new(spool_manager.clone(), worker_tx);

        let rep_id = RepresentationId::new();
        let bytes = vec![1, 2, 3];

        queue
            .enqueue(SpoolRequest {
                rep_id: rep_id.clone(),
                bytes: bytes.clone(),
            })
            .await?;

        // Bytes must be on disk immediately after enqueue() returns.
        let on_disk = spool_manager.read(&rep_id).await?;
        assert_eq!(
            on_disk,
            Some(bytes),
            "spool file must be written synchronously"
        );

        // Worker must be notified.
        let notified = timeout(Duration::from_millis(50), worker_rx.recv()).await?;
        assert_eq!(notified, Some(rep_id));

        Ok(())
    }

    #[tokio::test]
    async fn enqueue_returns_error_on_spool_write_failure() -> Result<()> {
        let temp_dir = tempfile::tempdir()?;
        // max_bytes=0 means any write will fail (size > max_bytes check)
        let spool_manager = Arc::new(SpoolManager::new(temp_dir.path(), 0)?);
        let (worker_tx, _worker_rx) = mpsc::channel(4);
        let queue = DurableSpoolQueue::new(spool_manager, worker_tx);

        let result = queue
            .enqueue(SpoolRequest {
                rep_id: RepresentationId::new(),
                bytes: vec![1, 2, 3],
            })
            .await;

        assert!(result.is_err(), "enqueue must fail when spool write fails");
        Ok(())
    }

    #[tokio::test]
    async fn enqueue_succeeds_even_if_worker_channel_is_full() -> Result<()> {
        let temp_dir = tempfile::tempdir()?;
        let spool_manager = Arc::new(SpoolManager::new(temp_dir.path(), 1_000_000)?);
        // Create a worker channel with capacity 0 effectively (capacity 1, already filled)
        let (worker_tx, _worker_rx) = mpsc::channel(1);
        // Fill the channel so try_send will fail
        let _ = worker_tx.try_send(RepresentationId::new());

        let queue = DurableSpoolQueue::new(spool_manager.clone(), worker_tx);

        let rep_id = RepresentationId::new();
        let bytes = vec![4, 5, 6];

        // enqueue() must succeed even with a full worker channel
        let result = queue
            .enqueue(SpoolRequest {
                rep_id: rep_id.clone(),
                bytes: bytes.clone(),
            })
            .await;

        assert!(
            result.is_ok(),
            "enqueue must succeed even when worker channel is full"
        );

        // Bytes must still be on disk.
        let on_disk = spool_manager.read(&rep_id).await?;
        assert_eq!(on_disk, Some(bytes));

        Ok(())
    }
}
