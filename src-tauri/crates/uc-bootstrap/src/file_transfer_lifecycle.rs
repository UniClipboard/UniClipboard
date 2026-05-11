//! File-transfer lifecycle wiring.
//!
//! Wires the durable event store + host-event publisher + receiver-side
//! projection plumbing + runtime-health tasks (timeout sweep + startup
//! reconcile). The 5 lifecycle use cases (Start / ReportProgress / Complete
//! / Fail / Cancel) live inside [`FileTransferFacade`] (application layer)
//! built alongside this lifecycle by [`build_file_transfer_assembly`] —
//! external callers reach those actions through the facade, not through
//! the lifecycle struct.

use std::sync::{Arc, RwLock};
use std::time::Duration;

use tokio::task::JoinHandle;
use tracing::{info, info_span, warn, Instrument};

use uc_application::facade::{
    FileTransferFacade, FileTransferFacadeDeps, FileTransferHostEventPublisher, HostEvent,
    HostEventEmitterPort, OutboundEntryIdCache, TransferHostEvent,
};
use uc_core::file_transfer::{FileTransferEventPublisherPort, FileTransferEventStorePort};
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
/// `spawn_timeout_sweep` and `reconcile_on_startup` currently operate
/// directly on `FileTransferRepositoryPort` (projection table), not through
/// the domain event store. Reason: failing a pending-timeout transfer
/// through the event timeline requires a `peer_id`, which the pending row
/// does not yet have (no `Started` event occurred). Re-threading this
/// through the event store would require domain-model changes to support a
/// peer-less failure scenario, which is deferred to the Phase 5 cleanup. In
/// the meantime this preserves the legacy behavior one-to-one.
pub struct FileTransferLifecycle {
    pub outbound_entry_cache: Arc<OutboundEntryIdCache>,
    /// Shared host-event emitter cell.
    ///
    /// Exposed so receiver-side workers can publish UI-facing `pending` status
    /// events directly after seeding the receiver projection — this bypasses
    /// the domain event bus on purpose, since `pending` is a presentation-layer
    /// preview, not a domain fact (there is no `Announced` event in the
    /// timeline).
    pub emitter_cell: Arc<RwLock<Arc<dyn HostEventEmitterPort>>>,

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
    ) -> JoinHandle<()> {
        let repo = Arc::clone(&self.file_transfer_repo);
        let clock = Arc::clone(&self.clock);
        let emitter_cell: Arc<RwLock<Arc<dyn HostEventEmitterPort>>> =
            Arc::clone(&self.emitter_cell);

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

                    warn!(
                        count = expired.len(),
                        "Timeout sweep found expired in-flight transfers"
                    );

                    let emitter = emitter_cell
                        .read()
                        .unwrap_or_else(|p| p.into_inner())
                        .clone();

                    for t in &expired {
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

                        if let Err(err) =
                            emitter.emit(HostEvent::Transfer(TransferHostEvent::StatusChanged {
                                transfer_id: t.transfer_id.clone(),
                                entry_id: t.entry_id.clone(),
                                status: "failed".to_string(),
                                reason: Some(reason.to_string()),
                            }))
                        {
                            warn!(error = %err, "Failed to emit timeout failure status");
                        }
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
        let emitter = self
            .emitter_cell
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .clone();

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

        warn!(
            count = cleanup_targets.len(),
            "Reconciled orphaned in-flight transfers at startup"
        );

        for t in &cleanup_targets {
            cleanup_cached_path(&t.cached_path).await;

            if let Err(err) = emitter.emit(HostEvent::Transfer(TransferHostEvent::StatusChanged {
                transfer_id: t.transfer_id.clone(),
                entry_id: t.entry_id.clone(),
                status: "failed".to_string(),
                reason: Some(reason.to_string()),
            })) {
                warn!(error = %err, "Failed to emit reconciliation status");
            }
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
    emitter_cell: Arc<RwLock<Arc<dyn HostEventEmitterPort>>>,
    file_transfer_repo: Arc<dyn FileTransferRepositoryPort>,
    clock: Arc<dyn ClockPort>,
) -> FileTransferAssembly {
    let outbound_entry_cache = Arc::new(OutboundEntryIdCache::new());

    let publisher = Arc::new(FileTransferHostEventPublisher::new(
        Arc::clone(&emitter_cell),
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
        emitter_cell,
        file_transfer_repo,
        clock,
    });

    FileTransferAssembly { lifecycle, facade }
}
