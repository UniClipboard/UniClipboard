use std::sync::Arc;
use std::time::Duration;
use std::{fs::OpenOptions, io::Write, path::PathBuf};

use async_trait::async_trait;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use tracing::{error, warn};
use uc_application::clipboard_write::LocalActiveRegisterAdvancer;
use uc_application::facade::{
    AppFacade, ClipboardHistoryError, ClipboardHostEvent, ClipboardOriginKind, HostEvent,
    HostEventBus, InMemoryLifecycleStatus, ResourceFacadeError, SearchCoordinator,
    SearchCoordinatorDeps, SearchFacadeError, SearchPageView, SearchQueryInput,
    MAX_INLINE_OUTBOUND_REPRESENTATION_BYTES,
};
use uc_application::facade::{
    ClipboardLiveIndexInput, ClipboardLiveIndexOutcome, ClipboardOutboundInput,
    ClipboardOutboundOutcome,
};
use uc_core::ids::{DeviceId, FormatId, RepresentationId};
use uc_core::ports::{SelfWriteLedgerPort, SystemClipboardPort};
use uc_core::TaskRegistry;
use uc_core::{
    ClipboardChangeOrigin, FileDisplayMetadata, FileDisplayMetadataEntry, MimeType,
    ObservedClipboardRepresentation, SystemClipboardSnapshot, FILE_DISPLAY_METADATA_FORMAT,
    FILE_DISPLAY_METADATA_MIME,
};

use crate::engine::EngineRuntime;
use crate::event_stream::EventSender;
use crate::internal::blob_tasks::{spawn_blob_processing_tasks, BlobProcessingPorts};
use crate::internal::cancel_invitation::execute_cancel_invitation;
use crate::internal::capture::execute_capture_current_clipboard;
use crate::internal::clipboard_runtime::{
    build_clipboard_runtime, spawn_clipboard_runtime_tasks, ClipboardRuntime,
};
use crate::internal::create_space::execute_create_space;
use crate::internal::delivery::execute_query_entry_delivery;
use crate::internal::deps::WiredDependencies;
use crate::internal::device::execute_query_local_device;
use crate::internal::encryption::{
    execute_lock_encryption, execute_query_encryption_state, execute_verify_secure_storage_access,
};
use crate::internal::facade::{
    build_app_facade_from_deps, AppFacadeAssemblyOptions, ClipboardRestoreAssembly,
};
use crate::internal::factory_reset::execute_factory_reset_space;
use crate::internal::file_transfer::FileTransferLifecycle;
use crate::internal::history::{
    execute_clear_history, execute_delete_history_entry, execute_get_history_entry,
    execute_get_history_entry_resource, execute_list_history_entries, execute_query_history_stats,
    execute_set_history_entry_favorite,
};
use crate::internal::history_maintenance::spawn_history_maintenance_task;
use crate::internal::host_adapters::{
    wire_host_capabilities_with_emitter, EngineHostEventEmitter, HostWiring,
};
use crate::internal::invitation::execute_issue_invitation;
use crate::internal::join_space::{execute_join_space, JoinSpaceMode};
use crate::internal::lifecycle::build_daemon_lifecycle;
use crate::internal::member::{
    execute_list_devices, execute_query_member_sync_preferences, execute_remove_member,
    execute_update_member_sync_preferences,
};
use crate::internal::migration_progress::execute_query_migration_progress;
use crate::internal::peer_connections::{
    execute_query_peer_connections, execute_refresh_peer_connections,
};
use crate::internal::peer_keepalive::spawn_peer_keepalive_task;
use crate::internal::receive::{
    execute_cancel_entry_receive, execute_cancel_inbound_transfer,
    execute_list_entry_receive_progress, execute_query_entry_receive_progress,
};
use crate::internal::resend::execute_resend_entry;
use crate::internal::reset_space::execute_reset_space;
use crate::internal::resource::{
    execute_read_blob, execute_read_entry_file, execute_read_thumbnail,
};
use crate::internal::restore::execute_restore_clipboard;
use crate::internal::search::{
    execute_query_search_status, execute_query_search_tags, execute_rebuild_search_index,
    execute_search_entries,
};
use crate::internal::session_recovery::execute_recover_session;
use crate::internal::settings::{
    execute_probe_relay, execute_query_settings, execute_update_settings,
};
use crate::internal::setup_state::execute_query_setup_state;
use crate::internal::storage::{execute_clear_storage_cache, execute_query_storage_stats};
use crate::internal::sync_engine::SyncEngineAssembly;
use crate::internal::unlock::execute_unlock_space;
use crate::internal::upgrade::{execute_acknowledge_upgrade, execute_query_upgrade_status};
use crate::{
    EngineConfig, EngineError, EngineErrorCategory, EntrySummary, HostCapabilities,
    HostClipboardChange, HostClipboardChangeStream, HostFileAccess, Operation, OperationResult,
    QueryHistoryInput,
};

const START_FAILED_CODE: u32 = 1101;
const OPERATION_UNAVAILABLE_CODE: u32 = 1103;
const QUERY_HISTORY_INVALID_INPUT_CODE: u32 = 1241;
const QUERY_HISTORY_UNAUTHORIZED_CODE: u32 = 1242;
const QUERY_HISTORY_UNAVAILABLE_CODE: u32 = 1243;
const QUERY_HISTORY_FAILED_CODE: u32 = 1244;
const HISTORY_CURSOR_PREFIX: &str = "uc-history-v1:";
const MAX_HISTORY_PAGE_SIZE: u32 = 200;
const SEND_INVALID_INPUT_CODE: u32 = 1251;
const SEND_FAILED_CODE: u32 = 1252;
const SEND_SKIPPED_CODE: u32 = 1253;
const EXPORT_NOT_FOUND_CODE: u32 = 1271;
const EXPORT_INVALID_TARGET_CODE: u32 = 1272;
const EXPORT_UNAUTHORIZED_CODE: u32 = 1273;
const EXPORT_UNAVAILABLE_CODE: u32 = 1274;
const EXPORT_FAILED_CODE: u32 = 1275;
const EXPORT_CHUNK_SIZE: usize = 64 * 1024;

