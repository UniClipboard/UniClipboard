use std::sync::Arc;

use tracing::{debug, info, warn};
use uc_application::clipboard_capture::CaptureClipboardUseCase;
use uc_application::facade::{
    ClipboardCaptureFacade, ClipboardLiveIndexDeps, ClipboardLiveIndexFacade,
    ClipboardLiveIndexPort, ClipboardLiveIndexer, ClipboardOutboundDeps, ClipboardOutboundFacade,
    ClipboardSyncFacade, InboundAction, InboundClipboardApplyOutcome, InboundClipboardFacade,
    InboundClipboardNoticeInput, InboundNotice,
};
use uc_application::{
    ApplyInboundClipboardUseCase, FileCacheBlobMaterializer, InboundCapture as ApplyInboundCapture,
    InboundWrite as ApplyInboundWrite,
};
use uc_core::TaskRegistry;
use uc_infra::fs::{FsAtomicPublisher, FsHiddenPathMarker, FsInboundFileTarget};

use crate::event_stream::EventSender;
use crate::internal::deps::WiredDependencies;
use crate::internal::sync_engine::SyncEngineAssembly;
use crate::{EngineEvent, InboundNoticeActionSummary, InboundNoticeEvent};

pub(crate) struct ClipboardRuntime {
    pub capture: Arc<ClipboardCaptureFacade>,
    pub live_index: Arc<ClipboardLiveIndexFacade>,
    pub outbound: Arc<ClipboardOutboundFacade>,
    pub inbound: Arc<InboundClipboardFacade>,
    pub apply_inbound: Arc<ApplyInboundClipboardUseCase>,
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
    let blob_materializer = Arc::new(
        FileCacheBlobMaterializer::new(
            sync_engine.blob.clone(),
            wired.shared.file_cache_dir.clone(),
            FsAtomicPublisher::new(),
        )
        .with_directory_receive_attempt_ports(
            deps.storage.directory_receive.get_attempt.clone(),
            deps.storage.directory_receive.claim_commit.clone(),
            deps.storage.directory_receive.record_publish.clone(),
            deps.system.clock.clone(),
        )
        .with_receive_artifact_log(deps.storage.directory_receive.record_artifacts.clone())
        .with_target_reserver(FsInboundFileTarget::new(deps.settings.clone()))
        .with_save_dir_resolver(FsInboundFileTarget::new(deps.settings.clone()))
        .with_hidden_marker(FsHiddenPathMarker::new()),
    );
    let apply_inbound = Arc::new(
        ApplyInboundClipboardUseCase::new(
            deps.clipboard.entry_ports.find_by_snapshot_hash.clone(),
            Arc::clone(&capture) as Arc<dyn ApplyInboundCapture>,
            Arc::clone(&wired.shared.clipboard_write_coordinator) as Arc<dyn ApplyInboundWrite>,
        )
        .with_blob_materializer(blob_materializer)
        .with_receive_attempt_ports(
            deps.storage.directory_receive.get_attempt.clone(),
            deps.storage.directory_receive.begin_receive.clone(),
            deps.storage.directory_receive.claim_commit.clone(),
            deps.storage.directory_receive.request_cancel.clone(),
            deps.storage.directory_receive.begin_failure.clone(),
            deps.storage.directory_receive.commit_inbound.clone(),
            deps.system.clock.clone(),
        )
        .with_receive_artifact_cleanup(Arc::new(uc_infra::fs::FsReceiveArtifactCleaner))
        .with_provisional_receive(deps.storage.file_transfer.finalize_provisional.clone())
        .with_receive_readiness(wired.shared.receive_readiness.clone())
        .with_host_event_emitter(wired.shared.host_event_bus.clone())
        .with_active_register(
            deps.clipboard.active_register.clone(),
            deps.clipboard.mobile_consumability.clone(),
        )
        .with_search_live_index(Arc::clone(&search_live_indexer))
        .with_check_entry_availability(deps.clipboard.entry_ports.availability.clone())
        .with_entry_identity_coordinator(deps.clipboard.entry_identity_coordinator.clone())
        .with_resurface(
            uc_application::facade::ClipboardSnapshotDeps {
                entry_repo: deps.clipboard.entry_ports.get.clone(),
                selection_repo: deps.clipboard.selection_repo.clone(),
                representation_repo: deps.clipboard.representation_ports.get.clone(),
                rep_processing_repo: deps
                    .clipboard
                    .representation_ports
                    .update_processing_result
                    .clone(),
                payload_resolver: deps.clipboard.payload_resolver.clone(),
                blob_store: deps.storage.blob_store.clone(),
            },
            deps.clipboard.entry_ports.touch.clone(),
        ),
    );

