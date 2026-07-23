use std::collections::HashMap;
use std::ffi::{c_char, CStr, CString};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::ptr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use base64::Engine as _;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::{mpsc, oneshot};
use uc_engine::{
    CreateSpaceInput, Engine, EngineConfig, EngineError, EngineEvent, ExportEntryInput,
    HostCapabilities, HostCapabilityError, HostCapabilityErrorCategory, HostClipboard,
    HostClipboardSnapshot, HostDirectories, HostFileAccess, HostFileHandle, HostFileMetadata,
    HostSecureStorage, JoinSpaceInput, Operation, OperationResult, QueryHistoryInput,
    ResendEntryInput, SecretString, SendFilesInput, SendImageInput, SendTextInput,
    UnlockSpaceInput,
};

#[cfg(target_os = "android")]
mod android;

#[cfg(target_vendor = "apple")]
const KEYCHAIN_SERVICE: &str = "app.uniclipboard.engine-probe";
#[cfg(target_vendor = "apple")]
const ITEM_NOT_FOUND_STATUS: i32 = -25300;

#[derive(Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
enum ProbeCommand {
    Start {
        private_data: String,
        cache: String,
        temporary: String,
        app_version: String,
    },
    CreateSpace {
        device_name: String,
        passphrase: String,
    },
    UnlockSpace {
        passphrase: String,
    },
    JoinSpace {
        invitation_code: String,
        device_name: String,
        passphrase: String,
    },
    IssueInvitation,
    ListDevices,
    SendText {
        text: String,
    },
    SendImage {
        bytes_base64: String,
        mime_type: String,
    },
    SendFile {
        path: String,
        display_name: String,
        mime_type: Option<String>,
    },
    QueryHistory {
        query: Option<String>,
        limit: u32,
    },
    ExportEntry {
        entry_id: String,
        path: String,
    },
    ResendEntry {
        entry_id: String,
    },
    Suspend,
    Resume,
    EventSummary,
    Shutdown,
}

struct ProbeRequest {
    command: ProbeCommand,
    response: oneshot::Sender<Value>,
}

struct ProbeClient {
    requests: mpsc::UnboundedSender<ProbeRequest>,
}

impl ProbeClient {
    fn new() -> Result<Self, String> {
        let (requests, receiver) = mpsc::unbounded_channel();
        std::thread::Builder::new()
            .name("uc-mobile-probe-runtime".into())
            .spawn(move || {
                let _ = tracing_subscriber::fmt()
                    .with_ansi(false)
                    .without_time()
                    .with_env_filter(
                        tracing_subscriber::EnvFilter::try_from_default_env()
                            .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("error")),
                    )
                    .try_init();
                let runtime = match tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(2)
                    .enable_all()
                    .build()
                {
                    Ok(runtime) => runtime,
                    Err(_) => return,
                };
                runtime.block_on(run_probe(receiver));
            })
            .map_err(|error| error.to_string())?;
        Ok(Self { requests })
    }

    fn execute(&self, command: ProbeCommand) -> Value {
        let (response, receiver) = oneshot::channel();
        if self
            .requests
            .send(ProbeRequest { command, response })
            .is_err()
        {
            return probe_error("runtime_unavailable");
        }
        match receiver.blocking_recv() {
            Ok(value) => value,
            Err(_) => probe_error("runtime_unavailable"),
        }
    }
}

#[derive(Default)]
struct EventSummary {
    incoming_entries: u64,
    transfer_updates: u64,
    refresh_requests: u64,
    completed_operations: u64,
    lifecycle_failures: u64,
    fatal_errors: u64,
    last_state: Option<String>,
}

#[derive(Clone)]
struct RegisteredFile {
    path: PathBuf,
    display_name: String,
    mime_type: Option<String>,
}

#[derive(Clone, Default)]
struct ProbeFiles {
    next_handle: Arc<AtomicU64>,
    files: Arc<Mutex<HashMap<String, RegisteredFile>>>,
}

impl ProbeFiles {
    fn register(
        &self,
        path: PathBuf,
        display_name: String,
        mime_type: Option<String>,
    ) -> HostFileHandle {
        let handle = format!(
            "probe-file-{}",
            self.next_handle.fetch_add(1, Ordering::Relaxed)
        );
        lock_unpoisoned(&self.files).insert(
            handle.clone(),
            RegisteredFile {
                path,
                display_name,
                mime_type,
            },
        );
        HostFileHandle::new(handle)
    }

    fn lookup(&self, handle: &HostFileHandle) -> Result<RegisteredFile, HostCapabilityError> {
        lock_unpoisoned(&self.files)
            .get(handle.as_str())
            .cloned()
            .ok_or_else(|| host_error(HostCapabilityErrorCategory::InvalidHandle))
    }
}

impl HostFileAccess for ProbeFiles {
    fn metadata(&self, handle: &HostFileHandle) -> Result<HostFileMetadata, HostCapabilityError> {
        let file = self.lookup(handle)?;
        let metadata = std::fs::metadata(&file.path)
            .map_err(|_| host_error(HostCapabilityErrorCategory::Io))?;
        Ok(HostFileMetadata {
            display_name: file.display_name,
            size_bytes: metadata.len(),
            mime_type: file.mime_type,
        })
    }

    fn read_chunk(
        &self,
        handle: &HostFileHandle,
        offset: u64,
        max_bytes: u32,
    ) -> Result<Vec<u8>, HostCapabilityError> {
        let file = self.lookup(handle)?;
        let mut input =
            File::open(file.path).map_err(|_| host_error(HostCapabilityErrorCategory::Io))?;
        input
            .seek(SeekFrom::Start(offset))
            .map_err(|_| host_error(HostCapabilityErrorCategory::Io))?;
        let mut bytes = vec![0; max_bytes as usize];
        let read = input
            .read(&mut bytes)
            .map_err(|_| host_error(HostCapabilityErrorCategory::Io))?;
        bytes.truncate(read);
        Ok(bytes)
    }

    fn write_chunk(
        &self,
        handle: &HostFileHandle,
        offset: u64,
        bytes: &[u8],
    ) -> Result<(), HostCapabilityError> {
        let file = self.lookup(handle)?;
        let mut output = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(file.path)
            .map_err(|_| host_error(HostCapabilityErrorCategory::Io))?;
        output
            .seek(SeekFrom::Start(offset))
            .and_then(|_| output.write_all(bytes))
            .map_err(|_| host_error(HostCapabilityErrorCategory::Io))
    }

    fn finish_write(&self, handle: &HostFileHandle) -> Result<(), HostCapabilityError> {
        let file = self.lookup(handle)?;
        OpenOptions::new()
            .write(true)
            .open(file.path)
            .and_then(|output| output.sync_all())
            .map_err(|_| host_error(HostCapabilityErrorCategory::Io))
    }
}

struct ProbeClipboard;

impl HostClipboard for ProbeClipboard {
    fn read(&self) -> Result<HostClipboardSnapshot, HostCapabilityError> {
        Ok(HostClipboardSnapshot {
            observed_at_ms: 0,
            representations: Vec::new(),
        })
    }

    fn write(&self, _snapshot: HostClipboardSnapshot) -> Result<(), HostCapabilityError> {
        Ok(())
    }
}

#[cfg(target_vendor = "apple")]
struct KeychainStorage;

#[cfg(target_vendor = "apple")]
impl HostSecureStorage for KeychainStorage {
    fn get(&self, key: &str) -> Result<Option<Vec<u8>>, HostCapabilityError> {
        match security_framework::passwords::get_generic_password(KEYCHAIN_SERVICE, key) {
            Ok(value) => Ok(Some(value)),
            Err(error) if error.code() == ITEM_NOT_FOUND_STATUS => Ok(None),
            Err(_) => Err(host_error(HostCapabilityErrorCategory::Unavailable)),
        }
    }