pub(crate) struct ProductionRuntime {
    app_version: String,
    wired: WiredDependencies,
    paths: uc_application::facade::AppPaths,
    session: Arc<Mutex<Option<ProductionSession>>>,
    task_registry: Arc<TaskRegistry>,
    file_transfer_lifecycle: Arc<FileTransferLifecycle>,
    _temporary_dir: std::path::PathBuf,
    clipboard_import_root: std::path::PathBuf,
    files: Arc<dyn HostFileAccess>,
}

struct ProductionSession {
    facade: Arc<AppFacade>,
    clipboard: ClipboardRuntime,
    sync_engine: SyncEngineAssembly,
    tasks: Arc<TaskRegistry>,
}

struct ImportedHostFile {
    path: PathBuf,
    storage_name: String,
    display_name: String,
}

struct HostClipboardChangeRuntime {
    session: Arc<Mutex<Option<ProductionSession>>>,
    system_clipboard: Arc<dyn SystemClipboardPort>,
    change_origin: Arc<dyn SelfWriteLedgerPort>,
    active_register: LocalActiveRegisterAdvancer,
    host_events: Arc<HostEventBus>,
}

async fn spawn_host_clipboard_change_task(
    mut changes: Box<dyn HostClipboardChangeStream>,
    runtime: HostClipboardChangeRuntime,
    tasks: Arc<TaskRegistry>,
) {
    tasks
        .spawn("host_clipboard_changes", move |cancel| async move {
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => {
                        if let Err(error) = changes.shutdown().await {
                            warn!(error = %error, "host clipboard change stream shutdown failed");
                        }
                        return;
                    }
                    change = changes.next() => match change {
                        Ok(HostClipboardChange::Changed) => {
                            if let Err(error) = runtime.process_change().await {
                                warn!(error = %error, "host clipboard change processing failed");
                            }
                        }
                        Ok(HostClipboardChange::Closed) => return,
                        Err(error) => {
                            warn!(error = %error, "host clipboard change stream failed");
                            return;
                        }
                    }
                }
            }
        })
        .await;
}

impl HostClipboardChangeRuntime {
    async fn process_change(&self) -> anyhow::Result<()> {
        let session = self.session.lock().await;
        let Some(session) = session.as_ref() else {
            return Ok(());
        };
        let encryption = session
            .facade
            .encryption
            .state()
            .await
            .map_err(|_| anyhow::anyhow!("host clipboard encryption state unavailable"))?;
        if !encryption.session_ready {
            return Ok(());
        }

        let snapshot = self
            .system_clipboard
            .read_snapshot()
            .map_err(|_| anyhow::anyhow!("host clipboard snapshot read failed"))?;
        if snapshot.is_empty() {
            return Ok(());
        }
        let origin_guard_key = snapshot.origin_guard_key();
        let origin = self
            .change_origin
            .attribute_observed_change(&origin_guard_key)
            .await;
        if origin.is_remote_push() {
            return Ok(());
        }
        if origin == ClipboardChangeOrigin::Resend {
            error!("host clipboard watcher observed an invalid resend origin");
            return Ok(());
        }

        let outbound_snapshot = Arc::new(snapshot.clone());
        let Some(captured) = session
            .clipboard
            .capture
            .capture(snapshot, origin, None)
            .await
            .map_err(|_| anyhow::anyhow!("host clipboard capture failed"))?
        else {
            return Ok(());
        };
        let entry_id = uc_core::ids::EntryId::from(captured.entry_id.as_str());
        self.active_register
            .advance_local(captured.snapshot_hash, entry_id)
            .await;
        self.host_events
            .emit_or_warn(HostEvent::Clipboard(ClipboardHostEvent::NewContent {
                entry_id: captured.entry_id.clone(),
                attempt_id: None,
                preview: "New clipboard content".to_string(),
                origin: ClipboardOriginKind::Local,
            }));

        if !captured.deduplicated {
            match session
                .clipboard
                .live_index
                .index_capture(ClipboardLiveIndexInput {
                    entry_id: captured.entry_id.clone(),
                    snapshot: Arc::clone(&outbound_snapshot),
                })
                .await
            {
                Ok(ClipboardLiveIndexOutcome::Indexed) => {}
                Ok(ClipboardLiveIndexOutcome::Skipped { reason }) => {
                    tracing::debug!(reason, "host clipboard live index skipped");
                }
                Err(error) => warn!(error = %error, "host clipboard live index failed"),
            }
        }

        let dispatch_snapshot =
            Arc::try_unwrap(outbound_snapshot).unwrap_or_else(|shared| (*shared).clone());
        let outbound = Arc::clone(&session.clipboard.outbound);
        session
            .tasks
            .spawn("host_clipboard_outbound", move |cancel| async move {
                let outcome = tokio::select! {
                    _ = cancel.cancelled() => return,
                    outcome = outbound.dispatch_capture(ClipboardOutboundInput {
                        entry_id: captured.entry_id,
                        snapshot: dispatch_snapshot,
                        origin,
                    }) => outcome,
                };
                match outcome {
                    Ok(ClipboardOutboundOutcome::Dispatched {
                        accepted,
                        duplicate,
                        offline,
                        errored,
                        pending,
                        ..
                    }) => tracing::info!(
                        accepted,
                        duplicate,
                        offline,
                        errored,
                        pending,
                        "host clipboard outbound sync completed"
                    ),
                    Ok(ClipboardOutboundOutcome::Skipped { reason }) => {
                        tracing::debug!(reason, "host clipboard outbound sync skipped");
                    }
                    Err(error) => warn!(error = %error, "host clipboard outbound sync failed"),
                }
            })
            .await;
        Ok(())
    }
}

