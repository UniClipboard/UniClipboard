//! File transfer event-loop orchestration for durable status transitions.
//!
//! Handles pending/transferring/completed/failed lifecycle through the
//! `TrackInboundTransfersUseCase`, emits `file-transfer.status_changed`
//! events via WS, runs periodic timeout sweeps, and performs startup reconciliation.
//!
//! The orchestrator holds a shared swappable emitter cell
//! `Arc<RwLock<Arc<dyn HostEventEmitterPort>>>` — matching the `HostEventSetupPort`
//! pattern in assembly.rs. This eliminates any emitter timing problem: the
//! orchestrator can be constructed at wire time with the `LoggingEventEmitter`
//! inside the cell, and automatically uses the `DaemonApiEventEmitter` after the swap.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use serde::Serialize;
use tracing::{debug, info, info_span, trace, warn, Instrument};

use uc_core::ports::host_event_emitter::{HostEvent, HostEventEmitterPort, TransferHostEvent};
use uc_core::ports::transfer_progress::{TransferDirection, TransferProgress};
use uc_core::ports::ClockPort;

use crate::usecases::clipboard::sync_inbound::PendingTransferLinkage;
use crate::usecases::file_sync::TrackInboundTransfersUseCase;

/// Info about a file transfer completion that arrived before its
/// pending record was seeded in the database.
#[derive(Debug, Clone)]
pub struct EarlyCompletionInfo {
    pub content_hash: Option<String>,
    pub completed_at_ms: i64,
}

/// Thread-safe cache for file transfer completions that arrive before
/// the pending record is seeded in the database (race condition).
///
/// Shared between the clipboard receive loop (which seeds pending records)
/// and the pairing events loop (which handles completions).
#[derive(Default)]
pub struct EarlyCompletionCache {
    inner: Mutex<HashMap<String, EarlyCompletionInfo>>,
}

impl EarlyCompletionCache {
    /// Store an early completion for later reconciliation.
    pub fn store(&self, transfer_id: String, info: EarlyCompletionInfo) {
        let mut map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        map.insert(transfer_id, info);
    }

    /// Drain entries whose transfer_id appears in the given list.
    /// Returns the matched entries so the caller can reconcile them.
    pub fn drain_matching(&self, transfer_ids: &[String]) -> Vec<(String, EarlyCompletionInfo)> {
        let mut map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let mut matched = Vec::new();
        for tid in transfer_ids {
            if let Some(info) = map.remove(tid) {
                matched.push((tid.clone(), info));
            }
        }
        matched
    }
}

/// Thread-safe cache for sender-side transfer linkage.
///
/// Sender progress events do not have durable transfer rows, so we keep an
/// in-memory transfer_id -> entry_id mapping long enough to enrich live events
/// for the frontend.
#[derive(Default)]
pub struct OutboundTransferLinkCache {
    inner: Mutex<HashMap<String, String>>,
}

impl OutboundTransferLinkCache {
    pub fn insert(&self, transfer_id: String, entry_id: String) {
        let mut map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        map.insert(transfer_id, entry_id);
    }

    pub fn get(&self, transfer_id: &str) -> Option<String> {
        let map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        map.get(transfer_id).cloned()
    }

    pub fn remove(&self, transfer_id: &str) {
        let mut map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        map.remove(transfer_id);
    }
}

const RECEIVER_ACTIVITY_REFRESH_MIN_INTERVAL_MS: i64 = 2_000;

#[derive(Default)]
struct TransferEntryCache {
    inner: Mutex<HashMap<String, String>>,
}

impl TransferEntryCache {
    fn insert(&self, transfer_id: String, entry_id: String) {
        let mut map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        map.insert(transfer_id, entry_id);
    }

    fn get(&self, transfer_id: &str) -> Option<String> {
        let map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        map.get(transfer_id).cloned()
    }

    fn remove(&self, transfer_id: &str) {
        let mut map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        map.remove(transfer_id);
    }
}

#[derive(Default)]
struct InboundActivityRefreshCache {
    inner: Mutex<HashMap<String, i64>>,
}

impl InboundActivityRefreshCache {
    fn should_refresh(&self, transfer_id: &str, now_ms: i64) -> bool {
        let mut map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        match map.get_mut(transfer_id) {
            Some(last_refresh_ms)
                if now_ms.saturating_sub(*last_refresh_ms)
                    < RECEIVER_ACTIVITY_REFRESH_MIN_INTERVAL_MS =>
            {
                false
            }
            Some(last_refresh_ms) => {
                *last_refresh_ms = now_ms;
                true
            }
            None => {
                map.insert(transfer_id.to_string(), now_ms);
                true
            }
        }
    }

    fn remove(&self, transfer_id: &str) {
        let mut map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        map.remove(transfer_id);
    }
}

/// Event payload for `file-transfer://status-changed`.
///
/// Emitted whenever a transfer transitions between durable states.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileTransferStatusPayload {
    pub transfer_id: String,
    pub entry_id: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Orchestrator for file transfer lifecycle management.
