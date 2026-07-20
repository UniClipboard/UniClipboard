use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use tracing::{error, warn};
use uc_application::facade::space_setup::{SwitchSpaceError, SwitchSpaceInput};
use uc_application::facade::{
    AppFacade, ClipboardHistoryError, InMemoryLifecycleStatus, InitializeSpaceError,
    InitializeSpaceInput as AppInitializeSpaceInput, IssuePairingInvitationError,
    QuerySetupStateError, RedeemPairingInvitationError, RedeemPairingInvitationInput,
    ResendEntryCommand, ResendEntryError, ResourceFacadeError, SearchCoordinator,
    SearchCoordinatorDeps, SearchFacadeError, SearchPageView, SearchQueryInput, UnlockSpaceError,
    UnlockSpaceInput as AppUnlockSpaceInput, MAX_INLINE_OUTBOUND_REPRESENTATION_BYTES,
};
use uc_application::facade::{
    ClipboardLiveIndexInput, ClipboardOutboundInput, ClipboardOutboundOutcome,
};
use uc_core::ids::{DeviceId, FormatId, RepresentationId};
use uc_core::ports::ReachabilityState;
use uc_core::TaskRegistry;
use uc_core::{
    ClipboardChangeOrigin, MimeType, ObservedClipboardRepresentation, SystemClipboardSnapshot,
};

use crate::engine::EngineRuntime;
use crate::event_stream::EventSender;
use crate::internal::blob_tasks::{spawn_blob_processing_tasks, BlobProcessingPorts};
use crate::internal::clipboard_runtime::{
    build_clipboard_runtime, spawn_clipboard_runtime_tasks, ClipboardRuntime,
};
use crate::internal::deps::WiredDependencies;
use crate::internal::facade::{
    build_app_facade_from_deps, AppFacadeAssemblyOptions, ClipboardRestoreAssembly,
};
use crate::internal::file_transfer::FileTransferLifecycle;
use crate::internal::host_adapters::{
    wire_host_capabilities_with_emitter, EngineHostEventEmitter, HostWiring,
};
use crate::internal::lifecycle::build_daemon_lifecycle;
use crate::internal::sync_engine::SyncEngineAssembly;
use crate::{
    DeviceSummary, EngineConfig, EngineError, EngineErrorCategory, EntrySummary, HostCapabilities,
    HostFileAccess, Operation, OperationResult, QueryHistoryInput,
};

const START_FAILED_CODE: u32 = 1101;
const OPERATION_FAILED_CODE: u32 = 1102;
const OPERATION_UNAVAILABLE_CODE: u32 = 1103;
const CREATE_SPACE_INVALID_INPUT_CODE: u32 = 1201;
const CREATE_SPACE_CONFLICT_CODE: u32 = 1202;
const CREATE_SPACE_FAILED_CODE: u32 = 1203;
const UNLOCK_SPACE_INVALID_STATE_CODE: u32 = 1211;
const UNLOCK_SPACE_UNAUTHORIZED_CODE: u32 = 1212;
const UNLOCK_SPACE_FAILED_CODE: u32 = 1213;
const INVITATION_INVALID_STATE_CODE: u32 = 1221;
const INVITATION_INVALID_INPUT_CODE: u32 = 1222;
const INVITATION_UNAVAILABLE_CODE: u32 = 1223;
const INVITATION_FAILED_CODE: u32 = 1224;
const JOIN_SPACE_INVALID_INPUT_CODE: u32 = 1231;
const JOIN_SPACE_INVALID_STATE_CODE: u32 = 1232;
const JOIN_SPACE_UNAUTHORIZED_CODE: u32 = 1233;
const JOIN_SPACE_NOT_FOUND_CODE: u32 = 1234;
const JOIN_SPACE_CONFLICT_CODE: u32 = 1235;
const JOIN_SPACE_UNAVAILABLE_CODE: u32 = 1236;
const JOIN_SPACE_DEADLINE_CODE: u32 = 1237;
const JOIN_SPACE_FAILED_CODE: u32 = 1238;
const QUERY_HISTORY_INVALID_INPUT_CODE: u32 = 1241;
const QUERY_HISTORY_UNAUTHORIZED_CODE: u32 = 1242;
const QUERY_HISTORY_UNAVAILABLE_CODE: u32 = 1243;
const QUERY_HISTORY_FAILED_CODE: u32 = 1244;
const HISTORY_CURSOR_PREFIX: &str = "uc-history-v1:";
const MAX_HISTORY_PAGE_SIZE: u32 = 200;
const SEND_INVALID_INPUT_CODE: u32 = 1251;
const SEND_FAILED_CODE: u32 = 1252;
const SEND_SKIPPED_CODE: u32 = 1253;
const RESEND_NOT_FOUND_CODE: u32 = 1261;
const RESEND_CONFLICT_CODE: u32 = 1262;
const RESEND_UNAUTHORIZED_CODE: u32 = 1263;
const RESEND_FAILED_CODE: u32 = 1264;
const EXPORT_NOT_FOUND_CODE: u32 = 1271;
const EXPORT_INVALID_TARGET_CODE: u32 = 1272;
const EXPORT_UNAUTHORIZED_CODE: u32 = 1273;
const EXPORT_UNAVAILABLE_CODE: u32 = 1274;
const EXPORT_FAILED_CODE: u32 = 1275;
const EXPORT_CHUNK_SIZE: usize = 64 * 1024;