impl ProductionRuntime {
    pub(crate) async fn start(
        config: EngineConfig,
        host: HostCapabilities,
        events: EventSender,
    ) -> Result<Self, EngineError> {
        let app_version = config.app_version().to_string();
        let emitter = Arc::new(EngineHostEventEmitter::new(events));
        let HostWiring {
            wired,
            background,
            paths,
            temporary_dir,
            clipboard_import_root,
            files,
            clipboard_changes,
        } = wire_host_capabilities_with_emitter(&config, host, emitter)
            .map_err(|error| startup_error("dependency wiring", error))?;

        let file_transfer_lifecycle = Arc::clone(&background.file_transfer_lifecycle);
        let session = Self::build_session(&wired, &paths, &file_transfer_lifecycle).await?;
        let task_registry = Arc::new(TaskRegistry::new());
        let blob_ports = BlobProcessingPorts::from_app_deps(&wired.deps);
        spawn_blob_processing_tasks(background, blob_ports, &task_registry).await;
        let session = Arc::new(Mutex::new(Some(session)));
        if let Some(changes) = clipboard_changes {
            let change_runtime = HostClipboardChangeRuntime {
                session: Arc::clone(&session),
                system_clipboard: Arc::clone(&wired.deps.clipboard.system_clipboard),
                change_origin: Arc::clone(&wired.deps.clipboard.clipboard_change_origin),
                active_register: LocalActiveRegisterAdvancer::new(
                    Arc::clone(&wired.deps.clipboard.active_register),
                    Arc::clone(&wired.deps.device.device_identity),
                    Arc::clone(&wired.deps.system.clock),
                    wired.deps.clipboard.mobile_consumability.clone(),
                ),
                host_events: Arc::clone(&wired.shared.host_event_bus),
            };
            spawn_host_clipboard_change_task(changes, change_runtime, Arc::clone(&task_registry))
                .await;
        }

        Ok(Self {
            app_version,
            wired,
            paths,
            session,
            task_registry,
            file_transfer_lifecycle,
            _temporary_dir: temporary_dir,
            clipboard_import_root,
            files,
        })
    }

    async fn build_session(
        wired: &WiredDependencies,
        paths: &uc_application::facade::AppPaths,
        file_transfer_lifecycle: &Arc<FileTransferLifecycle>,
    ) -> Result<ProductionSession, EngineError> {
        let lifecycle = build_daemon_lifecycle(&wired.deps, &wired.sync_engine, &wired.shared)
            .await
            .map_err(|error| startup_error("p2p session", error))?;
        let mut sync_engine = lifecycle.sync_engine_assembly;
        let (restore_tx, restore_rx) = tokio::sync::mpsc::unbounded_channel();
        sync_engine.attach_restore_broadcast(restore_rx);
        let search_coordinator = build_search_coordinator(&wired.deps);
        let clipboard = build_clipboard_runtime(wired, &sync_engine);
        let tasks = Arc::new(TaskRegistry::new());
        let search = Arc::clone(&search_coordinator);
        let facade = build_app_facade_from_deps(
            &wired.deps,
            paths,
            Arc::new(InMemoryLifecycleStatus::new()),
            AppFacadeAssemblyOptions {
                space_setup: Some(Arc::clone(&sync_engine.facade)),
                member_roster: Some(Arc::clone(&sync_engine.roster)),
                clipboard_sync: Some(Arc::clone(&sync_engine.clipboard_sync)),
                blob_transfer: Some(Arc::clone(&sync_engine.blob)),
                file_transfer: Some(Arc::clone(&wired.shared.file_transfer_facade)),
                blob_transfer_port: Some(Arc::clone(&sync_engine.blob_transfer)),
                clipboard_restore: Some(ClipboardRestoreAssembly {
                    write_coordinator: Arc::clone(&wired.shared.clipboard_write_coordinator),
                    integration_mode: uc_core::clipboard::ClipboardIntegrationMode::Full,
                    restore_broadcast: Some(
                        uc_application::clipboard_write::RestoreBroadcastTrigger::new(restore_tx),
                    ),
                }),
                search_coordinator: Some(search_coordinator),
                clipboard_outbound: Some(Arc::clone(&clipboard.outbound)),
                ..Default::default()
            },
        );
        spawn_history_maintenance_task(Arc::clone(&facade.clipboard_history), &tasks).await;
        spawn_peer_keepalive_task(Arc::clone(&facade), &tasks).await;
        spawn_clipboard_runtime_tasks(&clipboard, Arc::clone(&sync_engine.clipboard_sync), &tasks)
            .await;
        tasks
            .spawn("search_coordinator", move |cancel| async move {
                if let Err(error) = search.start(cancel).await {
                    error!(error = %error, "search coordinator stopped with error");
                }
            })
            .await;
        let lifecycle = Arc::clone(file_transfer_lifecycle);
        let blob_transfer = Arc::clone(&sync_engine.blob);
        tasks
            .spawn("file_transfer_timeout_sweep", move |cancel| async move {
                let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
                let mut handle = lifecycle.spawn_timeout_sweep(cancel_rx, blob_transfer);
                cancel.cancelled().await;
                let _ = cancel_tx.send(true);
                if tokio::time::timeout(Duration::from_secs(1), &mut handle)
                    .await
                    .is_err()
                {
                    handle.abort();
                }
            })
            .await;

        Ok(ProductionSession {
            facade,
            clipboard,
            sync_engine,
            tasks,
        })
    }

    async fn current_facade(&self) -> Result<Arc<AppFacade>, EngineError> {
        self.session
            .lock()
            .await
            .as_ref()
            .map(|session| Arc::clone(&session.facade))
            .ok_or_else(operation_unavailable_error)
    }
}