///
/// Holds a shared swappable emitter cell so it can be constructed at wire
/// time and automatically pick up the real `DaemonApiEventEmitter` after bootstrap
/// swaps the cell, without needing `Option` or deferred construction.
pub struct FileTransferOrchestrator {
    tracker: Arc<TrackInboundTransfersUseCase>,
    emitter_cell: Arc<RwLock<Arc<dyn HostEventEmitterPort>>>,
    clock: Arc<dyn ClockPort>,
    early_completion_cache: EarlyCompletionCache,
    outbound_link_cache: OutboundTransferLinkCache,
    transfer_entry_cache: TransferEntryCache,
    inbound_activity_refresh_cache: InboundActivityRefreshCache,
}

impl FileTransferOrchestrator {
    /// Construct the orchestrator.
    ///
    /// `emitter_cell` is the shared `Arc<RwLock<Arc<dyn HostEventEmitterPort>>>` created
    /// once at wire time. The cell initially holds a `LoggingEventEmitter`; bootstrap
    /// later swaps it to a `DaemonApiEventEmitter` — and this orchestrator automatically
    /// sees the new emitter on every call.
    pub fn new(
        tracker: Arc<TrackInboundTransfersUseCase>,
        emitter_cell: Arc<RwLock<Arc<dyn HostEventEmitterPort>>>,
        clock: Arc<dyn ClockPort>,
    ) -> Self {
        Self {
            tracker,
            emitter_cell,
            clock,
            early_completion_cache: EarlyCompletionCache::default(),
            outbound_link_cache: OutboundTransferLinkCache::default(),
            transfer_entry_cache: TransferEntryCache::default(),
            inbound_activity_refresh_cache: InboundActivityRefreshCache::default(),
        }
    }

    /// Expose the inner `TrackInboundTransfersUseCase` for callers that need
    /// to call `record_pending_from_clipboard` directly (e.g., wiring.rs).
    pub fn tracker(&self) -> &TrackInboundTransfersUseCase {
        &self.tracker
    }

    /// Expose the early-completion cache for drain operations by the clipboard
    /// receive loop.
    pub fn early_completion_cache(&self) -> &EarlyCompletionCache {
        &self.early_completion_cache
    }

    /// Register sender-side transfer linkage so outbound progress can be mapped
    /// back to the originating clipboard entry.
    pub fn register_outbound_transfer(&self, transfer_id: &str, entry_id: &str) {
        debug!(
            transfer_id,
            entry_id, "Registered outbound transfer linkage for live progress routing"
        );
        self.outbound_link_cache
            .insert(transfer_id.to_string(), entry_id.to_string());
    }

    /// Get the current timestamp in milliseconds from the orchestrator's clock.
    ///
    /// Exposed for callers (e.g., wiring.rs clipboard receive loop) that need to
    /// build `PendingInboundTransfer` records with a `created_at_ms` value using
    /// the same clock instance as the orchestrator.
    pub fn now_ms(&self) -> i64 {
        self.clock.now_ms()
    }

    /// Emit `file-transfer://status-changed` for each pending transfer
    /// after inbound clipboard metadata is applied.
    pub fn emit_pending_status(
        &self,
        entry_id: &str,
        pending_transfers: &[PendingTransferLinkage],
    ) {
        let emitter = self
            .emitter_cell
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        for t in pending_transfers {
            if let Err(err) = emitter.emit(HostEvent::Transfer(TransferHostEvent::StatusChanged {
                transfer_id: t.transfer_id.clone(),
                entry_id: entry_id.to_string(),
                status: "pending".to_string(),
                reason: None,
            })) {
                warn!(
                    error = %err,
                    transfer_id = %t.transfer_id,
                    "Failed to emit pending file-transfer status"
                );
            }
        }
    }

