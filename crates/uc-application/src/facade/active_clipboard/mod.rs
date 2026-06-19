//! `ActiveClipboardFacade` — application entry point for the cross-device
//! active-clipboard register convergence (issue #1017).
//!
//! Owns the inbound state use case and exposes a single action: spawn the
//! background loop that subscribes to inbound 0xC3 observations and drives the
//! register toward convergence (write OS → advance register → re-broadcast).
//! The outbound *origination* paths (restore broadcast, peer-online resync,
//! mobile fan-out) are separate edit-sites in later PRs; this facade is the
//! inbound-convergence seam.

use std::sync::Arc;

use uc_core::ports::clipboard::{
    ActiveClipboardDispatchPort, ActiveClipboardReceiverPort, AdvanceActiveClipboardPort,
    ClipboardPayloadResolverPort, ClipboardSelectionRepositoryPort, FindEntryIdBySnapshotHashPort,
    GetClipboardEntryPort, GetRepresentationPort, LoadActiveClipboardPort,
    UpdateRepresentationProcessingResultPort,
};
use uc_core::ports::space::IsSpaceUnlockedPort;
use uc_core::ports::{ClockPort, PeerAddressRepositoryPort};
use uc_core::{blob::ports::BlobReaderPort, MemberRepositoryPort};

use crate::clipboard_write::ClipboardWriteCoordinator;
use crate::usecases::clipboard_sync::apply_inbound_active_state::{
    ActiveClipboardInboundHandle, ApplyInboundActiveClipboardStateUseCase,
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
    pub entry_lookup: Arc<dyn FindEntryIdBySnapshotHashPort>,
    pub coordinator: Arc<ClipboardWriteCoordinator>,
    pub clock: Arc<dyn ClockPort>,
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

/// Thin facade over the inbound active-clipboard state use case.
pub struct ActiveClipboardFacade {
    inbound_uc: Arc<ApplyInboundActiveClipboardStateUseCase>,
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
            deps.load_register,
            deps.advance_register,
            deps.member_repo,
            deps.entry_lookup,
            reconstructor,
            deps.coordinator,
            deps.dispatch,
            deps.peer_addr_repo,
            deps.clock,
        ));
        Self { inbound_uc }
    }

    /// Spawn the inbound convergence loop. Caller owns the returned handle;
    /// dropping it (or `abort()`) terminates the loop. The loop also exits on
    /// its own when the receiver adapter shuts down.
    pub fn spawn_inbound_loop(&self) -> ActiveClipboardInboundHandle {
        Arc::clone(&self.inbound_uc).spawn_run()
    }
}
