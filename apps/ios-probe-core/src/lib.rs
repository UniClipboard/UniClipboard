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

const KEYCHAIN_SERVICE: &str = "app.uniclipboard.engine-probe";
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
            .name("uc-ios-probe-runtime".into())
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

struct KeychainStorage;

impl HostSecureStorage for KeychainStorage {
    fn get(&self, key: &str) -> Result<Option<Vec<u8>>, HostCapabilityError> {
        #[cfg(target_vendor = "apple")]
        {
            match security_framework::passwords::get_generic_password(KEYCHAIN_SERVICE, key) {
                Ok(value) => Ok(Some(value)),
                Err(error) if error.code() == ITEM_NOT_FOUND_STATUS => Ok(None),
                Err(_) => Err(host_error(HostCapabilityErrorCategory::Unavailable)),
            }
        }
        #[cfg(not(target_vendor = "apple"))]
        {
            let _ = key;
            Err(host_error(HostCapabilityErrorCategory::Unavailable))
        }
    }

    fn set(&self, key: &str, value: &[u8]) -> Result<(), HostCapabilityError> {
        #[cfg(target_vendor = "apple")]
        {
            security_framework::passwords::set_generic_password(KEYCHAIN_SERVICE, key, value)
                .map_err(|_| host_error(HostCapabilityErrorCategory::Unavailable))
        }
        #[cfg(not(target_vendor = "apple"))]
        {
            let _ = (key, value);
            Err(host_error(HostCapabilityErrorCategory::Unavailable))
        }
    }

    fn delete(&self, key: &str) -> Result<(), HostCapabilityError> {
        #[cfg(target_vendor = "apple")]
        {
            match security_framework::passwords::delete_generic_password(KEYCHAIN_SERVICE, key) {
                Ok(()) => Ok(()),
                Err(error) if error.code() == ITEM_NOT_FOUND_STATUS => Ok(()),
                Err(_) => Err(host_error(HostCapabilityErrorCategory::Unavailable)),
            }
        }
        #[cfg(not(target_vendor = "apple"))]
        {
            let _ = key;
            Err(host_error(HostCapabilityErrorCategory::Unavailable))
        }
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
                Box::new(KeychainStorage),
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
                    device_name,
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
        OperationResult::SpaceJoined { space_id } => {
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
        OperationResult::Devices(devices) => json!({
            "ok": true,
            "kind": "devices",
            "count": devices.len(),
            "online_count": devices.iter().filter(|device| device.online).count(),
            "device_ids": devices.into_iter().map(|device| device.device_id).collect::<Vec<_>>(),
        }),
        OperationResult::EntrySent { entry_id } => {
            json!({"ok": true, "kind": "entry_sent", "entry_id": entry_id})
        }
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
        OperationResult::EntryExported => json!({"ok": true, "kind": "entry_exported"}),
        OperationResult::EntryResent { entry_id } => {
            json!({"ok": true, "kind": "entry_resent", "entry_id": entry_id})
        }
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
        EngineEvent::IncomingEntry { .. } => summary.incoming_entries += 1,
        EngineEvent::TransferProgress(_) => summary.transfer_updates += 1,
        EngineEvent::RefreshRequired { .. } => summary.refresh_requests += 1,
        EngineEvent::OperationFinished { .. } => summary.completed_operations += 1,
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

#[no_mangle]
pub unsafe extern "C" fn uc_ios_probe_command(command: *const c_char) -> *mut c_char {
    if command.is_null() {
        return value_to_c_string(probe_error("invalid_command"));
    }
    let input = match CStr::from_ptr(command).to_str() {
        Ok(input) => input,
        Err(_) => return value_to_c_string(probe_error("invalid_command")),
    };
    let command = match serde_json::from_str(input) {
        Ok(command) => command,
        Err(_) => return value_to_c_string(probe_error("invalid_command")),
    };
    let client = CLIENT.get_or_init(ProbeClient::new);
    match client {
        Ok(client) => value_to_c_string(client.execute(command)),
        Err(_) => value_to_c_string(probe_error("runtime_unavailable")),
    }
}

#[no_mangle]
pub unsafe extern "C" fn uc_ios_probe_string_free(value: *mut c_char) {
    if !value.is_null() {
        drop(CString::from_raw(value));
    }
}

fn value_to_c_string(value: Value) -> *mut c_char {
    let serialized = value.to_string();
    match CString::new(serialized) {
        Ok(value) => value.into_raw(),
        Err(_) => ptr::null_mut(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operation_response_exposes_stable_result_without_debug_output() {
        let response = operation_response(OperationResult::SpaceJoined {
            space_id: "space-1".into(),
        });

        assert_eq!(
            response,
            json!({"ok": true, "kind": "space_joined", "space_id": "space-1"})
        );
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
    fn operation_response_redacts_device_names_and_history_previews() {
        let devices =
            operation_response(OperationResult::Devices(vec![uc_engine::DeviceSummary {
                device_id: "device-1".into(),
                display_name: "private phone name".into(),
                online: true,
            }]));
        let history = operation_response(OperationResult::HistoryPage {
            entries: vec![uc_engine::EntrySummary {
                entry_id: "entry-1".into(),
                content_type: "text".into(),
                preview: Some("private payload".into()),
                created_at_ms: 1,
            }],
            next_cursor: None,
        });

        assert!(!devices.to_string().contains("private phone name"));
        assert!(!history.to_string().contains("private payload"));
    }
}