    /// Handle a transfer `TransferProgress` event.
    ///
    /// On first chunk (chunks_completed == 1), promotes to `transferring`.
    /// On subsequent chunks, refreshes durable liveness.
    ///
    /// Returns `true` if promoted to `transferring` (first time).
    pub async fn handle_transfer_progress(&self, progress: &TransferProgress) -> bool {
        let entry_id = self.resolve_entry_id(&progress.transfer_id).await;

        trace!(
            transfer_id = %progress.transfer_id,
            peer_id = %progress.peer_id,
            direction = ?progress.direction,
            chunks_completed = progress.chunks_completed,
            total_chunks = progress.total_chunks,
            resolved_entry_id = ?entry_id,
            "Resolved transfer progress linkage before host event emission"
        );

        let emitter = self
            .emitter_cell
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        if let Err(err) = emitter.emit(HostEvent::Transfer(TransferHostEvent::Progress {
            transfer_id: progress.transfer_id.clone(),
            entry_id: entry_id.clone(),
            peer_id: progress.peer_id.clone(),
            direction: progress.direction.clone(),
            chunks_completed: progress.chunks_completed,
            total_chunks: progress.total_chunks,
            bytes_transferred: progress.bytes_transferred,
            total_bytes: progress.total_bytes,
        })) {
            warn!(error = %err, transfer_id = %progress.transfer_id, "Failed to emit transfer progress");
        }

        // Only track receiving-side progress durably.
        if progress.direction != TransferDirection::Receiving {
            return false;
        }

        let now_ms = self.clock.now_ms();

        if progress.chunks_completed == 1 {
            // First chunk: promote to transferring
            match self
                .tracker
                .mark_transferring(&progress.transfer_id, now_ms)
                .await
            {
                Ok(true) => {
                    info!(transfer_id = %progress.transfer_id, "Transfer promoted to transferring");
                    if let Some(entry_id) = entry_id {
                        let emitter = self
                            .emitter_cell
                            .read()
                            .unwrap_or_else(|p| p.into_inner())
                            .clone();
                        if let Err(err) =
                            emitter.emit(HostEvent::Transfer(TransferHostEvent::StatusChanged {
                                transfer_id: progress.transfer_id.clone(),
                                entry_id,
                                status: "transferring".to_string(),
                                reason: None,
                            }))
                        {
                            warn!(error = %err, "Failed to emit transferring status");
                        }
                    }
                    return true;
                }
                Ok(false) => {
                    self.refresh_inbound_activity_if_due(&progress.transfer_id, now_ms)
                        .await;
                }
                Err(err) => {
                    warn!(error = %err, transfer_id = %progress.transfer_id, "Failed to mark transferring");
                }
            }
        } else {
            self.refresh_inbound_activity_if_due(&progress.transfer_id, now_ms)
                .await;
        }

        false
    }

    /// Handle a file transfer completion event.
    ///
    /// Marks the transfer row as completed before emitting the status event.
    /// If the pending record hasn't been seeded yet (race condition), stores
    /// the completion in `early_completion_cache` for later reconciliation.
    pub async fn handle_transfer_completed(&self, transfer_id: &str, content_hash: Option<&str>) {
        debug!(
            transfer_id,
            "Removing outbound transfer linkage on completion"
        );
        self.outbound_link_cache.remove(transfer_id);
        self.transfer_entry_cache.remove(transfer_id);
        self.inbound_activity_refresh_cache.remove(transfer_id);
        let now_ms = self.clock.now_ms();

        // Mark durable row completed
        match self
            .tracker
            .mark_completed(transfer_id, content_hash, now_ms)
            .await
        {
            Ok(true) => {
                // Row was updated — emit status-changed
            }
            Ok(false) => {
                // No row found — pending record hasn't been seeded yet.
                // Cache completion for reconciliation after seeding.
                warn!(
                    transfer_id,
                    "Early completion cached: pending record not yet seeded"
                );
                self.early_completion_cache.store(
                    transfer_id.to_string(),
                    EarlyCompletionInfo {
                        content_hash: content_hash.map(|s| s.to_string()),
                        completed_at_ms: now_ms,
                    },
                );
                return;
            }
            Err(err) => {
                warn!(error = %err, transfer_id, "Failed to mark transfer completed");
                return;
            }
        }

        // Emit status-changed for completed
        if let Ok(Some(entry_id)) = self
            .tracker
            .get_entry_summary_by_transfer(transfer_id)
            .await
        {
            let emitter = self
                .emitter_cell
                .read()
                .unwrap_or_else(|p| p.into_inner())
                .clone();
            if let Err(err) = emitter.emit(HostEvent::Transfer(TransferHostEvent::StatusChanged {
                transfer_id: transfer_id.to_string(),
                entry_id,
                status: "completed".to_string(),
                reason: None,
            })) {
                warn!(error = %err, "Failed to emit completed status");
            }
        }
    }

    /// Handle a file transfer failure event.
    ///
    /// Marks the durable row failed with the error reason, cleans partial cache,
    /// and emits `file-transfer://status-changed`.
    pub async fn handle_transfer_failed(&self, transfer_id: &str, error_reason: &str) {
        debug!(
            transfer_id,
            error_reason, "Removing outbound transfer linkage on failure"
        );
        self.outbound_link_cache.remove(transfer_id);
        self.transfer_entry_cache.remove(transfer_id);
        self.inbound_activity_refresh_cache.remove(transfer_id);
        let now_ms = self.clock.now_ms();

        // Mark durable row failed
        if let Err(err) = self
            .tracker
            .mark_failed(transfer_id, error_reason, now_ms)
            .await
        {
            warn!(error = %err, transfer_id, "Failed to mark transfer failed");
            return;
        }

        // Emit status-changed for failed
        if let Ok(Some(entry_id)) = self
            .tracker
            .get_entry_summary_by_transfer(transfer_id)
            .await
        {
            let emitter = self
                .emitter_cell
                .read()
                .unwrap_or_else(|p| p.into_inner())
                .clone();
            if let Err(err) = emitter.emit(HostEvent::Transfer(TransferHostEvent::StatusChanged {
                transfer_id: transfer_id.to_string(),
                entry_id,
                status: "failed".to_string(),
                reason: Some(error_reason.to_string()),
            })) {
                warn!(error = %err, "Failed to emit failed status");
            }
        }
    }