#[async_trait]
impl EngineRuntime for ProductionRuntime {
    async fn execute(
        &self,
        operation: Operation,
        cancellation: CancellationToken,
    ) -> Result<OperationResult, EngineError> {
        match operation {
            Operation::CreateSpace(input) => {
                execute_create_space(
                    self.current_facade().await?.as_ref(),
                    self.file_transfer_lifecycle.as_ref(),
                    input,
                )
                .await
            }
            Operation::UnlockSpace(input) => {
                let facade = self.current_facade().await?;
                execute_unlock_space(
                    facade.as_ref(),
                    self.file_transfer_lifecycle.as_ref(),
                    input,
                )
                .await
            }
            Operation::RecoverSession(input) => {
                let facade = self.current_facade().await?;
                execute_recover_session(
                    facade.as_ref(),
                    self.file_transfer_lifecycle.as_ref(),
                    input,
                )
                .await
            }
            Operation::JoinSpace(input) => {
                execute_join_space(
                    self.current_facade().await?.as_ref(),
                    self.file_transfer_lifecycle.as_ref(),
                    input,
                    JoinSpaceMode::Automatic,
                )
                .await
            }
            Operation::IssueInvitation => {
                execute_issue_invitation(self.current_facade().await?.as_ref()).await
            }
            Operation::CancelInvitation => {
                execute_cancel_invitation(self.current_facade().await?.as_ref()).await
            }
            Operation::ResetSpace => {
                execute_reset_space(self.current_facade().await?.as_ref()).await
            }
            Operation::FactoryResetSpace => {
                execute_factory_reset_space(
                    self.current_facade().await?.as_ref(),
                    self.file_transfer_lifecycle.as_ref(),
                )
                .await
            }
            Operation::QuerySetupState => {
                execute_query_setup_state(self.current_facade().await?.as_ref()).await
            }
            Operation::QueryMigrationProgress => {
                execute_query_migration_progress(self.current_facade().await?.as_ref()).await
            }
            Operation::QueryStorageStats => {
                execute_query_storage_stats(self.current_facade().await?.as_ref()).await
            }
            Operation::ClearStorageCache => {
                execute_clear_storage_cache(self.current_facade().await?.as_ref()).await
            }
            Operation::QueryLocalDevice => {
                execute_query_local_device(self.current_facade().await?.as_ref()).await
            }
            Operation::QueryPeerConnections => {
                execute_query_peer_connections(self.current_facade().await?.as_ref()).await
            }
            Operation::RefreshPeerConnections => {
                execute_refresh_peer_connections(self.current_facade().await?.as_ref()).await
            }
            Operation::QuerySettings => {
                execute_query_settings(self.current_facade().await?.as_ref()).await
            }
            Operation::UpdateSettings(patch) => {
                execute_update_settings(self.current_facade().await?.as_ref(), *patch).await
            }
            Operation::ProbeRelay(input) => {
                execute_probe_relay(self.current_facade().await?.as_ref(), input).await
            }
            Operation::QueryUpgradeStatus => {
                execute_query_upgrade_status(
                    self.current_facade().await?.as_ref(),
                    &self.app_version,
                )
                .await
            }
            Operation::AcknowledgeUpgrade => {
                execute_acknowledge_upgrade(
                    self.current_facade().await?.as_ref(),
                    &self.app_version,
                )
                .await
            }
            Operation::QueryEncryptionState => {
                execute_query_encryption_state(self.current_facade().await?.as_ref()).await
            }
            Operation::LockEncryption => {
                execute_lock_encryption(
                    self.current_facade().await?.as_ref(),
                    self.file_transfer_lifecycle.as_ref(),
                )
                .await
            }
            Operation::VerifySecureStorageAccess => {
                execute_verify_secure_storage_access(self.current_facade().await?.as_ref()).await
            }
            Operation::ListDevices => {
                execute_list_devices(self.current_facade().await?.as_ref()).await
            }
            Operation::QueryMemberSyncPreferences(input) => {
                execute_query_member_sync_preferences(self.current_facade().await?.as_ref(), input)
                    .await
            }
            Operation::UpdateMemberSyncPreferences(input) => {
                execute_update_member_sync_preferences(self.current_facade().await?.as_ref(), input)
                    .await
            }
            Operation::RemoveMember(input) => {
                execute_remove_member(self.current_facade().await?.as_ref(), input).await
            }
            Operation::SearchEntries(input) => {
                execute_search_entries(self.current_facade().await?.as_ref(), input).await
            }
            Operation::QuerySearchTags => {
                execute_query_search_tags(self.current_facade().await?.as_ref()).await
            }
            Operation::QuerySearchStatus => {
                execute_query_search_status(self.current_facade().await?.as_ref()).await
            }
            Operation::RebuildSearchIndex => {
                execute_rebuild_search_index(self.current_facade().await?.as_ref()).await
            }
            Operation::QueryHistory(input) => {
                let search_input = history_search_input(input)?;
                let offset = search_input.offset;
                let limit = search_input.limit;
                let page = self
                    .current_facade()
                    .await?
                    .search_query(search_input)
                    .await
                    .map_err(map_query_history_error)?;
                history_page_result(page, offset, limit)
            }
            Operation::ListHistoryEntries(input) => {
                execute_list_history_entries(self.current_facade().await?.as_ref(), input).await
            }
            Operation::GetHistoryEntry(input) => {
                execute_get_history_entry(self.current_facade().await?.as_ref(), input).await
            }
            Operation::DeleteHistoryEntry(input) => {
                execute_delete_history_entry(self.current_facade().await?.as_ref(), input).await
            }
            Operation::SetHistoryEntryFavorite(input) => {
                execute_set_history_entry_favorite(self.current_facade().await?.as_ref(), input)
                    .await
            }
            Operation::QueryHistoryStats => {
                execute_query_history_stats(self.current_facade().await?.as_ref()).await
            }
            Operation::GetHistoryEntryResource(input) => {
                execute_get_history_entry_resource(self.current_facade().await?.as_ref(), input)
                    .await
            }
            Operation::ReadBlob(input) => {
                execute_read_blob(self.current_facade().await?.as_ref(), input).await
            }
            Operation::ReadThumbnail(input) => {
                execute_read_thumbnail(self.current_facade().await?.as_ref(), input).await
            }
            Operation::ReadEntryFile(input) => {
                execute_read_entry_file(self.current_facade().await?.as_ref(), input).await
            }
            Operation::QueryEntryDelivery(input) => {
                execute_query_entry_delivery(self.current_facade().await?.as_ref(), input).await
            }
            Operation::ClearHistory => {
                execute_clear_history(self.current_facade().await?.as_ref()).await
            }
            Operation::QueryEntryReceiveProgress(input) => {
                execute_query_entry_receive_progress(self.current_facade().await?.as_ref(), input)
                    .await
            }
            Operation::ListEntryReceiveProgress => {
                execute_list_entry_receive_progress(self.current_facade().await?.as_ref()).await
            }
            Operation::CancelEntryReceive(input) => {
                execute_cancel_entry_receive(self.current_facade().await?.as_ref(), input).await
            }
            Operation::CancelInboundTransfer(input) => {
                execute_cancel_inbound_transfer(self.current_facade().await?.as_ref(), input).await
            }
            Operation::CaptureCurrentClipboard => {
                execute_capture_current_clipboard(self.current_facade().await?.as_ref()).await
            }
            Operation::RestoreClipboard(input) => {
                execute_restore_clipboard(self.current_facade().await?.as_ref(), input).await
            }
            Operation::SendText(input) => {
                if input.text.is_empty()
                    || input.text.len() > MAX_INLINE_OUTBOUND_REPRESENTATION_BYTES
                {
                    return Err(send_invalid_input_error());
                }
                let snapshot = SystemClipboardSnapshot {
                    ts_ms: self.wired.deps.system.clock.now_ms(),
                    representations: vec![ObservedClipboardRepresentation::new(
                        RepresentationId::new(),
                        FormatId::from("text"),
                        Some(MimeType("text/plain".into())),
                        input.text.into_bytes(),
                    )],
                    file_content_digests: Vec::new(),
                    file_set_v1_component: None,
                };
                self.send_snapshot(snapshot, input.target_devices).await
            }
            Operation::SendImage(input) => {
                if input.bytes.is_empty()
                    || input.bytes.len() > MAX_INLINE_OUTBOUND_REPRESENTATION_BYTES
                    || !input.mime_type.starts_with("image/")
                {
                    return Err(send_invalid_input_error());
                }
                let snapshot = SystemClipboardSnapshot {
                    ts_ms: self.wired.deps.system.clock.now_ms(),
                    representations: vec![ObservedClipboardRepresentation::new(
                        RepresentationId::new(),
                        FormatId::from("image"),
                        Some(MimeType(input.mime_type)),
                        input.bytes,
                    )],
                    file_content_digests: Vec::new(),
                    file_set_v1_component: None,
                };
                self.send_snapshot(snapshot, input.target_devices).await
            }
            Operation::SendFiles(input) => {
                if input.files.is_empty() {
                    return Err(send_invalid_input_error());
                }
                let imported = self.import_host_files(&input.files, &cancellation).await?;
                let uri_list = imported
                    .iter()
                    .map(|file| {
                        url::Url::from_file_path(&file.path)
                            .map(|url| url.to_string())
                            .map_err(|()| send_failed_error())
                    })
                    .collect::<Result<Vec<_>, _>>()?
                    .join("\n");
                let display_metadata = FileDisplayMetadata {
                    files: imported
                        .iter()
                        .map(|file| FileDisplayMetadataEntry {
                            storage_name: file.storage_name.clone(),
                            display_name: file.display_name.clone(),
                        })
                        .collect(),
                }
                .encode()
                .map_err(|error| {
                    error!(error = %error, "failed to encode file display metadata");
                    send_failed_error()
                })?;
                let snapshot = SystemClipboardSnapshot {
                    ts_ms: self.wired.deps.system.clock.now_ms(),
                    representations: vec![
                        ObservedClipboardRepresentation::new(
                            RepresentationId::new(),
                            FormatId::from("files"),
                            Some(MimeType("text/uri-list".into())),
                            uri_list.into_bytes(),
                        ),
                        ObservedClipboardRepresentation::new(
                            RepresentationId::new(),
                            FormatId::from(FILE_DISPLAY_METADATA_FORMAT),
                            Some(MimeType(FILE_DISPLAY_METADATA_MIME.into())),
                            display_metadata,
                        ),
                    ],
                    file_content_digests: Vec::new(),
                    file_set_v1_component: None,
                };
                self.send_snapshot(snapshot, input.target_devices).await
            }
            Operation::ResendEntry(input) => {
                execute_resend_entry(self.current_facade().await?.as_ref(), input).await
            }
            Operation::ExportEntry(input) => {
                let facade = self.current_facade().await?;
                let bytes = load_export_bytes(&facade, &input.entry_id).await?;
                for (index, chunk) in bytes.chunks(EXPORT_CHUNK_SIZE).enumerate() {
                    let offset = (index as u64) * (EXPORT_CHUNK_SIZE as u64);
                    self.files
                        .write_chunk(&input.destination, offset, chunk)
                        .map_err(map_export_host_error)?;
                    tokio::task::yield_now().await;
                }
                self.files
                    .finish_write(&input.destination)
                    .map_err(map_export_host_error)?;
                Ok(OperationResult::EntryExported)
            }
        }
    }

