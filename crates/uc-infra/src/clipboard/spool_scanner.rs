//! Spool scanner for recovery.
//! 用于恢复的磁盘缓存扫描器。

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use tokio::fs;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};
use uc_core::clipboard::PayloadAvailability;
use uc_core::ids::RepresentationId;
use uc_core::ports::ClipboardRepresentationRepositoryPort;

/// Scans spool directory and re-queues recoverable representations.
/// 扫描磁盘缓存目录并重新入队可恢复的表示。
pub struct SpoolScanner {
    spool_dir: PathBuf,
    repo: Arc<dyn ClipboardRepresentationRepositoryPort>,
    worker_tx: mpsc::Sender<RepresentationId>,
}

impl SpoolScanner {
    /// Create a new scanner.
    /// 创建新的扫描器。
    pub fn new(
        spool_dir: PathBuf,
        repo: Arc<dyn ClipboardRepresentationRepositoryPort>,
        worker_tx: mpsc::Sender<RepresentationId>,
    ) -> Self {
        Self {
            spool_dir,
            repo,
            worker_tx,
        }
    }

    /// Scan spool directory and recover queued items.
    /// 扫描磁盘缓存并恢复待处理项。
    pub async fn scan_and_recover(&self) -> Result<usize> {
        self.scan_and_recover_dir(&self.spool_dir).await
    }

    async fn scan_and_recover_dir(&self, spool_dir: &PathBuf) -> Result<usize> {
        let mut entries = fs::read_dir(spool_dir)
            .await
            .with_context(|| format!("Failed to read spool dir: {}", spool_dir.display()))?;

        let mut recovered = 0usize;

        while let Some(entry) = entries.next_entry().await? {
            let file_type = entry.file_type().await?;
            if !file_type.is_file() {
                continue;
            }

            let file_name = entry.file_name();
            let Some(file_name_str) = file_name.to_str() else {
                debug!("Skipping spool entry with non-utf8 filename");
                continue;
            };

            if file_name_str.is_empty() {
                debug!("Skipping spool entry with empty filename");
                continue;
            }

            let rep_id = RepresentationId::from(file_name_str);

            match self.repo.get_representation_by_id(&rep_id).await? {
                Some(rep) => match rep.payload_state() {
                    PayloadAvailability::Staged | PayloadAvailability::Processing => {
                        match self.worker_tx.try_send(rep_id.clone()) {
                            Ok(()) => {
                                recovered += 1;
                            }
                            Err(err) => {
                                warn!(
                                    representation_id = %rep_id,
                                    error = %err,
                                    "Failed to re-queue representation during recovery"
                                );
                            }
                        }
                    }
                    _ => {
                        let path = entry.path();
                        if let Err(err) = fs::remove_file(&path).await {
                            warn!(
                                representation_id = %rep_id,
                                error = %err,
                                "Failed to delete stale spool file"
                            );
                        }
                    }
                },
                None => {
                    let path = entry.path();
                    debug!(
                        representation_id = %rep_id,
                        "Representation missing for spool entry; deleting stale file"
                    );
                    if let Err(err) = fs::remove_file(&path).await {
                        warn!(
                            representation_id = %rep_id,
                            error = %err,
                            "Failed to delete orphaned spool file"
                        );
                    }
                }
            }
        }

        info!(
            spool_dir = %spool_dir.display(),
            "Spool scan completed; recovered {recovered} items"
        );
        Ok(recovered)
    }
}