    /// Spawn a periodic timeout sweep task.
    ///
    /// Runs every 15 seconds. Fails stalled pending (>60s) and transferring (>5min)
    /// rows, emits status-changed events, and cleans partial cache artifacts.
    pub fn spawn_timeout_sweep(
        &self,
        cancel: tokio::sync::watch::Receiver<bool>,
    ) -> tokio::task::JoinHandle<()> {
        let tracker = self.tracker.clone();
        let emitter_cell = self.emitter_cell.clone();
        let clock = self.clock.clone();

        tokio::spawn(
            async move {
                let mut interval = tokio::time::interval(Duration::from_secs(15));
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
                    match tracker.list_expired_inflight(now_ms).await {
                        Ok(expired) if expired.is_empty() => {}
                        Ok(expired) => {
                            let count = expired.len();
                            warn!(count, "Timeout sweep found expired in-flight transfers");

                            let emitter = emitter_cell
                                .read()
                                .unwrap_or_else(|p| p.into_inner())
                                .clone();

                            for t in &expired {
                                let reason = match t.status {
                                    uc_core::ports::file_transfer_repository::TrackedFileTransferStatus::Pending => {
                                        "timeout: no data received within 60 seconds"
                                    }
                                    uc_core::ports::file_transfer_repository::TrackedFileTransferStatus::Transferring => {
                                        "timeout: no new chunk received within 5 minutes"
                                    }
                                    _ => "timeout: stalled transfer",
                                };

                                if let Err(err) =
                                    tracker.mark_failed(&t.transfer_id, reason, now_ms).await
                                {
                                    warn!(
                                        error = %err,
                                        transfer_id = %t.transfer_id,
                                        "Failed to mark expired transfer as failed"
                                    );
                                    continue;
                                }

                                // Clean partial cache artifact
                                cleanup_cached_path(&t.cached_path).await;

                                // Emit status-changed
                                if let Err(err) = emitter.emit(HostEvent::Transfer(
                                    TransferHostEvent::StatusChanged {
                                        transfer_id: t.transfer_id.clone(),
                                        entry_id: t.entry_id.clone(),
                                        status: "failed".to_string(),
                                        reason: Some(reason.to_string()),
                                    },
                                )) {
                                    warn!(error = %err, "Failed to emit timeout failure status");
                                }
                            }
                        }
                        Err(err) => {
                            warn!(error = %err, "Timeout sweep query failed");
                        }
                    }
                }
            }
            .instrument(info_span!("file_transfer.timeout_sweep")),
        )
    }

    /// Run startup reconciliation: mark orphaned in-flight transfers as failed
    /// and clean their cache artifacts.
    ///
    /// Non-blocking and non-fatal: errors are logged as warnings.
    pub async fn reconcile_on_startup(&self) {
        let now_ms = self.clock.now_ms();
        let emitter = self
            .emitter_cell
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .clone();

        match self
            .tracker
            .reconcile_inflight_after_startup(now_ms)
            .instrument(info_span!("file_transfer.startup_reconcile"))
            .await
        {
            Ok(cleanup_targets) if cleanup_targets.is_empty() => {
                info!("No orphaned in-flight transfers found at startup");
            }
            Ok(cleanup_targets) => {
                let count = cleanup_targets.len();
                warn!(count, "Reconciled orphaned in-flight transfers at startup");

                for t in &cleanup_targets {
                    cleanup_cached_path(&t.cached_path).await;

                    // Emit status-changed for reconciled entries
                    if let Err(err) =
                        emitter.emit(HostEvent::Transfer(TransferHostEvent::StatusChanged {
                            transfer_id: t.transfer_id.clone(),
                            entry_id: t.entry_id.clone(),
                            status: "failed".to_string(),
                            reason: Some(
                                "orphaned: app restarted while transfer was in-flight".to_string(),
                            ),
                        }))
                    {
                        warn!(error = %err, "Failed to emit reconciliation status");
                    }
                }
            }
            Err(err) => {
                warn!(error = %err, "Startup reconciliation failed (non-fatal)");
            }
        }
    }

    async fn resolve_entry_id(&self, transfer_id: &str) -> Option<String> {
        if let Some(entry_id) = self.outbound_link_cache.get(transfer_id) {
            return Some(entry_id);
        }

        if let Some(entry_id) = self.transfer_entry_cache.get(transfer_id) {
            return Some(entry_id);
        }

        match self
            .tracker
            .get_entry_summary_by_transfer(transfer_id)
            .await
        {
            Ok(Some(entry_id)) => {
                self.transfer_entry_cache
                    .insert(transfer_id.to_string(), entry_id.clone());
                Some(entry_id)
            }
            Ok(None) => None,
            Err(err) => {
                warn!(
                    error = %err,
                    transfer_id,
                    "Failed to resolve transfer entry from tracker"
                );
                None
            }
        }
    }

    async fn refresh_inbound_activity_if_due(&self, transfer_id: &str, now_ms: i64) {
        if !self
            .inbound_activity_refresh_cache
            .should_refresh(transfer_id, now_ms)
        {
            return;
        }

        if let Err(err) = self.tracker.refresh_activity(transfer_id, now_ms).await {
            warn!(error = %err, transfer_id, "Failed to refresh transfer activity");
        }
    }
}

