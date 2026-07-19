use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use tracing::error;
use uc_application::facade::{
    AppFacade, InMemoryLifecycleStatus, InitializeSpaceError,
    InitializeSpaceInput as AppInitializeSpaceInput, UnlockSpaceError,
    UnlockSpaceInput as AppUnlockSpaceInput,
};
use uc_core::ports::ReachabilityState;
use uc_core::TaskRegistry;

use crate::engine::EngineRuntime;
use crate::event_stream::EventSender;
use crate::internal::blob_tasks::{spawn_blob_processing_tasks, BlobProcessingPorts};
use crate::internal::deps::WiredDependencies;
use crate::internal::facade::{
    build_app_facade_from_deps, AppFacadeAssemblyOptions, ClipboardRestoreAssembly,
};
use crate::internal::host_adapters::{
    wire_host_capabilities_with_emitter, EngineHostEventEmitter, HostWiring,
};
use crate::internal::lifecycle::build_daemon_lifecycle;
use crate::internal::sync_engine::SyncEngineAssembly;
use crate::{
    DeviceSummary, EngineConfig, EngineError, EngineErrorCategory, HostCapabilities,
    HostFileAccess, Operation, OperationResult,
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

pub(crate) struct ProductionRuntime {
    wired: WiredDependencies,
    paths: uc_application::facade::AppPaths,
    session: Mutex<Option<ProductionSession>>,
    task_registry: Arc<TaskRegistry>,
    _temporary_dir: std::path::PathBuf,
    _files: Box<dyn HostFileAccess>,
}

struct ProductionSession {
    facade: Arc<AppFacade>,
    sync_engine: SyncEngineAssembly,
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

        let session = Self::build_session(&wired, &paths).await?;
        let task_registry = Arc::new(TaskRegistry::new());
        let blob_ports = BlobProcessingPorts::from_app_deps(&wired.deps);
        spawn_blob_processing_tasks(background, blob_ports, &task_registry).await;

        Ok(Self {
            wired,
            paths,
            session: Mutex::new(Some(session)),
            task_registry,
            _temporary_dir: temporary_dir,
            _files: files,
        })
    }

    async fn build_session(
        wired: &WiredDependencies,
        paths: &uc_application::facade::AppPaths,
    ) -> Result<ProductionSession, EngineError> {
        let lifecycle = build_daemon_lifecycle(&wired.deps, &wired.sync_engine, &wired.shared)
            .await
            .map_err(|error| startup_error("p2p session", error))?;
        let mut sync_engine = lifecycle.sync_engine_assembly;
        let (restore_tx, restore_rx) = tokio::sync::mpsc::unbounded_channel();
        sync_engine.attach_restore_broadcast(restore_rx);
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
                ..Default::default()
            },
        );

        Ok(ProductionSession {
            facade,
            sync_engine,
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
                self.current_facade()
                    .await?
                    .unlock_space(AppUnlockSpaceInput {
                        passphrase: input.passphrase.expose().to_owned(),
                    })
                    .await
                    .map_err(map_unlock_space_error)?;
                Ok(OperationResult::SpaceUnlocked)
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
            _ => Err(operation_unavailable_error()),
        }
    }

    async fn suspend(&self) -> Result<(), EngineError> {
        let session = self.session.lock().await.take();
        if let Some(session) = session {
            session.sync_engine.shutdown().await;
        }
        Ok(())
    }

    async fn resume(&self) -> Result<(), EngineError> {
        let session = Self::build_session(&self.wired, &self.paths).await?;
        *self.session.lock().await = Some(session);
        Ok(())
    }

    async fn shutdown(&self, deadline: Duration) -> Result<(), EngineError> {
        self.suspend().await?;
        self.task_registry.shutdown(deadline).await;
        Ok(())
    }
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

fn operation_error_with_code(
    code: u32,
    context: &'static str,
    error: impl std::fmt::Display,
) -> EngineError {
    error!(context, error = %error, "engine operation failed");
    EngineError::new(code, EngineErrorCategory::Internal, false)
}
