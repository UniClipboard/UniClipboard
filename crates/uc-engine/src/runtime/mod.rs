mod dispatch;
mod host_clipboard;
pub(crate) mod host_file;
mod host_operations;
#[cfg(feature = "lan-compat")]
mod mobile_upload;

#[cfg(feature = "lan-compat")]
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Mutex;
use tracing::error;
use uc_application::clipboard_write::LocalActiveRegisterAdvancer;
#[cfg(feature = "lan-compat")]
use uc_application::facade::FileTransferFacade;
use uc_application::facade::{AppFacade, InMemoryLifecycleStatus};
use uc_core::ports::ClockPort;
use uc_core::TaskRegistry;

use crate::assembly::blob_tasks::{spawn_blob_processing_tasks, BlobProcessingPorts};
use crate::assembly::clipboard_runtime::{
    build_clipboard_runtime, spawn_clipboard_runtime_tasks, ClipboardRuntime,
};
use crate::assembly::deps::WiredDependencies;
#[cfg(feature = "lan-compat")]
use crate::assembly::facade::build_mobile_sync_facade;
use crate::assembly::facade::{
    build_app_facade_from_deps, AppFacadeAssemblyOptions, ClipboardRestoreAssembly,
};
use crate::assembly::file_transfer::FileTransferLifecycle;
use crate::assembly::host::{
    wire_host_capabilities_with_emitter, EngineHostEventEmitter, HostWiring,
};
use crate::assembly::lifecycle::build_daemon_lifecycle;
#[cfg(feature = "lan-compat")]
use crate::assembly::mobile_lan::MobileLanEndpointUpdater;
use crate::assembly::search::build_search_coordinator;
use crate::assembly::sync_engine::SyncEngineAssembly;
use crate::engine::event_stream::EventSender;
use crate::subsystems::history_maintenance::spawn_history_maintenance_task;
use crate::subsystems::peer_keepalive::spawn_peer_keepalive_task;
use crate::{EngineConfig, EngineError, EngineErrorCategory, HostCapabilities, HostFileAccess};
use host_clipboard::{spawn_host_clipboard_change_task, HostClipboardChangeRuntime};
#[cfg(feature = "lan-compat")]
use mobile_upload::ActiveMobileFileUploadState;

const START_FAILED_CODE: u32 = 1101;
const OPERATION_UNAVAILABLE_CODE: u32 = 1103;

pub(crate) struct ProductionRuntime {
    app_version: String,
    session_factory: SessionFactory,
    session: Arc<Mutex<Option<ProductionSession>>>,
    task_registry: Arc<TaskRegistry>,
    file_transfer_lifecycle: Arc<FileTransferLifecycle>,
    #[cfg(feature = "lan-compat")]
    file_transfer_facade: Arc<FileTransferFacade>,
    #[cfg(feature = "lan-compat")]
    mobile_lan_endpoint: MobileLanEndpointUpdater,
    clock: Arc<dyn ClockPort>,
    file_cache_dir: PathBuf,
    temporary_dir: std::path::PathBuf,
    clipboard_import_root: std::path::PathBuf,
    files: Arc<dyn HostFileAccess>,
    #[cfg(feature = "lan-compat")]
    events: EventSender,
    #[cfg(feature = "lan-compat")]
    mobile_file_uploads: Mutex<HashMap<String, ActiveMobileFileUploadState>>,
}

struct SessionFactory {
    wired: WiredDependencies,
    paths: uc_application::facade::AppPaths,
    file_transfer_lifecycle: Arc<FileTransferLifecycle>,
    events: EventSender,
}

struct ProductionSession {
    facade: Arc<AppFacade>,
    #[cfg(feature = "lan-compat")]
    mobile_sync: Arc<uc_application::facade::MobileSyncFacade>,
    clipboard: ClipboardRuntime,
    sync_engine: SyncEngineAssembly,
    tasks: Arc<TaskRegistry>,
}