    async fn suspend(&self) -> Result<(), EngineError> {
        let session = self.session.lock().await.take();
        if let Some(session) = session {
            session.tasks.shutdown(Duration::from_secs(5)).await;
            session.sync_engine.shutdown().await;
        }
        Ok(())
    }

    async fn resume(&self) -> Result<(), EngineError> {
        let session =
            Self::build_session(&self.wired, &self.paths, &self.file_transfer_lifecycle).await?;
        *self.session.lock().await = Some(session);
        Ok(())
    }

    async fn shutdown(&self, deadline: Duration) -> Result<(), EngineError> {
        self.suspend().await?;
        self.task_registry.shutdown(deadline).await;
        if let Err(error) = std::fs::remove_dir_all(&self.clipboard_import_root) {
            if error.kind() != std::io::ErrorKind::NotFound {
                warn!(error = %error, "failed to remove host clipboard imports");
            }
        }
        Ok(())
    }
}

impl ProductionRuntime {
    async fn import_host_files(
        &self,
        handles: &[crate::HostFileHandle],
        cancellation: &CancellationToken,
    ) -> Result<Vec<ImportedHostFile>, EngineError> {
        let import_root = self.paths.file_cache_dir.join("engine-imports");
        std::fs::create_dir_all(&import_root).map_err(|error| {
            error!(error = %error, "failed to create engine file import directory");
            send_failed_error()
        })?;
        let operation_dir = import_root.join(RepresentationId::new().to_string());
        std::fs::create_dir(&operation_dir).map_err(|error| {
            error!(error = %error, "failed to create engine file import operation directory");
            send_failed_error()
        })?;

        let mut imported = Vec::with_capacity(handles.len());
        for (index, handle) in handles.iter().enumerate() {
            if cancellation.is_cancelled() {
                cleanup_failed_import(&operation_dir);
                return Err(operation_unavailable_error());
            }
            let metadata = match self.files.metadata(handle) {
                Ok(metadata) => metadata,
                Err(error) => {
                    cleanup_failed_import(&operation_dir);
                    return Err(map_send_host_error(error));
                }
            };
            if metadata.size_bytes == 0 || !valid_host_display_name(&metadata.display_name) {
                cleanup_failed_import(&operation_dir);
                return Err(send_invalid_input_error());
            }
            let storage_name = format!("{index:08}");
            let path = operation_dir.join(&storage_name);
            if let Err(error) = copy_host_file(
                self.files.as_ref(),
                handle,
                metadata.size_bytes,
                &path,
                cancellation,
            ) {
                cleanup_failed_import(&operation_dir);
                return Err(error);
            }
            imported.push(ImportedHostFile {
                path,
                storage_name,
                display_name: metadata.display_name,
            });
            tokio::task::yield_now().await;
        }
        Ok(imported)
    }

