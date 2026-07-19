use std::sync::Arc;

use uc_application::clipboard_capture::CaptureClipboardUseCase;
use uc_application::facade::{
    ClipboardCaptureFacade, ClipboardLiveIndexDeps, ClipboardLiveIndexFacade,
    ClipboardLiveIndexPort, ClipboardLiveIndexer, ClipboardOutboundDeps, ClipboardOutboundFacade,
};

use crate::internal::deps::WiredDependencies;
use crate::internal::sync_engine::SyncEngineAssembly;

pub(crate) struct ClipboardRuntime {
    pub capture: Arc<ClipboardCaptureFacade>,
    pub live_index: Arc<ClipboardLiveIndexFacade>,
    pub outbound: Arc<ClipboardOutboundFacade>,
}

pub(crate) fn build_clipboard_runtime(
    wired: &WiredDependencies,
    sync_engine: &SyncEngineAssembly,
) -> ClipboardRuntime {
    let deps = &wired.deps;
    let capture = Arc::new(
        CaptureClipboardUseCase::new(
            deps.clipboard.entry_ports.save.clone(),
            deps.clipboard.entry_ports.touch.clone(),
            deps.clipboard.entry_ports.find_by_snapshot_hash.clone(),
            deps.clipboard.clipboard_event_repo.clone(),
            deps.clipboard.representation_policy.clone(),
            deps.clipboard.representation_normalizer.clone(),
            deps.device.device_identity.clone(),
            deps.clipboard.representation_cache.clone(),
            deps.clipboard.spool_queue.clone(),
            deps.storage.blob_content_ingest.clone(),
            deps.storage.entry_file_set_repo.clone(),
            deps.settings.clone(),
            deps.clipboard.entry_ports.replace_content.clone(),
            deps.analytics.clone(),
        )
        .with_inbound_receive_commit(deps.storage.directory_receive.commit_inbound.clone())
        .with_entry_identity_coordinator(deps.clipboard.entry_identity_coordinator.clone()),
    );
    let search_live_indexer: Arc<dyn ClipboardLiveIndexPort> =
        Arc::new(ClipboardLiveIndexer::new(ClipboardLiveIndexDeps {
            clipboard_entry_repo: deps.clipboard.entry_ports.get.clone(),
            representation_policy: deps.clipboard.representation_policy.clone(),
            search_key_derivation: deps.search.search_key_derivation.clone(),
            search_pipeline: deps.search.search_pipeline.clone(),
            search_index: deps.search.search_index.clone(),
            event_repo: wired.shared.clipboard_event_reader_repo.clone(),
            entry_file_set_repo: deps.storage.entry_file_set_repo.clone(),
        }));
    let outbound = Arc::new(ClipboardOutboundFacade::new(ClipboardOutboundDeps {
        settings: deps.settings.clone(),
        clipboard_sync: sync_engine.clipboard_sync.clone(),
        blob_transfer: sync_engine.blob.clone(),
        entry_repo: deps.clipboard.entry_ports.get.clone(),
        event_repo: wired.shared.clipboard_event_reader_repo.clone(),
        selection_repo: deps.clipboard.selection_repo.clone(),
        representation_repo: deps.clipboard.representation_ports.get.clone(),
        rep_processing_repo: deps
            .clipboard
            .representation_ports
            .update_processing_result
            .clone(),
        payload_resolver: deps.clipboard.payload_resolver.clone(),
        blob_store: deps.storage.blob_store.clone(),
        entry_delivery_repo: wired.shared.entry_delivery_repo.clone(),
        trusted_peer_repo: wired.shared.trusted_peer_repo.clone(),
        device_identity: deps.device.device_identity.clone(),
        entry_file_set_repo: deps.storage.entry_file_set_repo.clone(),
    }));

    ClipboardRuntime {
        capture: Arc::new(ClipboardCaptureFacade::new(
            capture,
            deps.clipboard.clipboard.clone(),
        )),
        live_index: Arc::new(ClipboardLiveIndexFacade::new(search_live_indexer)),
        outbound,
    }
}