    fn set(&self, key: &str, value: &[u8]) -> Result<(), HostCapabilityError> {
        security_framework::passwords::set_generic_password(KEYCHAIN_SERVICE, key, value)
            .map_err(|_| host_error(HostCapabilityErrorCategory::Unavailable))
    }

    fn delete(&self, key: &str) -> Result<(), HostCapabilityError> {
        match security_framework::passwords::delete_generic_password(KEYCHAIN_SERVICE, key) {
            Ok(()) => Ok(()),
            Err(error) if error.code() == ITEM_NOT_FOUND_STATUS => Ok(()),
            Err(_) => Err(host_error(HostCapabilityErrorCategory::Unavailable)),
        }
    }
}

#[cfg(not(any(target_vendor = "apple", target_os = "android")))]
struct UnavailableSecureStorage;

#[cfg(not(any(target_vendor = "apple", target_os = "android")))]
impl HostSecureStorage for UnavailableSecureStorage {
    fn get(&self, _key: &str) -> Result<Option<Vec<u8>>, HostCapabilityError> {
        Err(host_error(HostCapabilityErrorCategory::Unavailable))
    }

    fn set(&self, _key: &str, _value: &[u8]) -> Result<(), HostCapabilityError> {
        Err(host_error(HostCapabilityErrorCategory::Unavailable))
    }

    fn delete(&self, _key: &str) -> Result<(), HostCapabilityError> {
        Err(host_error(HostCapabilityErrorCategory::Unavailable))
    }
}

struct ProbeState {
    engine: Option<Arc<Engine>>,
    files: ProbeFiles,
    events: Arc<Mutex<EventSummary>>,
}

async fn run_probe(mut requests: mpsc::UnboundedReceiver<ProbeRequest>) {
    let mut state = ProbeState {
        engine: None,
        files: ProbeFiles::default(),
        events: Arc::new(Mutex::new(EventSummary::default())),
    };
    while let Some(request) = requests.recv().await {
        let response = execute_command(&mut state, request.command).await;
        let _ = request.response.send(response);
    }
}

async fn execute_command(state: &mut ProbeState, command: ProbeCommand) -> Value {
    match command {
        ProbeCommand::Start {
            private_data,
            cache,
            temporary,
            app_version,
        } => {
            if state.engine.is_some() {
                return probe_error("already_started");
            }
            let directories = [
                PathBuf::from(&private_data),
                PathBuf::from(&cache),
                PathBuf::from(&temporary),
            ];
            for directory in &directories {
                if std::fs::create_dir_all(directory).is_err() {
                    return probe_error("directory_unavailable");
                }
            }
            let host = HostCapabilities::new(
                HostDirectories::new(
                    directories[0].clone(),
                    directories[1].clone(),
                    directories[2].clone(),
                ),
                host_secure_storage(),
                Box::new(ProbeClipboard),
                Box::new(state.files.clone()),
            );
            match Engine::start(EngineConfig::new(app_version), host).await {
                Ok((engine, mut stream)) => {
                    let events = Arc::clone(&state.events);
                    tokio::spawn(async move {
                        while let Some(event) = stream.next().await {
                            record_event(&events, event);
                        }
                    });
                    state.engine = Some(Arc::new(engine));
                    json!({"ok": true, "kind": "started"})
                }
                Err(error) => engine_error(error),
            }
        }
        ProbeCommand::CreateSpace {
            device_name,
            passphrase,
        } => {
            execute_operation(
                state,
                Operation::CreateSpace(CreateSpaceInput {
                    device_name: Some(device_name),
                    passphrase: SecretString::new(passphrase.clone()),
                    passphrase_confirmation: SecretString::new(passphrase),
                }),
            )
            .await
        }
        ProbeCommand::UnlockSpace { passphrase } => {
            execute_operation(
                state,
                Operation::UnlockSpace(UnlockSpaceInput {
                    passphrase: SecretString::new(passphrase),
                }),
            )
            .await
        }
        ProbeCommand::JoinSpace {
            invitation_code,
            device_name,
            passphrase,
        } => {
            execute_operation(
                state,
                Operation::JoinSpace(JoinSpaceInput {
                    invitation_code,
                    device_name: Some(device_name),
                    passphrase: SecretString::new(passphrase),
                }),
            )
            .await
        }
        ProbeCommand::IssueInvitation => execute_operation(state, Operation::IssueInvitation).await,
        ProbeCommand::ListDevices => execute_operation(state, Operation::ListDevices).await,
        ProbeCommand::SendText { text } => {
            execute_operation(
                state,
                Operation::SendText(SendTextInput {
                    text,
                    target_devices: Vec::new(),
                }),
            )
            .await
        }
        ProbeCommand::SendImage {
            bytes_base64,
            mime_type,
        } => match base64::engine::general_purpose::STANDARD.decode(bytes_base64) {
            Ok(bytes) => {
                execute_operation(
                    state,
                    Operation::SendImage(SendImageInput {
                        bytes,
                        mime_type,
                        target_devices: Vec::new(),
                    }),
                )
                .await
            }
            Err(_) => probe_error("invalid_image"),
        },
        ProbeCommand::SendFile {
            path,
            display_name,
            mime_type,
        } => {
            let handle = state
                .files
                .register(PathBuf::from(path), display_name, mime_type);
            execute_operation(
                state,
                Operation::SendFiles(SendFilesInput {
                    files: vec![handle],
                    target_devices: Vec::new(),
                }),
            )
            .await
        }
        ProbeCommand::QueryHistory { query, limit } => {
            execute_operation(
                state,
                Operation::QueryHistory(QueryHistoryInput {
                    cursor: None,
                    limit,
                    query,
                }),
            )
            .await
        }
        ProbeCommand::ExportEntry { entry_id, path } => {
            let display_name = PathBuf::from(&path)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("export.bin")
                .to_owned();
            let handle = state
                .files
                .register(PathBuf::from(path), display_name, None);
            execute_operation(
                state,
                Operation::ExportEntry(ExportEntryInput {
                    entry_id,
                    destination: handle,
                }),
            )
            .await
        }
        ProbeCommand::ResendEntry { entry_id } => {
            execute_operation(
                state,
                Operation::ResendEntry(ResendEntryInput {
                    entry_id,
                    target_devices: Vec::new(),
                }),
            )
            .await
        }
        ProbeCommand::Suspend => match state.engine.as_ref() {
            Some(engine) => lifecycle_response(engine.suspend().await, "suspended"),
            None => probe_error("not_started"),
        },
        ProbeCommand::Resume => match state.engine.as_ref() {
            Some(engine) => lifecycle_response(engine.resume().await, "resumed"),
            None => probe_error("not_started"),
        },
        ProbeCommand::EventSummary => {
            let events = lock_unpoisoned(&state.events);
            json!({
                "ok": true,
                "kind": "event_summary",
                "incoming_entries": events.incoming_entries,
                "transfer_updates": events.transfer_updates,
                "refresh_requests": events.refresh_requests,
                "completed_operations": events.completed_operations,
                "lifecycle_failures": events.lifecycle_failures,
                "fatal_errors": events.fatal_errors,
                "last_state": events.last_state,
            })
        }
        ProbeCommand::Shutdown => match state.engine.take() {
            Some(engine) => {
                lifecycle_response(engine.shutdown(Duration::from_secs(15)).await, "shutdown")
            }
            None => probe_error("not_started"),
        },
    }
}