    async fn send_snapshot(
        &self,
        snapshot: SystemClipboardSnapshot,
        target_devices: Vec<String>,
    ) -> Result<OperationResult, EngineError> {
        let (capture, live_index, outbound) = {
            let session = self.session.lock().await;
            let session = session.as_ref().ok_or_else(operation_unavailable_error)?;
            (
                Arc::clone(&session.clipboard.capture),
                Arc::clone(&session.clipboard.live_index),
                Arc::clone(&session.clipboard.outbound),
            )
        };
        let captured = capture
            .capture(snapshot.clone(), ClipboardChangeOrigin::LocalCapture, None)
            .await
            .map_err(|error| operation_error_with_code(SEND_FAILED_CODE, "capture send", error))?
            .ok_or_else(|| {
                EngineError::new(SEND_SKIPPED_CODE, EngineErrorCategory::Conflict, false)
            })?;
        if !captured.deduplicated {
            if let Err(error) = live_index
                .index_capture(ClipboardLiveIndexInput {
                    entry_id: captured.entry_id.clone(),
                    snapshot: Arc::new(snapshot.clone()),
                })
                .await
            {
                warn!(error = %error, "failed to index engine send");
            }
        }
        let target_filter = (!target_devices.is_empty()).then(|| {
            target_devices
                .into_iter()
                .map(DeviceId::new)
                .collect::<Vec<_>>()
        });
        let outcome = outbound
            .dispatch_capture_to_targets(
                ClipboardOutboundInput {
                    entry_id: captured.entry_id.clone(),
                    snapshot,
                    origin: ClipboardChangeOrigin::LocalCapture,
                },
                target_filter,
            )
            .await
            .map_err(|error| {
                operation_error_with_code(SEND_FAILED_CODE, "send clipboard", error)
            })?;
        send_report_result(captured.entry_id, outcome)
    }
}

fn send_report_result(
    entry_id: String,
    outcome: ClipboardOutboundOutcome,
) -> Result<OperationResult, EngineError> {
    let ClipboardOutboundOutcome::Dispatched {
        snapshot_hash,
        per_target,
        accepted,
        duplicate,
        offline,
        errored,
        pending,
        at_ms,
        ..
    } = outcome
    else {
        return Err(EngineError::new(
            SEND_SKIPPED_CODE,
            EngineErrorCategory::Conflict,
            false,
        ));
    };

    Ok(OperationResult::EntrySent(crate::SendReportSummary {
        entry_id,
        snapshot_hash,
        at_ms,
        total_accepted: accepted,
        total_duplicate: duplicate,
        total_offline: offline,
        total_errored: errored,
        total_pending: pending,
        per_target: per_target
            .into_iter()
            .map(|target| crate::SendTargetSummary {
                device_id: target.device_id.as_str().to_string(),
                outcome: match target.outcome {
                    Ok(uc_core::ports::DispatchAck::Accepted) => crate::SendTargetOutcome::Accepted,
                    Ok(uc_core::ports::DispatchAck::DuplicateIgnored) => {
                        crate::SendTargetOutcome::Duplicate
                    }
                    Err(message) => crate::SendTargetOutcome::Error { message },
                },
            })
            .collect(),
    }))
}

fn valid_host_display_name(display_name: &str) -> bool {
    let name = display_name.trim();
    !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\\', '\0'])
}

fn copy_host_file(
    files: &dyn HostFileAccess,
    handle: &crate::HostFileHandle,
    size_bytes: u64,
    destination: &std::path::Path,
    cancellation: &CancellationToken,
) -> Result<(), EngineError> {
    let mut output = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(destination)
        .map_err(|error| {
            error!(error = %error, "failed to create imported host file");
            send_failed_error()
        })?;
    let mut offset = 0_u64;
    while offset < size_bytes {
        if cancellation.is_cancelled() {
            return Err(operation_unavailable_error());
        }
        let remaining = size_bytes - offset;
        let requested = remaining.min(EXPORT_CHUNK_SIZE as u64) as u32;
        let chunk = files
            .read_chunk(handle, offset, requested)
            .map_err(map_send_host_error)?;
        if chunk.is_empty() || chunk.len() > requested as usize {
            return Err(send_failed_error());
        }
        output.write_all(&chunk).map_err(|error| {
            error!(error = %error, "failed to write imported host file");
            send_failed_error()
        })?;
        offset = offset
            .checked_add(chunk.len() as u64)
            .ok_or_else(send_failed_error)?;
    }
    output.sync_all().map_err(|error| {
        error!(error = %error, "failed to sync imported host file");
        send_failed_error()
    })
}

fn cleanup_failed_import(operation_dir: &std::path::Path) {
    if let Err(error) = std::fs::remove_dir_all(operation_dir) {
        warn!(error = %error, "failed to remove incomplete engine file import");
    }
}

fn send_invalid_input_error() -> EngineError {
    EngineError::new(
        SEND_INVALID_INPUT_CODE,
        EngineErrorCategory::InvalidInput,
        false,
    )
}

fn send_failed_error() -> EngineError {
    EngineError::new(SEND_FAILED_CODE, EngineErrorCategory::Internal, false)
}

fn map_send_host_error(error: crate::HostCapabilityError) -> EngineError {
    let (category, retryable) = match error.category() {
        crate::HostCapabilityErrorCategory::InvalidHandle => {
            (EngineErrorCategory::InvalidInput, false)
        }
        crate::HostCapabilityErrorCategory::PermissionDenied => {
            (EngineErrorCategory::Unauthorized, false)
        }
        crate::HostCapabilityErrorCategory::Unavailable
        | crate::HostCapabilityErrorCategory::Io => (EngineErrorCategory::Unavailable, true),
    };
    error!(error = %error, "host file import failed");
    EngineError::new(SEND_FAILED_CODE, category, retryable)
}