fn engine_event_for_active_clipboard(
    state: &uc_core::clipboard::ActiveClipboardState,
) -> crate::EngineEvent {
    crate::EngineEvent::ActiveClipboardChanged(crate::ActiveClipboardChanged {
        snapshot_hash: state.snapshot_hash.clone(),
        entry_id: state.entry_id.as_str().to_string(),
        activated_at_ms: state.activated_at_ms,
        activated_by: state.activated_by.as_str().to_string(),
    })
}

#[cfg(feature = "lan-compat")]
fn engine_event_for_mobile_settings_update(
    settings: &crate::MobileSyncSettingsUpdateSummary,
) -> crate::EngineEvent {
    crate::EngineEvent::MobileLanSettingsChanged(crate::MobileLanSettingsChanged {
        enabled: settings.enabled,
        lan_listen_enabled: settings.lan_listen_enabled,
        lan_port: settings.lan_port,
    })
}

impl ProductionRuntime {
    pub(crate) async fn start(
        config: EngineConfig,
        host: HostCapabilities,
        events: EventSender,
    ) -> Result<Self, EngineError> {
        let app_version = config.app_version().to_string();
        let emitter = Arc::new(EngineHostEventEmitter::new(events.clone()));
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
        let session = Self::build_session(&SessionFactory {
            wired: wired.clone(),
            paths: paths.clone(),
            file_transfer_lifecycle: Arc::clone(&file_transfer_lifecycle),
            events: events.clone(),
        })
        .await?;
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

        #[cfg(feature = "lan-compat")]
        let file_transfer_facade = Arc::clone(&wired.shared.file_transfer_facade);
        #[cfg(feature = "lan-compat")]
        let mobile_lan_endpoint = MobileLanEndpointUpdater::new(Arc::clone(
            &wired.daemon_runtime.mobile_sync_endpoint_info,
        ));
        let clock = Arc::clone(&wired.deps.system.clock);
        let file_cache_dir = paths.file_cache_dir.clone();
        let session_factory = SessionFactory {
            wired,
            paths,
            file_transfer_lifecycle: Arc::clone(&file_transfer_lifecycle),
            events: events.clone(),
        };

        Ok(Self {
            app_version,
            session_factory,
            session,
            task_registry,
            file_transfer_lifecycle,
            #[cfg(feature = "lan-compat")]
            file_transfer_facade,
            #[cfg(feature = "lan-compat")]
            mobile_lan_endpoint,
            clock,
            file_cache_dir,
            temporary_dir,
            clipboard_import_root,
            files,
            #[cfg(feature = "lan-compat")]
            events,
            #[cfg(feature = "lan-compat")]
            mobile_file_uploads: Mutex::new(HashMap::new()),
        })
    }

    async fn build_session(factory: &SessionFactory) -> Result<ProductionSession, EngineError> {
        let wired = &factory.wired;
        let paths = &factory.paths;
        let file_transfer_lifecycle = &factory.file_transfer_lifecycle;
        let events = factory.events.clone();
        let lifecycle = build_daemon_lifecycle(&wired.deps, &wired.sync_engine, &wired.shared)
            .await
            .map_err(|error| startup_error("p2p session", error))?;
        let mut sync_engine = lifecycle.sync_engine_assembly;
        let (restore_tx, restore_rx) = tokio::sync::mpsc::unbounded_channel();
        sync_engine.attach_restore_broadcast(restore_rx);
        let search_coordinator = build_search_coordinator(&wired.deps);
        let clipboard = build_clipboard_runtime(wired, &sync_engine);
        #[cfg(feature = "lan-compat")]
        let mobile_sync = build_mobile_sync_facade(
            &wired.deps,
            paths,
            Arc::clone(&clipboard.apply_inbound),
            Some(Arc::clone(&wired.shared.file_transfer_facade)),
            None,
            Some(Arc::clone(&clipboard.outbound)),
            Some(Arc::clone(&sync_engine.active_clipboard)),
        );
        let tasks = Arc::new(TaskRegistry::new());
        let mut active_clipboard_changes = wired.shared.active_clipboard_sse_source.subscribe();
        let active_clipboard_events = events.clone();
        tasks
            .spawn("active_clipboard_events", move |cancel| async move {
                loop {
                    tokio::select! {
                        _ = cancel.cancelled() => return,
                        change = active_clipboard_changes.recv() => match change {
                            Ok(state) => active_clipboard_events
                                .send(engine_event_for_active_clipboard(&state)),
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                                active_clipboard_events.send(crate::EngineEvent::RefreshRequired {
                                    reason: crate::RefreshReason::ConsumerLagged,
                                });
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
                        }
                    }
                }
            })
            .await;
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
        spawn_peer_keepalive_task(Arc::clone(&facade), &tasks, events.clone()).await;
        spawn_clipboard_runtime_tasks(
            &clipboard,
            Arc::clone(&sync_engine.clipboard_sync),
            &tasks,
            events,
        )
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
            #[cfg(feature = "lan-compat")]
            mobile_sync,
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

    #[cfg(feature = "lan-compat")]
    async fn current_mobile_sync(
        &self,
    ) -> Result<Arc<uc_application::facade::MobileSyncFacade>, EngineError> {
        self.session
            .lock()
            .await
            .as_ref()
            .map(|session| Arc::clone(&session.mobile_sync))
            .ok_or_else(operation_unavailable_error)
    }
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
        ClipboardOutboundOutcome, SearchFacadeError, SearchPageView, SearchResultView,
        StorageFacadeError, StorageStatsView,
    };
    use uc_core::ids::DeviceId;
    #[cfg(feature = "lan-compat")]
    use uc_core::mobile_sync::StagingHandle;