async fn execute_operation(state: &ProbeState, operation: Operation) -> Value {
    match state.engine.as_ref() {
        Some(engine) => match engine.execute(operation).await {
            Ok(result) => operation_response(result),
            Err(error) => engine_error(error),
        },
        None => probe_error("not_started"),
    }
}

fn operation_response(result: OperationResult) -> Value {
    match result {
        OperationResult::SpaceCreated { space_id, .. } => {
            json!({"ok": true, "kind": "space_created", "space_id": space_id})
        }
        OperationResult::SpaceJoined { space_id, .. } => {
            json!({"ok": true, "kind": "space_joined", "space_id": space_id})
        }
        OperationResult::SpaceUnlocked { space_id } => {
            json!({"ok": true, "kind": "space_unlocked", "space_id": space_id})
        }
        OperationResult::SessionRecovered { unlocked, resumed } => json!({
            "ok": true,
            "kind": "session_recovered",
            "unlocked": unlocked,
            "resumed": resumed,
        }),
        OperationResult::InvitationIssued {
            invitation_code, ..
        } => json!({
            "ok": true,
            "kind": "invitation_issued",
            "invitation_code": invitation_code,
        }),
        OperationResult::InvitationCancelled => {
            json!({"ok": true, "kind": "invitation_cancelled"})
        }
        OperationResult::SpaceReset => json!({"ok": true, "kind": "space_reset"}),
        OperationResult::SpaceFactoryReset => {
            json!({"ok": true, "kind": "space_factory_reset"})
        }
        OperationResult::SetupState(state) => json!({
            "ok": true,
            "kind": "setup_state",
            "has_completed": state.has_completed,
            "has_current_invitation": state.current_invitation.is_some(),
            "has_device_name": state.device_name.is_some(),
        }),
        OperationResult::MigrationProgress(progress) => json!({
            "ok": true,
            "kind": "migration_progress",
            "has_phase": progress.phase.is_some(),
            "backup_record_count": progress.backup_record_count,
        }),
        OperationResult::StorageStats(stats) => json!({
            "ok": true,
            "kind": "storage_stats",
            "total_bytes": stats.total_bytes,
            "database_bytes": stats.database_bytes,
            "vault_bytes": stats.vault_bytes,
            "cache_bytes": stats.cache_bytes,
            "logs_bytes": stats.logs_bytes,
        }),
        OperationResult::StorageCacheCleared { freed_bytes } => json!({
            "ok": true,
            "kind": "storage_cache_cleared",
            "freed_bytes": freed_bytes,
        }),
        OperationResult::LocalDevice(device) => json!({
            "ok": true,
            "kind": "local_device",
            "device_id": device.device_id,
            "has_display_name": !device.display_name.is_empty(),
        }),
        OperationResult::PeerConnections(peers) => {
            let channels = peers
                .iter()
                .map(|peer| match peer.channel {
                    uc_engine::PeerConnectionChannelSummary::Direct => "direct",
                    uc_engine::PeerConnectionChannelSummary::Relay => "relay",
                    uc_engine::PeerConnectionChannelSummary::Offline => "offline",
                    uc_engine::PeerConnectionChannelSummary::Unknown => "unknown",
                })
                .collect::<Vec<_>>();
            json!({
                "ok": true,
                "kind": "peer_connections",
                "count": peers.len(),
                "connected_count": peers.iter().filter(|peer| peer.connected).count(),
                "channels": channels,
            })
        }
        OperationResult::PeerConnectionsRefreshed(report) => json!({
            "ok": true,
            "kind": "peer_connections_refreshed",
            "total": report.total,
            "online": report.online,
            "offline": report.offline,
            "errors": report.errors,
        }),
        OperationResult::Settings(settings) => json!({
            "ok": true,
            "kind": "settings",
            "schema_version": settings.schema_version,
            "auto_sync": settings.sync.auto_sync,
            "retention_enabled": settings.retention_policy.enabled,
            "retention_rule_count": settings.retention_policy.rules.len(),
            "shortcut_count": settings.keyboard_shortcuts.len(),
            "custom_relay_count": settings.network.custom_relay_urls.len(),
            "has_auto_save_dir": settings.file_sync.auto_save_dir.is_some(),
        }),
        OperationResult::SettingsUpdated(outcome) => match outcome {
            uc_engine::SettingsUpdateOutcome::Updated(_) => json!({
                "ok": true,
                "kind": "settings_updated",
                "outcome": "updated",
            }),
            uc_engine::SettingsUpdateOutcome::Rejected { .. } => json!({
                "ok": true,
                "kind": "settings_updated",
                "outcome": "rejected",
            }),
        },
        OperationResult::RelayProbed(outcome) => {
            let (outcome, latency_ms) = match outcome {
                uc_engine::RelayProbeOutcome::Success { latency_ms } => {
                    ("success", Some(latency_ms))
                }
                uc_engine::RelayProbeOutcome::InvalidUrl { .. } => ("invalid_url", None),
                uc_engine::RelayProbeOutcome::Dns { .. } => ("dns", None),
                uc_engine::RelayProbeOutcome::Tls { .. } => ("tls", None),
                uc_engine::RelayProbeOutcome::Handshake { .. } => ("handshake", None),
                uc_engine::RelayProbeOutcome::Timeout => ("timeout", None),
                uc_engine::RelayProbeOutcome::Other { .. } => ("other", None),
            };
            json!({
                "ok": true,
                "kind": "relay_probed",
                "outcome": outcome,
                "latency_ms": latency_ms,
            })
        }
        OperationResult::UpgradeStatus(status) => {
            let (outcome, from, to) = match status {
                uc_engine::UpgradeStatusSummary::FreshInstall { current } => {
                    ("fresh_install", None, current)
                }
                uc_engine::UpgradeStatusSummary::NoChange { current } => {
                    ("no_change", None, current)
                }
                uc_engine::UpgradeStatusSummary::Upgraded { from, to } => ("upgraded", from, to),
                uc_engine::UpgradeStatusSummary::Downgraded { from, to } => {
                    ("downgraded", Some(from), to)
                }
            };
            json!({
                "ok": true,
                "kind": "upgrade_status",
                "outcome": outcome,
                "from": from,
                "to": to,
            })
        }
        OperationResult::UpgradeAcknowledged { version } => json!({
            "ok": true,
            "kind": "upgrade_acknowledged",
            "version": version,
        }),
        OperationResult::DiagnosticsStatus(status) => json!({
            "ok": true,
            "kind": "diagnostics_status",
            "debug_mode": status.debug_mode,
            "effective_log_profile": status.effective_log_profile,
            "restart_required": status.restart_required,
        }),
        OperationResult::DebugModeUpdated(result) => json!({
            "ok": true,
            "kind": "debug_mode_updated",
            "debug_mode": result.debug_mode,
            "restart_required": result.restart_required,
        }),
        OperationResult::DiagnosticLogsExported(result) => json!({
            "ok": true,
            "kind": "diagnostic_logs_exported",
            "included_file_count": result.included_files.len(),
            "since_unix_ms": result.since_unix_ms,
        }),
        OperationResult::ConfigExport(outcome) => json!({
            "ok": true,
            "kind": "config_export",
            "outcome": match outcome {
                uc_engine::ConfigExportOutcome::Exported => "exported",
                uc_engine::ConfigExportOutcome::Locked => "locked",
                uc_engine::ConfigExportOutcome::NotInitialized => "not_initialized",
            },
        }),
        OperationResult::ConfigImportPreview(outcome) => match outcome {
            uc_engine::ConfigImportPreviewOutcome::Ready(preview) => json!({
                "ok": true,
                "kind": "config_import_preview",
                "outcome": "ready",
                "app_version": preview.app_version,
                "source_mode": match preview.source_mode {
                    uc_engine::ConfigSourceModeSummary::Portable => "portable",
                    uc_engine::ConfigSourceModeSummary::Installed => "installed",
                },
                "created_at_unix_ms": preview.created_at_unix_ms,
                "has_profile_id": !preview.profile_id.is_empty(),
                "has_device_fingerprint": !preview.device_fingerprint.is_empty(),
            }),
            uc_engine::ConfigImportPreviewOutcome::InvalidPasswordOrCorrupt => json!({
                "ok": true,
                "kind": "config_import_preview",
                "outcome": "invalid_password_or_corrupt",
            }),
            uc_engine::ConfigImportPreviewOutcome::Incompatible { .. } => json!({
                "ok": true,
                "kind": "config_import_preview",
                "outcome": "incompatible",
            }),
        },
        OperationResult::ConfigImportStaged(outcome) => match outcome {
            uc_engine::ConfigImportStageOutcome::Staged {
                unlock_required_after_apply,
            } => json!({
                "ok": true,
                "kind": "config_import_staged",
                "outcome": "staged",
                "unlock_required_after_apply": unlock_required_after_apply,
            }),
            uc_engine::ConfigImportStageOutcome::InvalidPasswordOrCorrupt => json!({
                "ok": true,
                "kind": "config_import_staged",
                "outcome": "invalid_password_or_corrupt",
            }),
            uc_engine::ConfigImportStageOutcome::Incompatible { .. } => json!({
                "ok": true,
                "kind": "config_import_staged",
                "outcome": "incompatible",
            }),
        },
        OperationResult::MobileDevices(devices) => json!({
            "ok": true,
            "kind": "mobile_devices",
            "count": devices.len(),
        }),
        OperationResult::MobileDeviceRevoked(outcome) => json!({
            "ok": true,
            "kind": "mobile_device_revoked",
            "outcome": match outcome {
                uc_engine::MobileDeviceRevokeOutcome::Revoked => "revoked",
                uc_engine::MobileDeviceRevokeOutcome::NotFound => "not_found",
            },
        }),
        OperationResult::MobileRequestAuthenticated(session) => json!({
            "ok": true,
            "kind": "mobile_request_authenticated",
            "client_type": match session.client_type {
                uc_engine::MobileClientTypeSummary::IosShortcut => "ios_shortcut",
            },
            "has_credential": true,
        }),
        OperationResult::MobileAuthentication(outcome) => json!({
            "ok": true,
            "kind": "mobile_authentication",
            "outcome": match outcome {
                uc_engine::MobileAuthenticationOutcome::Rejected => "rejected",
            },
        }),
        OperationResult::MobileCredentialCurrent { current } => json!({
            "ok": true,
            "kind": "mobile_credential_current",
            "current": current,
        }),
        OperationResult::MobileLanInterfaces(interfaces) => json!({
            "ok": true,
            "kind": "mobile_lan_interfaces",
            "count": interfaces.len(),
        }),
        OperationResult::MobileSyncSettings(settings) => json!({
            "ok": true,
            "kind": "mobile_sync_settings",
            "enabled": settings.enabled,
            "lan_listen_enabled": settings.lan_listen_enabled,
            "has_lan_advertise_ip": settings.lan_advertise_ip.is_some(),
            "has_lan_advertise_base_url": settings.lan_advertise_base_url.is_some(),
            "lan_port": settings.lan_port,
            "has_lan_listener_error": settings.lan_listener_error.is_some(),
            "shortcut_install_method_count": settings.shortcut_install_methods.len(),
        }),
        OperationResult::MobileSyncSettingsUpdated(outcome) => match outcome {
            uc_engine::MobileSyncSettingsUpdateOutcome::Updated(settings) => json!({
                "ok": true,
                "kind": "mobile_sync_settings_updated",
                "outcome": "updated",
                "enabled": settings.enabled,
                "lan_listen_enabled": settings.lan_listen_enabled,
                "has_lan_advertise_ip": settings.lan_advertise_ip.is_some(),
                "has_lan_advertise_base_url": settings.lan_advertise_base_url.is_some(),
                "lan_port": settings.lan_port,
                "changed": settings.changed,
            }),
            uc_engine::MobileSyncSettingsUpdateOutcome::Rejected { .. } => json!({
                "ok": true,
                "kind": "mobile_sync_settings_updated",
                "outcome": "rejected",
            }),
        },
        OperationResult::MobileLanEndpointUpdated => json!({
            "ok": true,
            "kind": "mobile_lan_endpoint_updated",
        }),
        OperationResult::MobileDeviceRegistered(outcome) => json!({
            "ok": true,
            "kind": "mobile_device_registered",
            "outcome": match outcome {
                uc_engine::MobileDeviceRegistrationOutcome::Registered(_) => "registered",
                uc_engine::MobileDeviceRegistrationOutcome::LabelEmpty => "label_empty",
                uc_engine::MobileDeviceRegistrationOutcome::LabelTooLong => "label_too_long",
                uc_engine::MobileDeviceRegistrationOutcome::LanListenerDisabled => "lan_listener_disabled",
                uc_engine::MobileDeviceRegistrationOutcome::UsernameTaken { .. } => "username_taken",
                uc_engine::MobileDeviceRegistrationOutcome::UsernameTooShort { .. } => "username_too_short",
                uc_engine::MobileDeviceRegistrationOutcome::UsernameTooLong { .. } => "username_too_long",
                uc_engine::MobileDeviceRegistrationOutcome::UsernameMustStartWithLetter => "username_must_start_with_letter",
                uc_engine::MobileDeviceRegistrationOutcome::UsernameContainsForbiddenChars => "username_contains_forbidden_chars",
                uc_engine::MobileDeviceRegistrationOutcome::PasswordTooShort { .. } => "password_too_short",
                uc_engine::MobileDeviceRegistrationOutcome::PasswordTooLong { .. } => "password_too_long",
                uc_engine::MobileDeviceRegistrationOutcome::NoLanInterfaceAvailable => "no_lan_interface_available",
            },
        }),
        OperationResult::MobileDeviceUpdated(outcome) => json!({
            "ok": true,
            "kind": "mobile_device_updated",
            "outcome": match outcome {
                uc_engine::MobileDeviceUpdateOutcome::Updated(_) => "updated",
                uc_engine::MobileDeviceUpdateOutcome::NotFound => "not_found",
                uc_engine::MobileDeviceUpdateOutcome::LabelEmpty => "label_empty",
                uc_engine::MobileDeviceUpdateOutcome::LabelTooLong => "label_too_long",
                uc_engine::MobileDeviceUpdateOutcome::UsernameTaken { .. } => "username_taken",
                uc_engine::MobileDeviceUpdateOutcome::UsernameTooShort { .. } => "username_too_short",
                uc_engine::MobileDeviceUpdateOutcome::UsernameTooLong { .. } => "username_too_long",
                uc_engine::MobileDeviceUpdateOutcome::UsernameMustStartWithLetter => "username_must_start_with_letter",
                uc_engine::MobileDeviceUpdateOutcome::UsernameContainsForbiddenChars => "username_contains_forbidden_chars",
                uc_engine::MobileDeviceUpdateOutcome::PasswordTooShort { .. } => "password_too_short",
                uc_engine::MobileDeviceUpdateOutcome::PasswordTooLong { .. } => "password_too_long",
            },
        }),
        OperationResult::MobileContentAvailability { available } => json!({
            "ok": true,
            "kind": "mobile_content_availability",
            "available": available,
        }),
        OperationResult::MobileSyncDocument(document) => match document {
            Some(document) => json!({
                "ok": true,
                "kind": "mobile_sync_document",
                "has_document": true,
                "item_type": mobile_sync_item_type(document.item_type),
                "text_bytes": document.text.len(),
                "has_data_name": document.data_name.is_some(),
                "has_data": document.has_data,
                "size": document.size,
                "has_hash": document.hash.is_some(),
                "has_content_id": document.content_id.is_some(),
            }),
            None => json!({
                "ok": true,
                "kind": "mobile_sync_document",
                "has_document": false,
            }),
        },
        OperationResult::MobileSyncDocumentApplied(outcome) => {
            let (outcome, os_write_succeeded) = mobile_sync_apply_outcome(outcome);
            json!({
                "ok": true,
                "kind": "mobile_sync_document_applied",
                "outcome": outcome,
                "os_write_succeeded": os_write_succeeded,
            })
        }
        OperationResult::MobileSyncFile(outcome) => match outcome {
            uc_engine::MobileSyncFileReadOutcome::Found(file) => json!({
                "ok": true,
                "kind": "mobile_sync_file",
                "outcome": "found",
                "has_media_type": !file.media_type.is_empty(),
                "byte_len": file.bytes.len(),
            }),
            uc_engine::MobileSyncFileReadOutcome::NotFound => json!({
                "ok": true,
                "kind": "mobile_sync_file",
                "outcome": "not_found",
            }),
        },
        OperationResult::MobileFileUploadStarted(_) => json!({
            "ok": true,
            "kind": "mobile_file_upload_started",
        }),
        OperationResult::MobileFileUploadChunkAppended => json!({
            "ok": true,
            "kind": "mobile_file_upload_chunk_appended",
        }),
        OperationResult::MobileFileUploadFinished(outcome) => {
            let (outcome, os_write_succeeded) = mobile_sync_apply_outcome(outcome);
            json!({
                "ok": true,
                "kind": "mobile_file_upload_finished",
                "outcome": outcome,
                "os_write_succeeded": os_write_succeeded,
            })
        }
        OperationResult::MobileFileUploadAborted { existed } => json!({
            "ok": true,
            "kind": "mobile_file_upload_aborted",
            "existed": existed,
        }),
        OperationResult::ReceiveReadiness(readiness) => json!({
            "ok": true,
            "kind": "receive_readiness",
            "ready": readiness.ready,
            "degraded": readiness.degraded,
        }),
        OperationResult::EncryptionState(state) => json!({
            "ok": true,
            "kind": "encryption_state",
            "initialized": state.initialized,
            "session_ready": state.session_ready,
        }),
        OperationResult::EncryptionLocked => json!({
            "ok": true,
            "kind": "encryption_locked",
        }),
        OperationResult::SecureStorageAccess { granted } => json!({
            "ok": true,
            "kind": "secure_storage_access",
            "granted": granted,
        }),
        OperationResult::Devices(devices) => json!({
            "ok": true,
            "kind": "devices",
            "count": devices.len(),
            "online_count": devices.iter().filter(|device| device.online).count(),
            "device_ids": devices.into_iter().map(|device| device.device_id).collect::<Vec<_>>(),
        }),
        OperationResult::MemberSyncPreferences(preferences) => json!({
            "ok": true,
            "kind": "member_sync_preferences",
            "send_enabled": preferences.send_enabled,
            "receive_enabled": preferences.receive_enabled,
            "send_content_types": preferences.send_content_types,
            "receive_content_types": preferences.receive_content_types,
        }),
        OperationResult::MemberRemoved => json!({
            "ok": true,
            "kind": "member_removed",
        }),
        OperationResult::SearchPage(page) => json!({
            "ok": true,
            "kind": "search_page",
            "total": page.total,
            "has_more": page.has_more,
            "item_count": page.items.len(),
            "state": page.state,
        }),
        OperationResult::SearchTags(tags) => json!({
            "ok": true,
            "kind": "search_tags",
            "tag_count": tags.len(),
        }),
        OperationResult::SearchStatus(status) => json!({
            "ok": true,
            "kind": "search_status",
            "state": status.state,
            "has_reason": status.reason.is_some(),
            "last_rebuild_started_at_ms": status.last_rebuild_started_at_ms,
            "last_rebuild_completed_at_ms": status.last_rebuild_completed_at_ms,
        }),
        OperationResult::SearchRebuildAccepted { accepted } => json!({
            "ok": true,
            "kind": "search_rebuild_accepted",
            "accepted": accepted,
        }),
        OperationResult::EntrySent(report) => json!({
            "ok": true,
            "kind": "entry_sent",
            "entry_id": report.entry_id,
            "accepted": report.total_accepted,
            "duplicate": report.total_duplicate,
            "offline": report.total_offline,
            "errored": report.total_errored,
            "pending": report.total_pending,
            "target_count": report.per_target.len(),
        }),
        OperationResult::HistoryPage {
            entries,
            next_cursor,
        } => json!({
            "ok": true,
            "kind": "history_page",
            "count": entries.len(),
            "entry_ids": entries.iter().map(|entry| entry.entry_id.clone()).collect::<Vec<_>>(),
            "content_types": entries.into_iter().map(|entry| entry.content_type).collect::<Vec<_>>(),
            "has_next": next_cursor.is_some(),
        }),
        OperationResult::HistoryEntries(entries) => json!({
            "ok": true,
            "kind": "history_entries",
            "count": entries.len(),
            "entry_ids": entries.iter().map(|entry| entry.entry_id.clone()).collect::<Vec<_>>(),
            "content_types": entries.into_iter().map(|entry| entry.content_type).collect::<Vec<_>>(),
        }),
        OperationResult::HistoryEntry(entry) => json!({
            "ok": true,
            "kind": "history_entry",
            "entry_id": entry.entry_id,
            "size_bytes": entry.size_bytes,
            "mime_type": entry.mime_type,
        }),
        OperationResult::HistoryEntryDeleted => {
            json!({"ok": true, "kind": "history_entry_deleted"})
        }
        OperationResult::HistoryEntryFavoriteSet => {
            json!({"ok": true, "kind": "history_entry_favorite_set"})
        }
        OperationResult::HistoryStats(stats) => json!({
            "ok": true,
            "kind": "history_stats",
            "total_items": stats.total_items,
            "total_size": stats.total_size,
        }),
        OperationResult::HistoryEntryResource(resource) => json!({
            "ok": true,
            "kind": "history_entry_resource",
            "has_blob": resource.blob_id.is_some(),
            "mime_type": resource.mime_type,
            "size_bytes": resource.size_bytes,
            "has_url": resource.url.is_some(),
            "has_inline_data": resource.inline_data.is_some(),
        }),
        OperationResult::BlobRead(resource) => json!({
            "ok": true,
            "kind": "blob_read",
            "byte_len": resource.bytes.len(),
            "media_type": resource.media_type,
        }),
        OperationResult::ThumbnailRead(resource) => json!({
            "ok": true,
            "kind": "thumbnail_read",
            "byte_len": resource.bytes.len(),
            "media_type": resource.media_type,
        }),
        OperationResult::EntryFileRead(resource) => json!({
            "ok": true,
            "kind": "entry_file_read",
            "byte_len": resource.bytes.len(),
            "media_type": resource.media_type,
            "has_file_name": !resource.file_name.is_empty(),
        }),
        OperationResult::HistoryCleared(result) => json!({
            "ok": true,
            "kind": "history_cleared",
            "deleted_count": result.deleted_count,
            "failed_count": result.failed_entry_ids.len(),
        }),
        OperationResult::EntryReceiveProgress(progress) => json!({
            "ok": true,
            "kind": "entry_receive_progress",
            "progress": progress.map(|progress| json!({
                "entry_id": progress.entry_id,
                "attempt_id": progress.attempt_id,
                "state": progress.state,
                "total_bytes": progress.total_bytes,
                "completed_bytes": progress.completed_bytes,
                "items_total": progress.items_total,
                "items_completed": progress.items_completed,
            })),
        }),
        OperationResult::EntryReceiveProgressList(progress) => json!({
            "ok": true,
            "kind": "entry_receive_progress_list",
            "count": progress.len(),
            "states": progress.into_iter().map(|item| item.state).collect::<Vec<_>>(),
        }),
        OperationResult::EntryReceiveCancellation(outcome) => json!({
            "ok": true,
            "kind": "entry_receive_cancellation",
            "outcome": outcome,
        }),
        OperationResult::InboundTransferCancellation(outcome) => json!({
            "ok": true,
            "kind": "inbound_transfer_cancellation",
            "outcome": outcome,
        }),
        OperationResult::ClipboardCaptured { entry_id } => json!({
            "ok": true,
            "kind": "clipboard_captured",
            "entry_id": entry_id,
        }),
        OperationResult::ClipboardRestored(outcome) => match outcome {
            uc_engine::ClipboardRestoreOutcome::Restored => json!({
                "ok": true,
                "kind": "clipboard_restored",
                "outcome": "restored",
            }),
            uc_engine::ClipboardRestoreOutcome::PayloadUnavailable { state, .. } => json!({
                "ok": true,
                "kind": "clipboard_restored",
                "outcome": "payload_unavailable",
                "state": state,
            }),
            uc_engine::ClipboardRestoreOutcome::NotApplicable { .. } => json!({
                "ok": true,
                "kind": "clipboard_restored",
                "outcome": "not_applicable",
            }),
        },
        OperationResult::EntryDelivery(view) => {
            let source = match view.source {
                uc_engine::EntrySourceSummary::Local => "local",
                uc_engine::EntrySourceSummary::Remote { .. } => "remote",
                uc_engine::EntrySourceSummary::Historical => "historical",
            };
            let statuses = view
                .deliveries
                .into_iter()
                .map(|delivery| match delivery.status {
                    uc_engine::EntryDeliveryStatusSummary::Pending => "pending",
                    uc_engine::EntryDeliveryStatusSummary::Delivered => "delivered",
                    uc_engine::EntryDeliveryStatusSummary::Duplicate => "duplicate",
                    uc_engine::EntryDeliveryStatusSummary::Unreachable => "unreachable",
                    uc_engine::EntryDeliveryStatusSummary::Failed { .. } => "failed",
                })
                .collect::<Vec<_>>();
            json!({
                "ok": true,
                "kind": "entry_delivery",
                "source": source,
                "delivery_count": statuses.len(),
                "statuses": statuses,
            })
        }
        OperationResult::EntryExported => json!({"ok": true, "kind": "entry_exported"}),
        OperationResult::EntryResent(outcome) => match outcome {
            uc_engine::ResendEntryOutcome::Completed(report) => json!({
                "ok": true,
                "kind": "entry_resent",
                "outcome": "completed",
                "accepted": report.accepted,
                "duplicate": report.duplicate,
                "offline": report.offline,
                "errored": report.errored,
                "pending": report.pending,
            }),
            uc_engine::ResendEntryOutcome::EntryNotFound { .. } => json!({
                "ok": true,
                "kind": "entry_resent",
                "outcome": "entry_not_found",
            }),
            uc_engine::ResendEntryOutcome::EntryNotResendable { reason, .. } => json!({
                "ok": true,
                "kind": "entry_resent",
                "outcome": "entry_not_resendable",
                "reason": reason,
            }),
            uc_engine::ResendEntryOutcome::TargetNotTrusted { .. } => json!({
                "ok": true,
                "kind": "entry_resent",
                "outcome": "target_not_trusted",
            }),
            uc_engine::ResendEntryOutcome::NoEligibleTargets => json!({
                "ok": true,
                "kind": "entry_resent",
                "outcome": "no_eligible_targets",
            }),
        },
    }
}