async fn load_export_bytes(facade: &AppFacade, entry_id: &str) -> Result<Vec<u8>, EngineError> {
    let resource = facade
        .clipboard_history
        .get_entry_resource(entry_id)
        .await
        .map_err(map_export_history_error)?;
    let file_list = resource.mime_type.as_deref().is_some_and(|mime| {
        mime.eq_ignore_ascii_case("text/uri-list") || mime.eq_ignore_ascii_case("file/uri-list")
    });
    if file_list {
        return facade
            .resource
            .entry_file(entry_id)
            .await
            .map(|file| file.bytes)
            .map_err(map_export_resource_error);
    }
    if let Some(bytes) = resource.inline_data {
        return Ok(bytes);
    }
    if let Some(blob_id) = resource.blob_id {
        return facade
            .resource
            .blob(&blob_id)
            .await
            .map(|blob| blob.bytes)
            .map_err(map_export_resource_error);
    }
    Err(EngineError::new(
        EXPORT_FAILED_CODE,
        EngineErrorCategory::Internal,
        false,
    ))
}

fn map_export_history_error(error: ClipboardHistoryError) -> EngineError {
    match error {
        ClipboardHistoryError::NotFound => {
            EngineError::new(EXPORT_NOT_FOUND_CODE, EngineErrorCategory::NotFound, false)
        }
        ClipboardHistoryError::UnsupportedContent => {
            EngineError::new(EXPORT_FAILED_CODE, EngineErrorCategory::Conflict, false)
        }
        ClipboardHistoryError::Internal(_) => {
            operation_error_with_code(EXPORT_FAILED_CODE, "load export entry", error)
        }
    }
}

fn map_export_resource_error(error: ResourceFacadeError) -> EngineError {
    match error {
        ResourceFacadeError::NotFound => {
            EngineError::new(EXPORT_NOT_FOUND_CODE, EngineErrorCategory::NotFound, false)
        }
        ResourceFacadeError::Mismatch(_) | ResourceFacadeError::Internal(_) => {
            operation_error_with_code(EXPORT_FAILED_CODE, "load export resource", error)
        }
    }
}

fn map_export_host_error(error: crate::HostCapabilityError) -> EngineError {
    let (code, category, retryable) = match error.category() {
        crate::HostCapabilityErrorCategory::InvalidHandle => (
            EXPORT_INVALID_TARGET_CODE,
            EngineErrorCategory::InvalidInput,
            false,
        ),
        crate::HostCapabilityErrorCategory::PermissionDenied => (
            EXPORT_UNAUTHORIZED_CODE,
            EngineErrorCategory::Unauthorized,
            false,
        ),
        crate::HostCapabilityErrorCategory::Unavailable
        | crate::HostCapabilityErrorCategory::Io => (
            EXPORT_UNAVAILABLE_CODE,
            EngineErrorCategory::Unavailable,
            true,
        ),
    };
    error!(error = %error, "host export failed");
    EngineError::new(code, category, retryable)
}

fn build_search_coordinator(deps: &uc_application::deps::AppDeps) -> Arc<SearchCoordinator> {
    Arc::new(SearchCoordinator::new(SearchCoordinatorDeps::new(
        deps.search.search_index.clone(),
        deps.search.search_maintenance.clone(),
        deps.search.search_key_derivation.clone(),
        deps.search.search_pipeline.clone(),
        deps.clipboard.entry_ports.list.clone(),
        deps.clipboard.entry_ports.get.clone(),
        deps.clipboard.representation_ports.list_for_event.clone(),
        deps.clipboard.selection_repo.clone(),
        deps.clipboard.clipboard_event_reader_repo.clone(),
        deps.storage.entry_file_set_repo.clone(),
        uc_infra::search::constants::CURRENT_INDEX_VERSION,
    )))
}

fn history_search_input(input: QueryHistoryInput) -> Result<SearchQueryInput, EngineError> {
    if input.limit == 0 || input.limit > MAX_HISTORY_PAGE_SIZE {
        return Err(query_history_invalid_input_error());
    }
    let offset = match input.cursor.as_deref() {
        None => 0,
        Some(cursor) => cursor
            .strip_prefix(HISTORY_CURSOR_PREFIX)
            .and_then(|value| value.parse::<u32>().ok())
            .ok_or_else(query_history_invalid_input_error)?,
    };

    Ok(SearchQueryInput {
        query: input.query.unwrap_or_default(),
        operator: None,
        time_preset: None,
        from_ms: None,
        to_ms: None,
        content_types: None,
        extensions: None,
        source_devices: None,
        tags: None,
        limit: input.limit,
        offset,
    })
}

fn history_page_result(
    page: SearchPageView,
    offset: u32,
    limit: u32,
) -> Result<OperationResult, EngineError> {
    let next_cursor = if page.has_more {
        let next_offset = offset
            .checked_add(limit)
            .ok_or_else(query_history_invalid_input_error)?;
        Some(format!("{HISTORY_CURSOR_PREFIX}{next_offset}"))
    } else {
        None
    };
    let entries = page
        .items
        .into_iter()
        .map(|item| EntrySummary {
            entry_id: item.entry_id,
            content_type: item.content_type,
            preview: item.text_preview,
            created_at_ms: item.active_time_ms,
        })
        .collect();

    Ok(OperationResult::HistoryPage {
        entries,
        next_cursor,
    })
}

fn startup_error(context: &'static str, error: impl std::fmt::Display) -> EngineError {
    error!(context, error = %error, "engine startup failed");
    EngineError::new(START_FAILED_CODE, EngineErrorCategory::Unavailable, true)
}

fn operation_unavailable_error() -> EngineError {
    EngineError::new(
        OPERATION_UNAVAILABLE_CODE,
        EngineErrorCategory::Unavailable,
        false,
    )
}

fn map_query_history_error(error: SearchFacadeError) -> EngineError {
    match error {
        SearchFacadeError::InvalidQuery(_) | SearchFacadeError::BadRequest(_) => {
            query_history_invalid_input_error()
        }
        SearchFacadeError::SessionLocked => EngineError::new(
            QUERY_HISTORY_UNAUTHORIZED_CODE,
            EngineErrorCategory::Unauthorized,
            false,
        ),
        SearchFacadeError::IndexNotReady
        | SearchFacadeError::IndexRebuilding
        | SearchFacadeError::IndexUnavailable
        | SearchFacadeError::ServiceUnavailable(_) => EngineError::new(
            QUERY_HISTORY_UNAVAILABLE_CODE,
            EngineErrorCategory::Unavailable,
            true,
        ),
        SearchFacadeError::RebuildAlreadyRunning => EngineError::new(
            QUERY_HISTORY_UNAVAILABLE_CODE,
            EngineErrorCategory::Conflict,
            true,
        ),
        SearchFacadeError::Internal(_) => {
            operation_error_with_code(QUERY_HISTORY_FAILED_CODE, "query history", error)
        }
    }
}

