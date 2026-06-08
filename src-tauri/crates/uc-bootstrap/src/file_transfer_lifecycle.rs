//! File-transfer lifecycle wiring.
//!
//! Wires the durable event store + host-event publisher + receiver-side
//! projection plumbing + runtime-health tasks (timeout sweep + startup
//! reconcile). The 5 lifecycle use cases (Start / ReportProgress / Complete
//! / Fail / Cancel) live inside [`FileTransferFacade`] (application layer)
//! built alongside this lifecycle by [`build_file_transfer_assembly`] —
//! external callers reach those actions through the facade, not through
//! the lifecycle struct.

use std::sync::Arc;
use std::time::Duration;

use tokio::task::JoinHandle;
use tracing::{info, info_span, warn, Instrument};

use uc_application::facade::{
    BlobTransferFacade, FileTransferFacade, FileTransferFacadeDeps, FileTransferHostEventPublisher,
    HostEvent, HostEventBus, InboundCancelOutcome, OutboundEntryIdCache, TransferHostEvent,
};
use uc_core::file_transfer::{
    FileTransferCancellationReason, FileTransferEventPublisherPort, FileTransferEventStorePort,
};
use uc_core::ports::file_transfer_repository::TrackedFileTransferStatus;
use uc_core::ports::{ClockPort, FileTransferRepositoryPort};
use uc_infra::db::executor::DieselSqliteExecutor;
use uc_infra::file_transfer::SqliteReceiverFileTransferStore;

pub type FileTransferEventStore = SqliteReceiverFileTransferStore<Arc<DieselSqliteExecutor>>;

/// Pending rows abandoned for longer than this are considered stalled and
/// force-failed by the sweep.
const PENDING_TIMEOUT_MS: i64 = 60_000;
/// Transferring rows with no new activity within this window are force-failed.
const TRANSFERRING_TIMEOUT_MS: i64 = 5 * 60_000;
/// Sweep frequency.
const SWEEP_INTERVAL: Duration = Duration::from_secs(15);

/// Wraps the receiver-side projection / publisher / outbound entry cache and
/// the periodic health tasks (timeout sweep + startup reconcile).
///
/// `outbound_entry_cache` is exposed so the sender-side worker can seed
/// `transfer_id → entry_id` hints; the publisher already reads it through
/// its fallback path.
///
/// ## Sweep / reconcile path
///
/// The sweep branches on the row's tracked status:
///
/// - **Transferring** rows route through
///   [`BlobTransferFacade::cancel_inbound_transfer`]: that tears down the
///   receiver-side iroh-blobs fetch task + QUIC connection AND appends a
///   `Cancelled { reason: Timeout }` domain event whose projection flips
///   the row to `cancelled`. This is the path that actually closes the
///   sender → receiver tap (the original bug — receiver "timed out"
///   locally while the sender provider kept streaming).
/// - **Pending** rows (no `Started` event yet, no `peer_id` available)
///   stay on the legacy `mark_failed` + manual host-event path: appending
///   a peer-less `Cancelled`/`Failed` to the timeline is a domain-model
///   change that belongs to the Phase 5 cleanup, not P1.
///
/// `reconcile_on_startup` always uses the legacy path: by definition the
/// runtime is not yet up, so there is no in-flight fetch to cancel.
pub struct FileTransferLifecycle {
    pub outbound_entry_cache: Arc<OutboundEntryIdCache>,
    /// Shared host-event bus.
    ///
    /// Exposed so receiver-side workers can publish UI-facing `pending` status
    /// events directly after seeding the receiver projection — this bypasses
    /// the domain event bus on purpose, since `pending` is a presentation-layer
    /// preview, not a domain fact (there is no `Announced` event in the
    /// timeline).
    pub host_event_bus: Arc<HostEventBus>,

    file_transfer_repo: Arc<dyn FileTransferRepositoryPort>,
    clock: Arc<dyn ClockPort>,
}

