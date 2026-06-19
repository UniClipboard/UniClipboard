//! `ActiveClipboardFacade` — application entry point for the cross-device
//! active-clipboard register convergence (issue #1017).
//!
//! Owns the inbound state use case and exposes outbound origination actions:
//! spawn the background loop that subscribes to inbound 0xC3 observations and
//! drives the register toward convergence (write OS → advance register →
//! re-broadcast), plus the outbound origination workers (restore broadcast,
//! peer-online resync). The mobile fan-out path is a separate edit-site in a
//! later PR.

use std::sync::Arc;

use tokio::sync::mpsc::UnboundedReceiver;

use uc_core::ports::clipboard::{
    ActiveClipboardDispatchPort, ActiveClipboardReceiverPort, AdvanceActiveClipboardPort,
    ClipboardPayloadResolverPort, ClipboardSelectionRepositoryPort, FindEntryIdBySnapshotHashPort,
    GetClipboardEntryPort, GetRepresentationPort, LoadActiveClipboardPort,
    UpdateRepresentationProcessingResultPort,
};
use uc_core::ports::space::IsSpaceUnlockedPort;
use uc_core::ports::{ClockPort, PeerAddressRepositoryPort, PresencePort, SettingsPort};
use uc_core::{blob::ports::BlobReaderPort, MemberRepositoryPort};

use crate::clipboard_write::{ClipboardWriteCoordinator, RestoreBroadcastRequest};
use crate::usecases::clipboard_sync::apply_inbound_active_state::{
    ActiveClipboardInboundHandle, ApplyInboundActiveClipboardStateUseCase,
};
use crate::usecases::clipboard_sync::peer_online_resync_worker::{
    PeerOnlineResyncHandle, PeerOnlineResyncWorker,
};
use crate::usecases::clipboard_sync::restore_broadcast_worker::{
    RestoreBroadcastHandle, RestoreBroadcastWorker,
};
use crate::usecases::clipboard_sync::snapshot_from_entry::SnapshotReconstructor;

/// Wiring dependencies for [`ActiveClipboardFacade`]. Assembled by bootstrap.
pub struct ActiveClipboardDeps {
    pub receiver: Arc<dyn ActiveClipboardReceiverPort>,
    pub dispatch: Arc<dyn ActiveClipboardDispatchPort>,
    pub is_unlocked: Arc<dyn IsSpaceUnlockedPort>,
    pub load_register: Arc<dyn LoadActiveClipboardPort>,
    pub advance_register: Arc<dyn AdvanceActiveClipboardPort>,
    pub member_repo: Arc<dyn MemberRepositoryPort>,
    pub peer_addr_repo: Arc<dyn PeerAddressRepositoryPort>,
    /// Presence stream for the peer-online resync worker: an "online"
    /// transition triggers a resend of the current register to that peer.
    pub presence: Arc<dyn PresencePort>,
    pub entry_lookup: Arc<dyn FindEntryIdBySnapshotHashPort>,
    pub coordinator: Arc<ClipboardWriteCoordinator>,
    pub clock: Arc<dyn ClockPort>,
    /// Settings reader for the restore-broadcast feature gate
    /// (`sync.sync_on_restore`).
    pub settings: Arc<dyn SettingsPort>,
    // Snapshot reconstruction ports (shared with restore / resend).
    pub entry_repo: Arc<dyn GetClipboardEntryPort>,
    pub selection_repo: Arc<dyn ClipboardSelectionRepositoryPort>,
    pub representation_repo: Arc<dyn GetRepresentationPort>,
    pub rep_processing_repo: Arc<dyn UpdateRepresentationProcessingResultPort>,
    pub payload_resolver: Arc<dyn ClipboardPayloadResolverPort>,
    pub blob_store: Arc<dyn BlobReaderPort>,
}

/// Re-exported handle so bootstrap can hold the spawned loop's lifetime.
pub use crate::usecases::clipboard_sync::apply_inbound_active_state::ActiveClipboardInboundHandle as ActiveClipboardHandle;

/// Thin facade over the inbound active-clipboard state use case plus the
/// outbound origination workers — restore broadcast and peer-online resync
/// (issue #1017).
pub struct ActiveClipboardFacade {
    inbound_uc: Arc<ApplyInboundActiveClipboardStateUseCase>,
    // Retained for the restore-broadcast worker (outbound origination). Same
    // dispatch / roster / gate as the inbound re-broadcast path.
    dispatch: Arc<dyn ActiveClipboardDispatchPort>,
    peer_addr_repo: Arc<dyn PeerAddressRepositoryPort>,
    member_repo: Arc<dyn MemberRepositoryPort>,
    settings: Arc<dyn SettingsPort>,
    // Retained for the peer-online resync worker (outbound origination): on a
    // peer-online presence transition it loads the register, reconstructs the
    // activation's content category for the outbound gate, and resends the
    // current state to that peer.
    presence: Arc<dyn PresencePort>,
    load_register: Arc<dyn LoadActiveClipboardPort>,
    reconstructor: SnapshotReconstructor,
}