    ClipboardRuntime {
        capture: Arc::new(ClipboardCaptureFacade::new(
            capture,
            deps.clipboard.clipboard.clone(),
        )),
        live_index: Arc::new(ClipboardLiveIndexFacade::new(search_live_indexer)),
        outbound,
        inbound: Arc::new(InboundClipboardFacade::new(apply_inbound.clone())),
        apply_inbound,
    }
}

pub(crate) async fn spawn_clipboard_runtime_tasks(
    runtime: &ClipboardRuntime,
    clipboard_sync: Arc<ClipboardSyncFacade>,
    tasks: &TaskRegistry,
    events: EventSender,
) {
    let inbound = Arc::clone(&runtime.inbound);
    tasks
        .spawn("inbound_clipboard_sync", move |cancel| async move {
            let mut notices = clipboard_sync.subscribe_inbound_notices();
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => return,
                    notice = notices.recv() => match notice {
                        Ok(notice) => {
                            events.send(engine_event_for_inbound_notice(&notice));
                            let input = InboundClipboardNoticeInput {
                                from_device: notice.from_device.as_str().to_string(),
                                snapshot_hash: notice.snapshot_hash,
                                plaintext: notice.plaintext,
                                flow_id: notice.flow_id,
                            };
                            match inbound.apply_notice(input).await {
                                Ok(InboundClipboardApplyOutcome::Applied { entry_id }) => {
                                    info!(entry_id = %entry_id, "inbound clipboard applied");
                                }
                                Ok(InboundClipboardApplyOutcome::Resurfaced {
                                    existing_entry_id,
                                    os_write_succeeded,
                                    ..
                                }) => {
                                    debug!(entry_id = %existing_entry_id, os_write_succeeded, "inbound clipboard resurfaced");
                                }
                                Ok(InboundClipboardApplyOutcome::DuplicateSkipped { .. }) => {
                                    debug!("inbound clipboard duplicate skipped");
                                }
                                Ok(InboundClipboardApplyOutcome::DecodeFailed { reason }) => {
                                    debug!(reason, "inbound clipboard decode failed");
                                }
                                Err(error) => warn!(error = %error, "inbound clipboard apply failed"),
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(missed)) => {
                            warn!(missed, "inbound clipboard notices lagged");
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
                    }
                }
            }
        })
        .await;
}

fn engine_event_for_inbound_notice(notice: &InboundNotice) -> EngineEvent {
    EngineEvent::InboundNotice(InboundNoticeEvent {
        from_device: notice.from_device.as_str().to_string(),
        snapshot_hash: notice.snapshot_hash.clone(),
        plaintext: notice.plaintext.to_vec(),
        action: match notice.action {
            InboundAction::NewEntry => InboundNoticeActionSummary::NewEntry,
            InboundAction::DuplicateIgnored => InboundNoticeActionSummary::DuplicateIgnored,
        },
        at_ms: notice.at_ms,
    })
}

#[cfg(test)]
mod tests {
    use uc_application::facade::{InboundAction, InboundNotice};
    use uc_core::ids::DeviceId;

    use crate::{EngineEvent, InboundNoticeActionSummary, InboundNoticeEvent};

    use super::engine_event_for_inbound_notice;

    #[test]
    fn inbound_notice_event_preserves_the_existing_daemon_watch_contract() {
        let notice = InboundNotice {
            from_device: DeviceId::new("peer-1"),
            snapshot_hash: "hash-1".into(),
            plaintext: vec![1, 2, 3].into(),
            flow_id: None,
            action: InboundAction::DuplicateIgnored,
            at_ms: 42,
        };

        assert_eq!(
            engine_event_for_inbound_notice(&notice),
            EngineEvent::InboundNotice(InboundNoticeEvent {
                from_device: "peer-1".into(),
                snapshot_hash: "hash-1".into(),
                plaintext: vec![1, 2, 3],
                action: InboundNoticeActionSummary::DuplicateIgnored,
                at_ms: 42,
            })
        );
    }
}