fn mobile_sync_item_type(item_type: uc_engine::MobileSyncItemType) -> &'static str {
    match item_type {
        uc_engine::MobileSyncItemType::Text => "text",
        uc_engine::MobileSyncItemType::Image => "image",
        uc_engine::MobileSyncItemType::File => "file",
        uc_engine::MobileSyncItemType::Group => "group",
    }
}

fn mobile_sync_apply_outcome(
    outcome: uc_engine::MobileSyncDocumentApplyOutcome,
) -> (&'static str, Option<bool>) {
    match outcome {
        uc_engine::MobileSyncDocumentApplyOutcome::Applied { .. } => ("applied", None),
        uc_engine::MobileSyncDocumentApplyOutcome::Resurfaced {
            os_write_succeeded, ..
        } => ("resurfaced", Some(os_write_succeeded)),
        uc_engine::MobileSyncDocumentApplyOutcome::DuplicateSkipped { .. } => {
            ("duplicate_skipped", None)
        }
        uc_engine::MobileSyncDocumentApplyOutcome::DecodeFailed { .. } => ("decode_failed", None),
        uc_engine::MobileSyncDocumentApplyOutcome::Buffered => ("buffered", None),
    }
}

fn lifecycle_response(result: Result<(), EngineError>, kind: &str) -> Value {
    match result {
        Ok(()) => json!({"ok": true, "kind": kind}),
        Err(error) => engine_error(error),
    }
}