fn query_history_invalid_input_error() -> EngineError {
    EngineError::new(
        QUERY_HISTORY_INVALID_INPUT_CODE,
        EngineErrorCategory::InvalidInput,
        false,
    )
}

fn operation_error_with_code(
    code: u32,
    context: &'static str,
    error: impl std::fmt::Display,
) -> EngineError {
    error!(context, error = %error, "engine operation failed");
    EngineError::new(code, EngineErrorCategory::Internal, false)
}

#[cfg(test)]
mod tests {
    use uc_application::facade::{
        SearchPageView, SearchResultView, StorageFacadeError, StorageStatsView,
    };

    use super::*;
    use crate::internal::storage::{
        map_storage_error, storage_stats_result, CLEAR_STORAGE_CACHE_FAILED_CODE,
        QUERY_STORAGE_STATS_FAILED_CODE,
    };
    use crate::StorageStatsSummary;

    #[test]
    fn history_search_input_parses_only_versioned_bounded_cursors() {
        let parsed = history_search_input(QueryHistoryInput {
            cursor: Some("uc-history-v1:40".into()),
            limit: 20,
            query: Some("needle".into()),
        })
        .unwrap();
        assert_eq!(parsed.offset, 40);
        assert_eq!(parsed.limit, 20);
        assert_eq!(parsed.query, "needle");

        for input in [
            QueryHistoryInput {
                cursor: Some("40".into()),
                limit: 20,
                query: None,
            },
            QueryHistoryInput {
                cursor: Some("uc-history-v2:40".into()),
                limit: 20,
                query: None,
            },
            QueryHistoryInput {
                cursor: None,
                limit: 0,
                query: None,
            },
            QueryHistoryInput {
                cursor: None,
                limit: 201,
                query: None,
            },
        ] {
            let error = history_search_input(input).unwrap_err();
            assert_eq!(error.category(), EngineErrorCategory::InvalidInput);
        }
    }

    #[test]
    fn history_page_result_projects_entries_and_advances_cursor() {
        let result = history_page_result(
            SearchPageView {
                total: 61,
                has_more: true,
                items: vec![SearchResultView {
                    entry_id: "entry-1".into(),
                    content_type: "text".into(),
                    active_time_ms: 123,
                    tags: Vec::new(),
                    text_preview: Some("private preview".into()),
                    char_count: Some(15),
                    mime_type: "text/plain".into(),
                    file_extensions: Vec::new(),
                    file_names: Vec::new(),
                    file_paths: Vec::new(),
                    link_urls: Vec::new(),
                    source_device: None,
                    payload_state: None,
                }],
                state: "ready".into(),
            },
            40,
            20,
        )
        .unwrap();

        assert_eq!(
            result,
            OperationResult::HistoryPage {
                entries: vec![EntrySummary {
                    entry_id: "entry-1".into(),
                    content_type: "text".into(),
                    preview: Some("private preview".into()),
                    created_at_ms: 123,
                }],
                next_cursor: Some("uc-history-v1:60".into()),
            }
        );
    }

    #[test]
    fn history_error_mapping_preserves_retry_semantics() {
        let locked = map_query_history_error(SearchFacadeError::SessionLocked);
        assert_eq!(locked.category(), EngineErrorCategory::Unauthorized);
        assert!(!locked.is_retryable());

        let rebuilding = map_query_history_error(SearchFacadeError::IndexRebuilding);
        assert_eq!(rebuilding.category(), EngineErrorCategory::Unavailable);
        assert!(rebuilding.is_retryable());
    }

    #[test]
    fn send_result_preserves_every_dispatch_field() {
        let result = send_report_result(
            "entry-1".into(),
            ClipboardOutboundOutcome::Dispatched {
                snapshot_hash: "hash-1".into(),
                per_target: vec![uc_application::facade::DispatchEntryPerTarget {
                    device_id: DeviceId::new("device-1"),
                    outcome: Err("private failure detail".into()),
                }],
                accepted: 1,
                duplicate: 2,
                offline: 3,
                errored: 4,
                pending: 5,
                at_ms: 123,
                blob_ref_count: 6,
            },
        )
        .unwrap();

        let OperationResult::EntrySent(report) = result else {
            panic!("expected entry-sent result");
        };
        assert_eq!(report.entry_id, "entry-1");
        assert_eq!(report.snapshot_hash, "hash-1");
        assert_eq!(report.at_ms, 123);
        assert_eq!(report.total_accepted, 1);
        assert_eq!(report.total_duplicate, 2);
        assert_eq!(report.total_offline, 3);
        assert_eq!(report.total_errored, 4);
        assert_eq!(report.total_pending, 5);
        assert_eq!(report.per_target.len(), 1);
        assert!(!format!("{report:?}").contains("private failure detail"));
    }

    #[test]
    fn storage_stats_projection_does_not_expose_the_host_data_path() {
        let result = storage_stats_result(StorageStatsView {
            total_bytes: 50,
            database_bytes: 10,
            vault_bytes: 20,
            cache_bytes: 15,
            logs_bytes: 5,
            data_dir: "/private/user/path".into(),
        });

        assert_eq!(
            result,
            OperationResult::StorageStats(StorageStatsSummary {
                total_bytes: 50,
                database_bytes: 10,
                vault_bytes: 20,
                cache_bytes: 15,
                logs_bytes: 5,
            })
        );
        assert!(!format!("{result:?}").contains("/private/user/path"));
    }

    #[test]
    fn storage_failures_use_distinct_stable_codes() {
        let stats = map_storage_error(StorageFacadeError::Stats("private detail".into()));
        let clear = map_storage_error(StorageFacadeError::ClearCache("private detail".into()));

        assert_eq!(stats.code(), QUERY_STORAGE_STATS_FAILED_CODE);
        assert_eq!(clear.code(), CLEAR_STORAGE_CACHE_FAILED_CODE);
        assert_eq!(stats.category(), EngineErrorCategory::Internal);
        assert_eq!(clear.category(), EngineErrorCategory::Internal);
    }
}