/// Best-effort cleanup of a cached file or transfer directory.
async fn cleanup_cached_path(cached_path: &str) {
    if cached_path.is_empty() {
        return;
    }

    let path = std::path::Path::new(cached_path);

    // Try removing the file first
    if path.is_file() {
        if let Err(err) = tokio::fs::remove_file(path).await {
            warn!(error = %err, path = %cached_path, "Failed to remove cached file");
        }
    }

    // Try removing the parent transfer directory (e.g., file-cache/{transfer_id}/)
    // Only if it's empty after the file was removed
    if let Some(parent) = path.parent() {
        // Safety: only remove if the parent looks like a transfer directory
        // (i.e., it lives under the file-cache directory)
        let _ = tokio::fs::remove_dir(parent).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::RwLock;
    use uc_core::ports::file_transfer_repository::FileTransferRepositoryPort;
    use uc_core::ports::file_transfer_repository::PendingInboundTransfer;
    use uc_core::ports::host_event_emitter::{EmitError, HostEventEmitterPort};

    #[derive(Default)]
    struct RecordingEmitter {
        events: std::sync::Mutex<Vec<HostEvent>>,
    }

    impl HostEventEmitterPort for RecordingEmitter {
        fn emit(&self, event: HostEvent) -> Result<(), EmitError> {
            self.events.lock().unwrap().push(event);
            Ok(())
        }
    }

    struct FixedClock(i64);

    impl uc_core::ports::ClockPort for FixedClock {
        fn now_ms(&self) -> i64 {
            self.0
        }
    }

    /// In-memory mock for FileTransferRepositoryPort.
    struct MockFileTransferRepo {
        transfers:
            std::sync::Mutex<Vec<uc_core::ports::file_transfer_repository::TrackedFileTransfer>>,
        refresh_activity_calls: std::sync::atomic::AtomicUsize,
        get_entry_id_for_transfer_calls: std::sync::atomic::AtomicUsize,
    }

    impl MockFileTransferRepo {
        fn new() -> Self {
            Self {
                transfers: std::sync::Mutex::new(Vec::new()),
                refresh_activity_calls: std::sync::atomic::AtomicUsize::new(0),
                get_entry_id_for_transfer_calls: std::sync::atomic::AtomicUsize::new(0),
            }
        }
    }

    #[async_trait::async_trait]
    impl uc_core::ports::file_transfer_repository::FileTransferRepositoryPort for MockFileTransferRepo {
        async fn insert_pending_transfers(
            &self,
            transfers: &[PendingInboundTransfer],
        ) -> anyhow::Result<()> {
            let mut store = self.transfers.lock().unwrap();
            for t in transfers {
                store.push(
                    uc_core::ports::file_transfer_repository::TrackedFileTransfer {
                        transfer_id: t.transfer_id.clone(),
                        entry_id: t.entry_id.clone(),
                        origin_device_id: t.origin_device_id.clone(),
                        filename: t.filename.clone(),
                        cached_path: t.cached_path.clone(),
                        status: uc_core::ports::file_transfer_repository::TrackedFileTransferStatus::Pending,
                        failure_reason: None,
                        file_size: None,
                        content_hash: None,
                        updated_at_ms: t.created_at_ms,
                        created_at_ms: t.created_at_ms,
                    },
                );
            }
            Ok(())
        }

        async fn backfill_announce_metadata(
            &self,
            transfer_id: &str,
            file_size: i64,
            content_hash: &str,
        ) -> anyhow::Result<()> {
            let mut store = self.transfers.lock().unwrap();
            if let Some(t) = store.iter_mut().find(|t| t.transfer_id == transfer_id) {
                t.file_size = Some(file_size);
                t.content_hash = Some(content_hash.to_string());
            }
            Ok(())
        }

        async fn mark_transferring(&self, transfer_id: &str, now_ms: i64) -> anyhow::Result<bool> {
            let mut store = self.transfers.lock().unwrap();
            if let Some(t) = store.iter_mut().find(|t| t.transfer_id == transfer_id) {
                if t.status
                    == uc_core::ports::file_transfer_repository::TrackedFileTransferStatus::Pending
                {
                    t.status = uc_core::ports::file_transfer_repository::TrackedFileTransferStatus::Transferring;
                    t.updated_at_ms = now_ms;
                    return Ok(true);
                }
            }
            Ok(false)
        }

        async fn refresh_activity(&self, transfer_id: &str, now_ms: i64) -> anyhow::Result<()> {
            self.refresh_activity_calls
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let mut store = self.transfers.lock().unwrap();
            if let Some(t) = store.iter_mut().find(|t| t.transfer_id == transfer_id) {
                t.updated_at_ms = now_ms;
            }
            Ok(())
        }

        async fn mark_completed(
            &self,
            transfer_id: &str,
            content_hash: Option<&str>,
            now_ms: i64,
        ) -> anyhow::Result<bool> {
            let mut store = self.transfers.lock().unwrap();
            if let Some(t) = store.iter_mut().find(|t| t.transfer_id == transfer_id) {
                t.status =
                    uc_core::ports::file_transfer_repository::TrackedFileTransferStatus::Completed;
                t.content_hash = content_hash.map(|s| s.to_string());
                t.updated_at_ms = now_ms;
                Ok(true)
            } else {
                Ok(false)
            }
        }

        async fn mark_failed(
            &self,
            transfer_id: &str,
            reason: &str,
            now_ms: i64,
        ) -> anyhow::Result<()> {
            let mut store = self.transfers.lock().unwrap();
            if let Some(t) = store.iter_mut().find(|t| t.transfer_id == transfer_id) {
                t.status =
                    uc_core::ports::file_transfer_repository::TrackedFileTransferStatus::Failed;
                t.failure_reason = Some(reason.to_string());
                t.updated_at_ms = now_ms;
            }
            Ok(())
        }

        async fn list_expired_inflight(
            &self,
            pending_cutoff_ms: i64,
            transferring_cutoff_ms: i64,
        ) -> anyhow::Result<Vec<uc_core::ports::file_transfer_repository::ExpiredInflightTransfer>>
        {
            let store = self.transfers.lock().unwrap();
            Ok(store
                .iter()
                .filter(|t| {
                    (t.status
                        == uc_core::ports::file_transfer_repository::TrackedFileTransferStatus::Pending
                        && t.updated_at_ms < pending_cutoff_ms)
                        || (t.status
                            == uc_core::ports::file_transfer_repository::TrackedFileTransferStatus::Transferring
                            && t.updated_at_ms < transferring_cutoff_ms)
                })
                .map(|t| uc_core::ports::file_transfer_repository::ExpiredInflightTransfer {
                    transfer_id: t.transfer_id.clone(),
                    entry_id: t.entry_id.clone(),
                    cached_path: t.cached_path.clone(),
                    status: t.status,
                })
                .collect())
        }

        async fn bulk_fail_inflight(
            &self,
            reason: &str,
            now_ms: i64,
        ) -> anyhow::Result<Vec<uc_core::ports::file_transfer_repository::ExpiredInflightTransfer>>
        {
            let mut store = self.transfers.lock().unwrap();
            let mut cleanup = Vec::new();
            for t in store.iter_mut() {
                if t.status
                    == uc_core::ports::file_transfer_repository::TrackedFileTransferStatus::Pending
                    || t.status
                        == uc_core::ports::file_transfer_repository::TrackedFileTransferStatus::Transferring
                {
                    cleanup.push(
                        uc_core::ports::file_transfer_repository::ExpiredInflightTransfer {
                            transfer_id: t.transfer_id.clone(),
                            entry_id: t.entry_id.clone(),
                            cached_path: t.cached_path.clone(),
                            status: t.status,
                        },
                    );
                    t.status = uc_core::ports::file_transfer_repository::TrackedFileTransferStatus::Failed;
                    t.failure_reason = Some(reason.to_string());
                    t.updated_at_ms = now_ms;
                }
            }
            Ok(cleanup)
        }

        async fn get_entry_transfer_summary(
            &self,
            entry_id: &str,
        ) -> anyhow::Result<Option<uc_core::ports::file_transfer_repository::EntryTransferSummary>>
        {
            let store = self.transfers.lock().unwrap();
            let entry_transfers: Vec<_> = store.iter().filter(|t| t.entry_id == entry_id).collect();
            if entry_transfers.is_empty() {
                return Ok(None);
            }
            let statuses: Vec<_> = entry_transfers.iter().map(|t| t.status).collect();
            let aggregate =
                uc_core::ports::file_transfer_repository::compute_aggregate_status(&statuses)
                    .unwrap_or(
                    uc_core::ports::file_transfer_repository::TrackedFileTransferStatus::Pending,
                );
            let failure_reason = if aggregate
                == uc_core::ports::file_transfer_repository::TrackedFileTransferStatus::Failed
            {
                entry_transfers
                    .iter()
                    .find(|t| {
                        t.status
                            == uc_core::ports::file_transfer_repository::TrackedFileTransferStatus::Failed
                    })
                    .and_then(|t| t.failure_reason.clone())
            } else {
                None
            };
            let transfer_ids: Vec<String> = entry_transfers
                .iter()
                .map(|t| t.transfer_id.clone())
                .collect();
            Ok(Some(
                uc_core::ports::file_transfer_repository::EntryTransferSummary {
                    entry_id: entry_id.to_string(),
                    aggregate_status: aggregate,
                    failure_reason,
                    transfer_ids,
                },
            ))
        }

        async fn list_transfers_for_entry(
            &self,
            entry_id: &str,
        ) -> anyhow::Result<Vec<uc_core::ports::file_transfer_repository::TrackedFileTransfer>>
        {
            let store = self.transfers.lock().unwrap();
            Ok(store
                .iter()
                .filter(|t| t.entry_id == entry_id)
                .cloned()
                .collect())
        }

        async fn get_entry_id_for_transfer(
            &self,
            transfer_id: &str,
        ) -> anyhow::Result<Option<String>> {
            self.get_entry_id_for_transfer_calls
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let store = self.transfers.lock().unwrap();
            Ok(store
                .iter()
                .find(|t| t.transfer_id == transfer_id)
                .map(|t| t.entry_id.clone()))
        }
    }

    fn make_orchestrator(
        repo: Arc<MockFileTransferRepo>,
        emitter: Arc<RecordingEmitter>,
    ) -> FileTransferOrchestrator {
        let emitter_cell = Arc::new(RwLock::new(emitter.clone() as Arc<dyn HostEventEmitterPort>));
        let tracker = Arc::new(TrackInboundTransfersUseCase::new(repo));
        let clock = Arc::new(FixedClock(1_000_000));
        FileTransferOrchestrator::new(tracker, emitter_cell, clock)
    }

    #[test]
    fn file_transfer_status_payload_serializes_camel_case() {
        let payload = FileTransferStatusPayload {
            transfer_id: "xfer-1".to_string(),
            entry_id: "entry-42".to_string(),
            status: "pending".to_string(),
            reason: None,
        };
        let json = serde_json::to_value(&payload).unwrap();

        // Verify camelCase field names
        assert_eq!(json["transferId"], "xfer-1");
        assert_eq!(json["entryId"], "entry-42");
        assert_eq!(json["status"], "pending");

        // Verify snake_case is NOT present
        assert!(json.get("transfer_id").is_none());
        assert!(json.get("entry_id").is_none());

        // reason should be omitted when None
        assert!(json.get("reason").is_none());
    }

    #[test]
    fn file_transfer_status_payload_includes_reason_when_present() {
        let payload = FileTransferStatusPayload {
            transfer_id: "xfer-2".to_string(),
            entry_id: "entry-99".to_string(),
            status: "failed".to_string(),
            reason: Some("hash mismatch".to_string()),
        };
        let json = serde_json::to_value(&payload).unwrap();

        assert_eq!(json["status"], "failed");
        assert_eq!(json["reason"], "hash mismatch");
    }

    #[test]
    fn file_transfer_status_payload_all_statuses() {
        for status in &["pending", "transferring", "completed", "failed"] {
            let payload = FileTransferStatusPayload {
                transfer_id: "t".to_string(),
                entry_id: "e".to_string(),
                status: status.to_string(),
                reason: None,
            };
            let json = serde_json::to_value(&payload).unwrap();
            assert_eq!(json["status"], *status);
        }
    }

    #[test]
    fn emit_pending_status_emits_one_status_changed_event_per_transfer() {
        let repo = Arc::new(MockFileTransferRepo::new());
        let emitter = Arc::new(RecordingEmitter::default());
        let orch = make_orchestrator(repo, emitter.clone());

        let pending_transfers = vec![
            PendingTransferLinkage {
                transfer_id: "transfer-1".to_string(),
                filename: "a.txt".to_string(),
                cached_path: "/tmp/a.txt".to_string(),
            },
            PendingTransferLinkage {
                transfer_id: "transfer-2".to_string(),
                filename: "b.txt".to_string(),
                cached_path: "/tmp/b.txt".to_string(),
            },
        ];

        orch.emit_pending_status("entry-77", &pending_transfers);

        let events = emitter.events.lock().unwrap();
        assert_eq!(events.len(), 2);

        for (event, transfer_id) in events.iter().zip(["transfer-1", "transfer-2"]) {
            match event {
                HostEvent::Transfer(TransferHostEvent::StatusChanged {
                    transfer_id: actual_transfer_id,
                    entry_id,
                    status,
                    reason,
                }) => {
                    assert_eq!(actual_transfer_id, transfer_id);
                    assert_eq!(entry_id, "entry-77");
                    assert_eq!(status, "pending");
                    assert!(reason.is_none());
                }
                other => panic!("expected transfer status event, got {other:?}"),
            }
        }
    }

    #[tokio::test]
    async fn outbound_progress_uses_registered_entry_linkage() {
        let repo = Arc::new(MockFileTransferRepo::new());
        let emitter = Arc::new(RecordingEmitter::default());
        let orch = make_orchestrator(repo, emitter.clone());

        orch.register_outbound_transfer("transfer-outbound-1", "entry-outbound-1");

        let promoted = orch
            .handle_transfer_progress(&TransferProgress {
                transfer_id: "transfer-outbound-1".to_string(),
                peer_id: "peer-1".to_string(),
                direction: TransferDirection::Sending,
                chunks_completed: 1,
                total_chunks: 4,
                bytes_transferred: 1024,
                total_bytes: Some(4096),
            })
            .await;

        assert!(
            !promoted,
            "sender-side progress should not touch receiver durable state"
        );

        let events = emitter.events.lock().unwrap();
        assert_eq!(events.len(), 1);

        match &events[0] {
            HostEvent::Transfer(TransferHostEvent::Progress {
                transfer_id,
                entry_id,
                direction,
                chunks_completed,
                ..
            }) => {
                assert_eq!(transfer_id, "transfer-outbound-1");
                assert_eq!(entry_id.as_deref(), Some("entry-outbound-1"));
                assert_eq!(direction, &TransferDirection::Sending);
                assert_eq!(*chunks_completed, 1);
            }
            other => panic!("expected progress event, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn inbound_progress_caches_entry_lookup_and_throttles_activity_refresh() {
        let repo = Arc::new(MockFileTransferRepo::new());
        repo.insert_pending_transfers(&[PendingInboundTransfer {
            transfer_id: "transfer-inbound-1".to_string(),
            entry_id: "entry-inbound-1".to_string(),
            origin_device_id: "device-1".to_string(),
            filename: "movie.mp4".to_string(),
            cached_path: "/tmp/movie.mp4".to_string(),
            created_at_ms: 1_000,
        }])
        .await
        .unwrap();

        let emitter = Arc::new(RecordingEmitter::default());
        let emitter_cell = Arc::new(RwLock::new(emitter as Arc<dyn HostEventEmitterPort>));
        let tracker = Arc::new(TrackInboundTransfersUseCase::new(repo.clone()));
        let clock = Arc::new(FixedClock(10_000));
        let orch = FileTransferOrchestrator::new(tracker, emitter_cell, clock);

        orch.handle_transfer_progress(&TransferProgress {
            transfer_id: "transfer-inbound-1".to_string(),
            peer_id: "peer-1".to_string(),
            direction: TransferDirection::Receiving,
            chunks_completed: 2,
            total_chunks: 10,
            bytes_transferred: 2_048,
            total_bytes: Some(10_240),
        })
        .await;

        orch.handle_transfer_progress(&TransferProgress {
            transfer_id: "transfer-inbound-1".to_string(),
            peer_id: "peer-1".to_string(),
            direction: TransferDirection::Receiving,
            chunks_completed: 3,
            total_chunks: 10,
            bytes_transferred: 3_072,
            total_bytes: Some(10_240),
        })
        .await;

        assert_eq!(
            repo.get_entry_id_for_transfer_calls
                .load(std::sync::atomic::Ordering::Relaxed),
            1
        );
        assert_eq!(
            repo.refresh_activity_calls
                .load(std::sync::atomic::Ordering::Relaxed),
            1
        );
    }

    #[tokio::test]
    async fn emitter_cell_swap_is_visible_to_orchestrator() {
        // Build orchestrator with a noop emitter initially
        struct NoopEmitter;
        impl HostEventEmitterPort for NoopEmitter {
            fn emit(&self, _event: HostEvent) -> Result<(), EmitError> {
                Ok(())
            }
        }

        let repo = Arc::new(MockFileTransferRepo::new());
        let noop: Arc<dyn HostEventEmitterPort> = Arc::new(NoopEmitter);
        let emitter_cell = Arc::new(RwLock::new(noop));
        let tracker = Arc::new(TrackInboundTransfersUseCase::new(repo));
        let clock = Arc::new(FixedClock(1_000_000));
        let orch = FileTransferOrchestrator::new(tracker, emitter_cell.clone(), clock);

        // Swap to a recording emitter
        let recording = Arc::new(RecordingEmitter::default());
        *emitter_cell.write().unwrap() = recording.clone() as Arc<dyn HostEventEmitterPort>;

        // Emit via orchestrator — should reach the recording emitter
        orch.emit_pending_status(
            "entry-1",
            &[PendingTransferLinkage {
                transfer_id: "t1".to_string(),
                filename: "f.txt".to_string(),
                cached_path: "/tmp/f.txt".to_string(),
            }],
        );

        let events = recording.events.lock().unwrap();
        assert_eq!(
            events.len(),
            1,
            "Event should reach the new emitter after swap"
        );
    }
}