fn record_event(summary: &Arc<Mutex<EventSummary>>, event: EngineEvent) {
    let mut summary = lock_unpoisoned(summary);
    match event {
        EngineEvent::StateChanged { state } => summary.last_state = Some(format!("{state:?}")),
        EngineEvent::IncomingEntry(_) => summary.incoming_entries += 1,
        EngineEvent::InboundNotice(_) => summary.incoming_entries += 1,
        EngineEvent::IncomingPending(_) | EngineEvent::ReceiveAttemptStateChanged(_) => {
            summary.refresh_requests += 1;
        }
        EngineEvent::TransferProgress(_) => summary.transfer_updates += 1,
        EngineEvent::TransferStatusChanged(_) | EngineEvent::DeliveryStatusChanged(_) => {
            summary.refresh_requests += 1;
        }
        EngineEvent::PeerPresenceChanged(_) => summary.refresh_requests += 1,
        EngineEvent::ActiveClipboardChanged(_) => summary.refresh_requests += 1,
        EngineEvent::MobileLanSettingsChanged(_) => summary.refresh_requests += 1,
        EngineEvent::RefreshRequired { .. } => summary.refresh_requests += 1,
        EngineEvent::OperationFinished { .. } => summary.completed_operations += 1,
        EngineEvent::LifecycleFailed { .. } => summary.lifecycle_failures += 1,
        EngineEvent::Fatal { .. } => summary.fatal_errors += 1,
    }
}