/// Assembled file-transfer plumbing returned by
/// [`build_file_transfer_assembly`].
///
/// Hands the composition root both halves at once: the runtime-health
/// `lifecycle` (sweep / reconcile workers) and the application-layer
/// `facade` that exposes the 5 lifecycle actions plus seed / link.
pub struct FileTransferAssembly {
    pub lifecycle: Arc<FileTransferLifecycle>,
    pub facade: Arc<FileTransferFacade>,
}

impl FileTransferLifecycle {
    /// Spawn a periodic timeout sweep.
    ///
    /// Runs every 15 seconds. Fails stalled pending (>60s) and transferring
    /// (>5min) rows, emits `TransferHostEvent::StatusChanged`, and cleans the
    /// partial cache artifacts on disk.
    pub fn spawn_timeout_sweep(
        &self,
        cancel: tokio::sync::watch::Receiver<bool>,
        blob_transfer: Arc<BlobTransferFacade>,
    ) -> JoinHandle<()> {
        let repo = Arc::clone(&self.file_transfer_repo);
        let clock = Arc::clone(&self.clock);
        let bus = Arc::clone(&self.host_event_bus);

        tokio::spawn(
            async move {
                let mut interval = tokio::time::interval(SWEEP_INTERVAL);
                let mut cancel = cancel;

                loop {
                    tokio::select! {
                        _ = interval.tick() => {},
                        _ = cancel.changed() => {
                            if *cancel.borrow() {
                                info!("File transfer timeout sweep shutting down");
                                return;
                            }
                        }
                    }

                    let now_ms = clock.now_ms();
                    let pending_cutoff = now_ms - PENDING_TIMEOUT_MS;
                    let transferring_cutoff = now_ms - TRANSFERRING_TIMEOUT_MS;

                    let expired = match repo
                        .list_expired_inflight(pending_cutoff, transferring_cutoff)
                        .await
                    {
                        Ok(list) => list,
                        Err(err) => {
                            warn!(error = %err, "Timeout sweep query failed");
                            continue;
                        }
                    };

                    if expired.is_empty() {
                        continue;
                    }

                    info!(
                        count = expired.len(),
                        "Timeout sweep found expired in-flight transfers"
                    );

                    for t in &expired {
                        // Transferring rows have a peer_id + an in-flight
                        // fetch — route them through the facade so the
                        // receiver-side iroh-blobs task and QUIC
                        // connection are actually torn down, and the
                        // Cancelled domain event flows via projection.
                        // If the registry race-lost the entry (fetch
                        // already exited but row is still expired in the
                        // projection), fall through to the legacy
                        // mark_failed path.
                        if matches!(t.status, TrackedFileTransferStatus::Transferring) {
                            match blob_transfer
                                .cancel_inbound_transfer(
                                    &t.transfer_id,
                                    FileTransferCancellationReason::Timeout,
                                )
                                .await
                            {
                                Ok(InboundCancelOutcome::Cancelled) => {
                                    cleanup_cached_path(&t.cached_path).await;
                                    continue;
                                }
                                Ok(InboundCancelOutcome::NotInflight) => {
                                    // fall through to mark_failed
                                }
                                Err(err) => {
                                    warn!(
                                        error = %err,
                                        transfer_id = %t.transfer_id,
                                        "Timeout sweep: cancel_inbound_transfer failed, falling back to mark_failed"
                                    );
                                }
                            }
                        }

                        let reason = timeout_reason_for(t.status);

                        if let Err(err) = repo.mark_failed(&t.transfer_id, reason, now_ms).await {
                            warn!(
                                error = %err,
                                transfer_id = %t.transfer_id,
                                "Failed to mark expired transfer as failed"
                            );
                            continue;
                        }

                        cleanup_cached_path(&t.cached_path).await;

                        bus.emit_or_warn(HostEvent::Transfer(TransferHostEvent::StatusChanged {
                            transfer_id: t.transfer_id.clone(),
                            entry_id: t.entry_id.clone(),
                            status: "failed".to_string(),
                            reason: Some(reason.to_string()),
                        }));
                    }
                }
            }
            .instrument(info_span!("file_transfer.timeout_sweep")),
        )
    }