pub(crate) struct ProductionRuntime {
    wired: WiredDependencies,
    paths: uc_application::facade::AppPaths,
    session: Mutex<Option<ProductionSession>>,
    task_registry: Arc<TaskRegistry>,
    file_transfer_lifecycle: Arc<FileTransferLifecycle>,
    _temporary_dir: std::path::PathBuf,
    files: Box<dyn HostFileAccess>,
}

struct ProductionSession {
    facade: Arc<AppFacade>,
    clipboard: ClipboardRuntime,
    sync_engine: SyncEngineAssembly,
    tasks: Arc<TaskRegistry>,
}

impl ProductionRuntime {
    pub(crate) async fn start(
        config: EngineConfig,
        host: HostCapabilities,
        events: EventSender,
    ) -> Result<Self, EngineError> {
        let emitter = Arc::new(EngineHostEventEmitter::new(events));
        let HostWiring {
            wired,
            background,
            paths,
            temporary_dir,
            files,
        } = wire_host_capabilities_with_emitter(&config, host, emitter)
            .map_err(|error| startup_error("dependency wiring", error))?;

        let file_transfer_lifecycle = Arc::clone(&background.file_transfer_lifecycle);
        let session = Self::build_session(&wired, &paths, &file_transfer_lifecycle).await?;
        let task_registry = Arc::new(TaskRegistry::new());
        let blob_ports = BlobProcessingPorts::from_app_deps(&wired.deps);
        spawn_blob_processing_tasks(background, blob_ports, &task_registry).await;

        Ok(Self {
            wired,
            paths,
            session: Mutex::new(Some(session)),
            task_registry,
            file_transfer_lifecycle,
            _temporary_dir: temporary_dir,
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
        spawn_clipboard_runtime_tasks(&clipboard, Arc::clone(&sync_engine.clipboard_sync), &tasks)
            .await;
        let search = Arc::clone(&search_coordinator);
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
        _cancellation: CancellationToken,
    ) -> Result<OperationResult, EngineError> {
        match operation {
            Operation::CreateSpace(input) => {
                let result = self
                    .current_facade()
                    .await?
                    .initialize_space(AppInitializeSpaceInput {
                        passphrase: input.passphrase.expose().to_owned(),
                        passphrase_confirm: input.passphrase_confirmation.expose().to_owned(),
                        device_name: Some(input.device_name),
                    })
                    .await
                    .map_err(map_create_space_error)?;
                Ok(OperationResult::SpaceCreated {
                    space_id: result.space_id.as_ref().to_string(),
                })
            }
            Operation::UnlockSpace(input) => {
                let facade = self.current_facade().await?;
                facade
                    .unlock_space(AppUnlockSpaceInput {
                        passphrase: input.passphrase.expose().to_owned(),
                    })
                    .await
                    .map_err(map_unlock_space_error)?;
                facade.search.on_session_ready().await;
                Ok(OperationResult::SpaceUnlocked)
            }
            Operation::JoinSpace(input) => {
                let device_name = input.device_name.trim().to_owned();
                if device_name.is_empty() {
                    return Err(join_invalid_input_error());
                }
                let facade = self.current_facade().await?;
                facade.set_device_name(device_name).await.map_err(|error| {
                    operation_error_with_code(
                        JOIN_SPACE_FAILED_CODE,
                        "save join device name",
                        error,
                    )
                })?;
                let setup = facade
                    .query_setup_state()
                    .await
                    .map_err(|error| map_query_setup_state_error("route join operation", error))?;
                let space_id = if setup.has_completed {
                    facade
                        .switch_space(SwitchSpaceInput {
                            code: input.invitation_code,
                            new_passphrase: input.passphrase.expose().to_owned(),
                        })
                        .await
                        .map_err(map_switch_space_error)?
                        .space_id
                } else {
                    facade
                        .redeem_pairing_invitation(RedeemPairingInvitationInput {
                            code: input.invitation_code,
                            passphrase: input.passphrase.expose().to_owned(),
                        })
                        .await
                        .map_err(map_join_space_error)?
                        .space_id
                };
                Ok(OperationResult::SpaceJoined {
                    space_id: space_id.as_ref().to_string(),
                })
            }
            Operation::IssueInvitation => {
                let invitation = self
                    .current_facade()
                    .await?
                    .issue_pairing_invitation()
                    .await
                    .map_err(map_issue_invitation_error)?;
                Ok(OperationResult::InvitationIssued {
                    invitation_code: invitation.code.as_str().to_string(),
                })
            }
            Operation::ListDevices => {
                let entries = self
                    .current_facade()
                    .await?
                    .list_roster_entries()
                    .await
                    .map_err(|error| operation_error("list devices", error))?;
                Ok(OperationResult::Devices(
                    entries
                        .into_iter()
                        .map(|entry| DeviceSummary {
                            device_id: entry.device_id.as_str().to_string(),
                            display_name: entry.device_name,
                            online: entry.state == ReachabilityState::Online,
                        })
                        .collect(),
                ))
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
            Operation::ResendEntry(input) => {
                let entry_id = input.entry_id;
                let target_filter = (!input.target_devices.is_empty()).then(|| {
                    input
                        .target_devices
                        .into_iter()
                        .map(DeviceId::new)
                        .collect()
                });
                self.current_facade()
                    .await?
                    .resend_entry(ResendEntryCommand {
                        entry_id: uc_core::ids::EntryId::from(entry_id.as_str()),
                        target_filter,
                    })
                    .await
                    .map_err(map_resend_error)?;
                Ok(OperationResult::EntryResent { entry_id })
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
            _ => Err(operation_unavailable_error()),
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
        Ok(())
    }
}

impl ProductionRuntime {
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
        match outbound
            .dispatch_capture_to_targets(
                ClipboardOutboundInput {
                    entry_id: captured.entry_id.clone(),
                    snapshot,
                    origin: ClipboardChangeOrigin::LocalCapture,
                },
                target_filter,
            )
            .await
            .map_err(|error| operation_error_with_code(SEND_FAILED_CODE, "send clipboard", error))?
        {
            ClipboardOutboundOutcome::Dispatched { .. } => Ok(OperationResult::EntrySent {
                entry_id: captured.entry_id,
            }),
            ClipboardOutboundOutcome::Skipped { .. } => Err(EngineError::new(
                SEND_SKIPPED_CODE,
                EngineErrorCategory::Conflict,
                false,
            )),
        }
    }
}

fn send_invalid_input_error() -> EngineError {
    EngineError::new(
        SEND_INVALID_INPUT_CODE,
        EngineErrorCategory::InvalidInput,
        false,
    )
}

fn map_resend_error(error: ResendEntryError) -> EngineError {
    match error {
        ResendEntryError::EntryNotFound(_) => {
            EngineError::new(RESEND_NOT_FOUND_CODE, EngineErrorCategory::NotFound, false)
        }
        ResendEntryError::EntryNotResendable { .. } | ResendEntryError::NoEligibleTargets => {
            EngineError::new(RESEND_CONFLICT_CODE, EngineErrorCategory::Conflict, false)
        }
        ResendEntryError::TargetNotTrusted(_) => EngineError::new(
            RESEND_UNAUTHORIZED_CODE,
            EngineErrorCategory::Unauthorized,
            false,
        ),
        ResendEntryError::Storage(_) | ResendEntryError::Dispatch(_) => {
            operation_error_with_code(RESEND_FAILED_CODE, "resend entry", error)
        }
    }
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

fn operation_error(context: &'static str, error: impl std::fmt::Display) -> EngineError {
    error!(context, error = %error, "engine operation failed");
    EngineError::new(OPERATION_FAILED_CODE, EngineErrorCategory::Internal, false)
}

fn operation_unavailable_error() -> EngineError {
    EngineError::new(
        OPERATION_UNAVAILABLE_CODE,
        EngineErrorCategory::Unavailable,
        false,
    )
}

fn map_create_space_error(error: InitializeSpaceError) -> EngineError {
    match error {
        InitializeSpaceError::PassphraseMismatch | InitializeSpaceError::DeviceNameRequired => {
            EngineError::new(
                CREATE_SPACE_INVALID_INPUT_CODE,
                EngineErrorCategory::InvalidInput,
                false,
            )
        }
        InitializeSpaceError::AlreadyInitialized | InitializeSpaceError::AlreadySetup => {
            EngineError::new(
                CREATE_SPACE_CONFLICT_CODE,
                EngineErrorCategory::Conflict,
                false,
            )
        }
        InitializeSpaceError::StorageFailed(_) | InitializeSpaceError::Internal(_) => {
            operation_error_with_code(CREATE_SPACE_FAILED_CODE, "create space", error)
        }
    }
}

fn map_unlock_space_error(error: UnlockSpaceError) -> EngineError {
    match error {
        UnlockSpaceError::SetupNotCompleted | UnlockSpaceError::SpaceNotInitialized => {
            EngineError::new(
                UNLOCK_SPACE_INVALID_STATE_CODE,
                EngineErrorCategory::InvalidState,
                false,
            )
        }
        UnlockSpaceError::WrongPassphrase => EngineError::new(
            UNLOCK_SPACE_UNAUTHORIZED_CODE,
            EngineErrorCategory::Unauthorized,
            false,
        ),
        UnlockSpaceError::CorruptedKeyMaterial | UnlockSpaceError::Internal(_) => {
            operation_error_with_code(UNLOCK_SPACE_FAILED_CODE, "unlock space", error)
        }
    }
}

fn map_issue_invitation_error(error: IssuePairingInvitationError) -> EngineError {
    match error {
        IssuePairingInvitationError::NetworkNotStarted => EngineError::new(
            INVITATION_INVALID_STATE_CODE,
            EngineErrorCategory::InvalidState,
            true,
        ),
        IssuePairingInvitationError::AddressNotAvailable(_) => EngineError::new(
            INVITATION_INVALID_INPUT_CODE,
            EngineErrorCategory::InvalidInput,
            false,
        ),
        IssuePairingInvitationError::ServiceUnavailable => EngineError::new(
            INVITATION_UNAVAILABLE_CODE,
            EngineErrorCategory::Unavailable,
            true,
        ),
        IssuePairingInvitationError::Internal(_) => {
            operation_error_with_code(INVITATION_FAILED_CODE, "issue invitation", error)
        }
    }
}

fn map_query_setup_state_error(context: &'static str, error: QuerySetupStateError) -> EngineError {
    operation_error_with_code(JOIN_SPACE_FAILED_CODE, context, error)
}

fn map_join_space_error(error: RedeemPairingInvitationError) -> EngineError {
    match error {
        RedeemPairingInvitationError::DeviceNameRequired => join_invalid_input_error(),
        RedeemPairingInvitationError::PassphraseMismatch => join_unauthorized_error(),
        RedeemPairingInvitationError::InvitationNotFound
        | RedeemPairingInvitationError::InvitationExpired => join_not_found_error(),
        RedeemPairingInvitationError::SponsorRejectedInvitation
        | RedeemPairingInvitationError::SponsorDeclined => join_conflict_error(),
        RedeemPairingInvitationError::SponsorUnreachable
        | RedeemPairingInvitationError::ServiceUnavailable
        | RedeemPairingInvitationError::ConnectionLost => join_unavailable_error(),
        RedeemPairingInvitationError::SponsorTimedOut | RedeemPairingInvitationError::Timeout => {
            join_deadline_error()
        }
        RedeemPairingInvitationError::CorruptedKeyMaterial
        | RedeemPairingInvitationError::SponsorInternal(_)
        | RedeemPairingInvitationError::Internal(_) => {
            operation_error_with_code(JOIN_SPACE_FAILED_CODE, "join space", error)
        }
    }
}

fn map_switch_space_error(error: SwitchSpaceError) -> EngineError {
    match error {
        SwitchSpaceError::DeviceNameRequired => join_invalid_input_error(),
        SwitchSpaceError::NotSetup | SwitchSpaceError::NotUnlocked => EngineError::new(
            JOIN_SPACE_INVALID_STATE_CODE,
            EngineErrorCategory::InvalidState,
            false,
        ),
        SwitchSpaceError::PassphraseMismatch => join_unauthorized_error(),
        SwitchSpaceError::InvitationNotFound | SwitchSpaceError::InvitationExpired => {
            join_not_found_error()
        }
        SwitchSpaceError::PendingMigration(_)
        | SwitchSpaceError::SponsorDeclined
        | SwitchSpaceError::SponsorRejectedInvitation => join_conflict_error(),
        SwitchSpaceError::SponsorUnreachable
        | SwitchSpaceError::ServiceUnavailable
        | SwitchSpaceError::ConnectionLost => join_unavailable_error(),
        SwitchSpaceError::Timeout => join_deadline_error(),
        SwitchSpaceError::CorruptedKeyMaterial
        | SwitchSpaceError::InvalidCiphertext
        | SwitchSpaceError::Storage(_)
        | SwitchSpaceError::Internal(_) => {
            operation_error_with_code(JOIN_SPACE_FAILED_CODE, "switch space", error)
        }
    }
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

fn join_invalid_input_error() -> EngineError {
    EngineError::new(
        JOIN_SPACE_INVALID_INPUT_CODE,
        EngineErrorCategory::InvalidInput,
        false,
    )
}

fn join_unauthorized_error() -> EngineError {
    EngineError::new(
        JOIN_SPACE_UNAUTHORIZED_CODE,
        EngineErrorCategory::Unauthorized,
        false,
    )
}

fn join_not_found_error() -> EngineError {
    EngineError::new(
        JOIN_SPACE_NOT_FOUND_CODE,
        EngineErrorCategory::NotFound,
        false,
    )
}

fn join_conflict_error() -> EngineError {
    EngineError::new(
        JOIN_SPACE_CONFLICT_CODE,
        EngineErrorCategory::Conflict,
        false,
    )
}

fn join_unavailable_error() -> EngineError {
    EngineError::new(
        JOIN_SPACE_UNAVAILABLE_CODE,
        EngineErrorCategory::Unavailable,
        true,
    )
}

fn join_deadline_error() -> EngineError {
    EngineError::new(
        JOIN_SPACE_DEADLINE_CODE,
        EngineErrorCategory::DeadlineExceeded,
        true,
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
    use uc_application::facade::{SearchPageView, SearchResultView};

    use super::*;

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
}