fn engine_error(error: EngineError) -> Value {
    json!({
        "ok": false,
        "kind": "engine_error",
        "code": error.code(),
        "category": error.category().to_string(),
        "retryable": error.is_retryable(),
    })
}

fn probe_error(kind: &str) -> Value {
    json!({"ok": false, "kind": kind})
}

fn host_error(category: HostCapabilityErrorCategory) -> HostCapabilityError {
    HostCapabilityError::new(category, "probe host capability failed")
}

fn lock_unpoisoned<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(value) => value,
        Err(poisoned) => poisoned.into_inner(),
    }
}

static CLIENT: OnceLock<Result<ProbeClient, String>> = OnceLock::new();

#[cfg(target_vendor = "apple")]
fn host_secure_storage() -> Box<dyn HostSecureStorage> {
    Box::new(KeychainStorage)
}

#[cfg(target_os = "android")]
fn host_secure_storage() -> Box<dyn HostSecureStorage> {
    Box::new(android::AndroidSecureStorage)
}

#[cfg(not(any(target_vendor = "apple", target_os = "android")))]
fn host_secure_storage() -> Box<dyn HostSecureStorage> {
    Box::new(UnavailableSecureStorage)
}

pub(crate) fn probe_command(command: &str) -> String {
    let command = match serde_json::from_str(command) {
        Ok(command) => command,
        Err(_) => return probe_error("invalid_command").to_string(),
    };
    let client = CLIENT.get_or_init(ProbeClient::new);
    match client {
        Ok(client) => client.execute(command).to_string(),
        Err(_) => probe_error("runtime_unavailable").to_string(),
    }
}