    /// Run startup reconciliation: mark orphaned in-flight transfers as
    /// failed and clean their cache artifacts.
    ///
    /// Non-blocking and non-fatal: errors are logged as warnings.
    pub async fn reconcile_on_startup(&self) {
        let now_ms = self.clock.now_ms();
        let reason = "orphaned: app restarted while transfer was in-flight";

        let cleanup_targets = match self
            .file_transfer_repo
            .bulk_fail_inflight(reason, now_ms)
            .instrument(info_span!("file_transfer.startup_reconcile"))
            .await
        {
            Ok(targets) => targets,
            Err(err) => {
                warn!(error = %err, "Startup reconciliation failed (non-fatal)");
                return;
            }
        };

        if cleanup_targets.is_empty() {
            info!("No orphaned in-flight transfers found at startup");
            return;
        }

        info!(
            count = cleanup_targets.len(),
            "Reconciled orphaned in-flight transfers at startup"
        );

        for t in &cleanup_targets {
            cleanup_cached_path(&t.cached_path).await;

            self.host_event_bus.emit_or_warn(HostEvent::Transfer(
                TransferHostEvent::StatusChanged {
                    transfer_id: t.transfer_id.clone(),
                    entry_id: t.entry_id.clone(),
                    status: "failed".to_string(),
                    reason: Some(reason.to_string()),
                },
            ));
        }
    }
}

fn timeout_reason_for(status: TrackedFileTransferStatus) -> &'static str {
    match status {
        TrackedFileTransferStatus::Pending => "timeout: no data received within 60 seconds",
        TrackedFileTransferStatus::Transferring => {
            "timeout: no new chunk received within 5 minutes"
        }
        _ => "timeout: stalled transfer",
    }
}

/// Best-effort cleanup of a cached file or transfer directory.
async fn cleanup_cached_path(cached_path: &str) {
    if cached_path.is_empty() {
        return;
    }

    let path = std::path::Path::new(cached_path);

    if path.is_file() {
        if let Err(err) = tokio::fs::remove_file(path).await {
            warn!(error = %err, path = %cached_path, "Failed to remove cached file");
        }
    }

    if let Some(parent) = path.parent() {
        // Only remove parent if it looks like a per-transfer directory — avoid
        // accidentally deleting the shared cache root. The heuristic matches
        // the previous orchestrator behavior.
        if parent.is_dir() {
            if let Ok(mut entries) = tokio::fs::read_dir(parent).await {
                if entries.next_entry().await.ok().flatten().is_none() {
                    if let Err(err) = tokio::fs::remove_dir(parent).await {
                        warn!(
                            error = %err,
                            path = %parent.display(),
                            "Failed to remove empty transfer directory"
                        );
                    }
                }
            }
        }
    }
}

pub fn build_file_transfer_assembly(
    store: Arc<FileTransferEventStore>,
    host_event_bus: Arc<HostEventBus>,
    file_transfer_repo: Arc<dyn FileTransferRepositoryPort>,
    clock: Arc<dyn ClockPort>,
) -> FileTransferAssembly {
    let outbound_entry_cache = Arc::new(OutboundEntryIdCache::new());

    let publisher = Arc::new(FileTransferHostEventPublisher::new(
        Arc::clone(&host_event_bus),
        Arc::clone(&file_transfer_repo),
        Arc::clone(&outbound_entry_cache),
    ));

    let store_port: Arc<dyn FileTransferEventStorePort> = store as _;
    let publisher_port: Arc<dyn FileTransferEventPublisherPort> = Arc::clone(&publisher) as _;

    let facade = Arc::new(FileTransferFacade::new(FileTransferFacadeDeps {
        store: store_port,
        publisher: publisher_port,
        repo: Arc::clone(&file_transfer_repo),
        clock: Arc::clone(&clock),
        host_publisher: Some(Arc::clone(&publisher)),
    }));

    let lifecycle = Arc::new(FileTransferLifecycle {
        outbound_entry_cache,
        host_event_bus,
        file_transfer_repo,
        clock,
    });

    FileTransferAssembly { lifecycle, facade }
}