impl ActiveClipboardFacade {
    pub fn new(deps: ActiveClipboardDeps) -> Self {
        let reconstructor = SnapshotReconstructor::new(
            deps.entry_repo,
            deps.selection_repo,
            deps.representation_repo,
            deps.rep_processing_repo,
            deps.payload_resolver,
            deps.blob_store,
        );
        let inbound_uc = Arc::new(ApplyInboundActiveClipboardStateUseCase::new(
            deps.receiver,
            deps.is_unlocked,
            Arc::clone(&deps.load_register),
            deps.advance_register,
            Arc::clone(&deps.member_repo),
            deps.entry_lookup,
            reconstructor.clone(),
            deps.coordinator,
            Arc::clone(&deps.dispatch),
            Arc::clone(&deps.peer_addr_repo),
            deps.clock,
        ));
        Self {
            inbound_uc,
            dispatch: deps.dispatch,
            peer_addr_repo: deps.peer_addr_repo,
            member_repo: deps.member_repo,
            settings: deps.settings,
            presence: deps.presence,
            load_register: deps.load_register,
            reconstructor,
        }
    }

    /// Spawn the inbound convergence loop. Caller owns the returned handle;
    /// dropping it (or `abort()`) terminates the loop. The loop also exits on
    /// its own when the receiver adapter shuts down.
    pub fn spawn_inbound_loop(&self) -> ActiveClipboardInboundHandle {
        Arc::clone(&self.inbound_uc).spawn_run()
    }

    /// Spawn the outbound restore-broadcast worker. `rx` is the receiving end
    /// of the restore-broadcast channel whose sender side
    /// ([`RestoreBroadcastTrigger`](crate::clipboard_write::RestoreBroadcastTrigger))
    /// the restore use cases hold. The worker debounces rapid restores, gates
    /// on `sync_on_restore` plus the per-device send preferences, and fans the
    /// activation out to allowed peers through the shared fan-out. Caller owns
    /// the returned handle; dropping it terminates the worker (which also exits
    /// on its own once every trigger sender is dropped).
    pub fn spawn_restore_broadcast(
        &self,
        rx: UnboundedReceiver<RestoreBroadcastRequest>,
    ) -> RestoreBroadcastHandle {
        RestoreBroadcastWorker::new(
            rx,
            Arc::clone(&self.settings),
            Arc::clone(&self.dispatch),
            Arc::clone(&self.peer_addr_repo),
            Arc::clone(&self.member_repo),
        )
        .spawn()
    }

    /// Spawn the peer-online resync worker (issue #1017 PR5, D10). The worker
    /// subscribes to presence transitions; when a peer comes online it
    /// debounces a burst (D7, ~1.5s), loads the current register, reconstructs
    /// the activation's content category for the outbound gate, and resends
    /// the current state to each newly-online peer (full outbound gate:
    /// `send_enabled` ∧ `send_content_types`). The resync is symmetric — the
    /// peer runs the same worker and resends to us; LWW converges both ends
    /// with no ack. Caller owns the returned handle; dropping it terminates
    /// the worker (which also exits when the presence subscription closes at
    /// router shutdown).
    pub fn spawn_peer_online_resync(&self) -> PeerOnlineResyncHandle {
        PeerOnlineResyncWorker::new(
            Arc::clone(&self.presence),
            Arc::clone(&self.load_register),
            self.reconstructor.clone(),
            Arc::clone(&self.dispatch),
            Arc::clone(&self.member_repo),
        )
        .spawn()
    }
}

/// Re-exported handle so bootstrap can hold the restore-broadcast worker's
/// lifetime alongside the inbound loop handle.
pub use crate::usecases::clipboard_sync::restore_broadcast_worker::RestoreBroadcastHandle as ActiveClipboardRestoreBroadcastHandle;

/// Re-exported handle so bootstrap can hold the peer-online resync worker's
/// lifetime alongside the other active-clipboard worker handles.
pub use crate::usecases::clipboard_sync::peer_online_resync_worker::PeerOnlineResyncHandle as ActiveClipboardPeerOnlineResyncHandle;