#[no_mangle]
pub unsafe extern "C" fn uc_ios_probe_command(command: *const c_char) -> *mut c_char {
    if command.is_null() {
        return value_to_c_string(probe_error("invalid_command"));
    }
    let input = match CStr::from_ptr(command).to_str() {
        Ok(input) => input,
        Err(_) => return value_to_c_string(probe_error("invalid_command")),
    };
    string_to_c_string(probe_command(input))
}

#[no_mangle]
pub unsafe extern "C" fn uc_ios_probe_string_free(value: *mut c_char) {
    if !value.is_null() {
        drop(CString::from_raw(value));
    }
}

fn value_to_c_string(value: Value) -> *mut c_char {
    string_to_c_string(value.to_string())
}

fn string_to_c_string(value: String) -> *mut c_char {
    match CString::new(value) {
        Ok(value) => value.into_raw(),
        Err(_) => ptr::null_mut(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_summary_counts_lifecycle_transition_failures() {
        let summary = Arc::new(Mutex::new(EventSummary::default()));

        record_event(
            &summary,
            EngineEvent::LifecycleFailed {
                action: uc_engine::LifecycleAction::Resume,
                error: EngineError::new(1214, uc_engine::EngineErrorCategory::Unavailable, true),
            },
        );

        assert_eq!(lock_unpoisoned(&summary).lifecycle_failures, 1);
    }

    #[test]
    fn operation_response_exposes_stable_result_without_debug_output() {
        let response = operation_response(OperationResult::SpaceJoined {
            sponsor_device_id: "sponsor-1".into(),
            sponsor_identity_fingerprint: "sponsor-fingerprint".into(),
            space_id: "space-1".into(),
            self_device_id: "device-1".into(),
            self_identity_fingerprint: "self-fingerprint".into(),
            migrated_records: None,
        });

        assert_eq!(
            response,
            json!({"ok": true, "kind": "space_joined", "space_id": "space-1"})
        );

        let resend = operation_response(OperationResult::EntryResent(
            uc_engine::ResendEntryOutcome::Completed(uc_engine::ResendReportSummary {
                accepted: 1,
                duplicate: 2,
                offline: 3,
                errored: 4,
                pending: 5,
            }),
        ));
        assert_eq!(resend["accepted"], 1);
        assert_eq!(resend["pending"], 5);

        let upgrade = operation_response(OperationResult::UpgradeStatus(
            uc_engine::UpgradeStatusSummary::Upgraded {
                from: Some("1.1.0".into()),
                to: "1.2.0".into(),
            },
        ));
        assert_eq!(
            upgrade,
            json!({
                "ok": true,
                "kind": "upgrade_status",
                "outcome": "upgraded",
                "from": "1.1.0",
                "to": "1.2.0",
            })
        );

        let logs = operation_response(OperationResult::DiagnosticLogsExported(
            uc_engine::DiagnosticLogsExportSummary {
                included_files: vec!["private-log-name.json".into()],
                since_unix_ms: 1_700_000_000_000,
            },
        ));
        assert_eq!(logs["included_file_count"], 1);
        assert!(!logs.to_string().contains("private-log-name.json"));

        let config = operation_response(OperationResult::ConfigImportPreview(
            uc_engine::ConfigImportPreviewOutcome::Ready(uc_engine::ConfigImportPreviewSummary {
                app_version: "1.2.3".into(),
                source_mode: uc_engine::ConfigSourceModeSummary::Installed,
                created_at_unix_ms: 1_700_000_000_000,
                profile_id: "private profile".into(),
                device_fingerprint: "private fingerprint".into(),
            }),
        ));
        assert_eq!(config["outcome"], "ready");
        assert_eq!(config["has_profile_id"], true);
        assert_eq!(config["has_device_fingerprint"], true);
        assert!(!config.to_string().contains("private profile"));
        assert!(!config.to_string().contains("private fingerprint"));

        let mobile = operation_response(OperationResult::MobileRequestAuthenticated(
            uc_engine::MobileAuthenticatedSession {
                device_id: "private mobile device".into(),
                client_type: uc_engine::MobileClientTypeSummary::IosShortcut,
                credential: uc_engine::MobileCredential::new(
                    "private mobile device",
                    "private password proof",
                ),
            },
        ));
        assert_eq!(mobile["kind"], "mobile_request_authenticated");
        assert_eq!(mobile["has_credential"], true);
        assert!(!mobile.to_string().contains("private mobile device"));
        assert!(!mobile.to_string().contains("private password proof"));
    }

    #[test]
    fn operation_response_exposes_session_recovery_state() {
        let response = operation_response(OperationResult::SessionRecovered {
            unlocked: true,
            resumed: false,
        });

        assert_eq!(
            response,
            json!({
                "ok": true,
                "kind": "session_recovered",
                "unlocked": true,
                "resumed": false,
            })
        );
    }

    #[test]
    fn operation_response_exposes_storage_counts() {
        let stats = operation_response(OperationResult::StorageStats(
            uc_engine::StorageStatsSummary {
                total_bytes: 50,
                database_bytes: 10,
                vault_bytes: 20,
                cache_bytes: 15,
                logs_bytes: 5,
            },
        ));
        let cleared = operation_response(OperationResult::StorageCacheCleared { freed_bytes: 15 });

        assert_eq!(
            stats,
            json!({
                "ok": true,
                "kind": "storage_stats",
                "total_bytes": 50,
                "database_bytes": 10,
                "vault_bytes": 20,
                "cache_bytes": 15,
                "logs_bytes": 5,
            })
        );
        assert_eq!(
            cleared,
            json!({"ok": true, "kind": "storage_cache_cleared", "freed_bytes": 15})
        );
    }

    #[test]
    fn operation_response_redacts_device_names_and_history_previews() {
        let local = operation_response(OperationResult::LocalDevice(
            uc_engine::LocalDeviceSummary {
                device_id: "device-local".into(),
                display_name: "private local device name".into(),
            },
        ));
        let devices =
            operation_response(OperationResult::Devices(vec![uc_engine::DeviceSummary {
                device_id: "device-1".into(),
                display_name: "private phone name".into(),
                online: true,
            }]));
        let peers = operation_response(OperationResult::PeerConnections(vec![
            uc_engine::PeerConnectionSummary {
                peer_id: "private-peer-id".into(),
                device_name: Some("private peer name".into()),
                addresses: vec!["private peer address".into()],
                is_paired: true,
                connected: true,
                pairing_state: "trusted".into(),
                channel: uc_engine::PeerConnectionChannelSummary::Relay,
                connection_address: Some("private active address".into()),
            },
        ]));
        let mut settings = uc_engine::SettingsSummary::default();
        settings.general.device_name = Some("private settings device".into());
        settings
            .network
            .custom_relay_urls
            .push("https://private-settings-relay.example".into());
        settings.file_sync.auto_save_dir = Some("/private/settings/path".into());
        let settings = operation_response(OperationResult::Settings(Box::new(settings)));
        let settings_rejected = operation_response(OperationResult::SettingsUpdated(
            uc_engine::SettingsUpdateOutcome::Rejected {
                reason: "private settings rejection".into(),
            },
        ));
        let mobile_settings = operation_response(OperationResult::MobileSyncSettings(Box::new(
            uc_engine::MobileSyncSettingsSummary {
                enabled: true,
                lan_listen_enabled: true,
                lan_advertise_ip: Some("192.168.1.23".into()),
                lan_advertise_base_url: Some("https://private-mobile.example".into()),
                lan_port: Some(42720),
                lan_listener_error: Some("private mobile bind failure".into()),
                shortcut_install_methods: vec![uc_engine::MobileShortcutInstallMethodSummary {
                    method: uc_engine::MobileShortcutInstallMethod::IcloudGeneric,
                    available: false,
                    disabled_reason: Some("private install reason".into()),
                }],
            },
        )));
        let mobile_settings_rejected =
            operation_response(OperationResult::MobileSyncSettingsUpdated(
                uc_engine::MobileSyncSettingsUpdateOutcome::Rejected {
                    reason: "private mobile settings rejection".into(),
                },
            ));
        let mobile_document = operation_response(OperationResult::MobileSyncDocument(Some(
            Box::new(uc_engine::MobileSyncDocument {
                item_type: uc_engine::MobileSyncItemType::File,
                text: "private mobile text".into(),
                data_name: Some("private-mobile-file.txt".into()),
                has_data: true,
                size: 19,
                hash: Some("private compatibility hash".into()),
                content_id: Some("private stable content id".into()),
            }),
        )));
        let mobile_applied = operation_response(OperationResult::MobileSyncDocumentApplied(
            uc_engine::MobileSyncDocumentApplyOutcome::DecodeFailed {
                reason: "private decode reason".into(),
            },
        ));
        let mobile_file = operation_response(OperationResult::MobileSyncFile(
            uc_engine::MobileSyncFileReadOutcome::Found(Box::new(uc_engine::MobileSyncFile {
                media_type: "text/private".into(),
                bytes: b"private mobile file bytes".to_vec(),
            })),
        ));
        let relay = operation_response(OperationResult::RelayProbed(
            uc_engine::RelayProbeOutcome::Dns {
                message: "private relay error".into(),
            },
        ));
        let history = operation_response(OperationResult::HistoryPage {
            entries: vec![uc_engine::EntrySummary {
                entry_id: "entry-1".into(),
                content_type: "text".into(),
                preview: Some("private payload".into()),
                created_at_ms: 1,
            }],
            next_cursor: None,
        });
        let history_entry = operation_response(OperationResult::HistoryEntry(
            uc_engine::HistoryEntryDetailSummary {
                entry_id: "entry-2".into(),
                content: "private full content".into(),
                size_bytes: 20,
                created_at_ms: 1,
                active_time_ms: 2,
                mime_type: Some("text/plain".into()),
            },
        ));
        let history_resource = operation_response(OperationResult::HistoryEntryResource(
            uc_engine::HistoryEntryResourceSummary {
                blob_id: Some("blob-1".into()),
                mime_type: Some("text/plain".into()),
                size_bytes: 20,
                url: Some("http://private/resource".into()),
                inline_data: Some(b"private inline content".to_vec()),
            },
        ));
        let blob = operation_response(OperationResult::BlobRead(
            uc_engine::BinaryResourceSummary {
                bytes: b"private blob bytes".to_vec(),
                media_type: Some("image/png".into()),
            },
        ));
        let entry_file = operation_response(OperationResult::EntryFileRead(
            uc_engine::EntryFileResourceSummary {
                bytes: b"private file bytes".to_vec(),
                media_type: Some("application/pdf".into()),
                file_name: "private-report.pdf".into(),
            },
        ));
        let payload_unavailable = operation_response(OperationResult::ClipboardRestored(
            uc_engine::ClipboardRestoreOutcome::PayloadUnavailable {
                entry_id: "private-entry".into(),
                representation_id: "private-representation".into(),
                state: "Lost".into(),
            },
        ));
        let delivery = operation_response(OperationResult::EntryDelivery(
            uc_engine::EntryDeliveryViewSummary {
                entry_id: "private-entry".into(),
                source: uc_engine::EntrySourceSummary::Remote {
                    device_id: "private-source-id".into(),
                    device_name: Some("private source name".into()),
                },
                deliveries: vec![uc_engine::EntryDeliveryTargetSummary {
                    target_device_id: "private-target-id".into(),
                    target_device_name: Some("private target name".into()),
                    status: uc_engine::EntryDeliveryStatusSummary::Failed {
                        reason: uc_engine::DeliveryFailureReasonSummary::Internal,
                    },
                    reason_detail: Some("private failure detail".into()),
                    updated_at_ms: Some(42),
                }],
            },
        ));

        assert!(!local.to_string().contains("private local device name"));
        assert!(!devices.to_string().contains("private phone name"));
        assert_eq!(peers["channels"][0], "relay");
        for secret in [
            "private-peer-id",
            "private peer name",
            "private peer address",
            "private active address",
        ] {
            assert!(!peers.to_string().contains(secret));
        }
        for secret in [
            "private settings device",
            "private-settings-relay.example",
            "/private/settings/path",
            "private settings rejection",
            "private relay error",
            "192.168.1.23",
            "private-mobile.example",
            "private mobile bind failure",
            "private install reason",
            "private mobile settings rejection",
            "private mobile text",
            "private-mobile-file.txt",
            "private compatibility hash",
            "private stable content id",
            "private decode reason",
            "text/private",
            "private mobile file bytes",
        ] {
            assert!(!settings.to_string().contains(secret));
            assert!(!settings_rejected.to_string().contains(secret));
            assert!(!relay.to_string().contains(secret));
            assert!(!mobile_settings.to_string().contains(secret));
            assert!(!mobile_settings_rejected.to_string().contains(secret));
            assert!(!mobile_document.to_string().contains(secret));
            assert!(!mobile_applied.to_string().contains(secret));
            assert!(!mobile_file.to_string().contains(secret));
        }
        assert_eq!(mobile_settings["shortcut_install_method_count"], 1);
        assert_eq!(mobile_document["item_type"], "file");
        assert_eq!(mobile_file["byte_len"], 25);
        assert!(!history.to_string().contains("private payload"));
        assert!(!history_entry.to_string().contains("private full content"));
        assert!(!history_resource.to_string().contains("private/resource"));
        assert!(!history_resource
            .to_string()
            .contains("private inline content"));
        assert_eq!(blob["byte_len"], 18);
        assert!(!blob.to_string().contains("private blob bytes"));
        assert_eq!(entry_file["has_file_name"], true);
        assert!(!entry_file.to_string().contains("private file bytes"));
        assert!(!entry_file.to_string().contains("private-report.pdf"));
        assert_eq!(payload_unavailable["outcome"], "payload_unavailable");
        assert_eq!(payload_unavailable["state"], "Lost");
        assert!(!payload_unavailable.to_string().contains("private-entry"));
        assert!(!payload_unavailable
            .to_string()
            .contains("private-representation"));
        assert_eq!(delivery["source"], "remote");
        assert_eq!(delivery["statuses"][0], "failed");
        assert!(!delivery.to_string().contains("private-entry"));
        assert!(!delivery.to_string().contains("private-source-id"));
        assert!(!delivery.to_string().contains("private source name"));
        assert!(!delivery.to_string().contains("private-target-id"));
        assert!(!delivery.to_string().contains("private target name"));
        assert!(!delivery.to_string().contains("private failure detail"));
    }
}