    use super::*;
    use crate::error_codes::{CLEAR_STORAGE_CACHE_FAILED_CODE, QUERY_STORAGE_STATS_FAILED_CODE};
    use crate::operations::history::search::{
        history_page_result, history_search_input, map_query_history_error,
    };
    use crate::operations::settings::storage::{map_storage_error, storage_stats_result};
    use crate::runtime::host_operations::send_report_result;
    #[cfg(feature = "lan-compat")]
    use crate::runtime::mobile_upload::new_mobile_file_upload_handle;
    use crate::{EntrySummary, OperationResult, QueryHistoryInput, StorageStatsSummary};

    #[test]
    fn active_clipboard_event_preserves_mobile_sse_identity() {
        let state = uc_core::clipboard::ActiveClipboardState::new(
            "hash-1",
            uc_core::ids::EntryId::from("entry-1"),
            42,
            DeviceId::new("device-1"),
        );

        assert_eq!(
            engine_event_for_active_clipboard(&state),
            crate::EngineEvent::ActiveClipboardChanged(crate::ActiveClipboardChanged {
                snapshot_hash: "hash-1".into(),
                entry_id: "entry-1".into(),
                activated_at_ms: 42,
                activated_by: "device-1".into(),
            })
        );
    }

    #[cfg(feature = "lan-compat")]
    #[test]
    fn mobile_settings_event_preserves_listener_target() {
        let settings = crate::MobileSyncSettingsUpdateSummary {
            enabled: true,
            lan_listen_enabled: true,
            lan_advertise_ip: None,
            lan_advertise_base_url: None,
            lan_port: Some(51234),
            changed: true,
        };

        assert_eq!(
            engine_event_for_mobile_settings_update(&settings),
            crate::EngineEvent::MobileLanSettingsChanged(crate::MobileLanSettingsChanged {
                enabled: true,
                lan_listen_enabled: true,
                lan_port: Some(51234),
            })
        );
    }

    #[cfg(feature = "lan-compat")]
    #[test]
    fn mobile_upload_handle_is_owned_by_engine_instead_of_exposing_staging_token() {
        let staging = StagingHandle::new();
        let handle = new_mobile_file_upload_handle();

        assert_ne!(handle.as_str(), staging.to_string());
        assert!(handle.as_str().starts_with("uc-mobile-upload-v1:"));
    }

    #[cfg(feature = "lan-compat")]
    #[test]
    fn mobile_upload_handles_are_unique() {
        let first = new_mobile_file_upload_handle();
        let second = new_mobile_file_upload_handle();

        assert_ne!(first, second);
    }

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
