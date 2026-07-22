use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use uc_engine::{
    ClipboardRestoreMode, ClipboardRestoreOutcome, CreateSpaceInput, Engine, EngineConfig,
    ExportEntryInput, HostCapabilities, HostCapabilityError, HostCapabilityErrorCategory,
    HostClipboard, HostClipboardRepresentation, HostClipboardSnapshot, HostDirectories,
    HostFileAccess, HostFileHandle, HostFileMetadata, HostSecureStorage, JoinSpaceInput, Operation,
    OperationResult, RestoreClipboardInput, SecretString, SendFilesInput, SendImageInput,
    SendTextInput,
};
use zeroize::Zeroizing;

use crate::{
    BindingClipboardRepresentation, BindingClipboardRestoreMode, BindingClipboardRestoreOutcome,
    BindingClipboardSnapshot, BindingConfig, BindingEngineState, BindingError, BindingEvent,
    BindingFailure, BindingFileMetadata, BindingHost, BindingOperationTerminal,
    BindingRefreshReason, HostBindingError,
};

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct SpaceCreated {
    pub space_id: String,
    pub self_device_id: String,
    pub identity_fingerprint: String,
}

#[derive(Clone, PartialEq, Eq, uniffi::Record)]
pub struct InvitationIssued {
    pub invitation_code: String,
    pub expires_at_ms: i64,
}

impl std::fmt::Debug for InvitationIssued {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("InvitationIssued")
            .field("invitation_code", &"[REDACTED]")
            .field("expires_at_ms", &self.expires_at_ms)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct SpaceJoined {
    pub sponsor_device_id: String,
    pub sponsor_identity_fingerprint: String,
    pub space_id: String,
    pub self_device_id: String,
    pub self_identity_fingerprint: String,
    pub migrated_records: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct SendReport {
    pub entry_id: String,
    pub at_ms: i64,
    pub total_accepted: u64,
    pub total_duplicate: u64,
    pub total_offline: u64,
    pub total_errored: u64,
    pub total_pending: u64,
}

enum WorkerCommand {
    CreateSpace {
        device_name: Option<String>,
        passphrase: Zeroizing<String>,
        response: mpsc::Sender<Result<SpaceCreated, BindingError>>,
    },
    IssueInvitation {
        response: mpsc::Sender<Result<InvitationIssued, BindingError>>,
    },
    JoinSpace {
        invitation_code: Zeroizing<String>,
        device_name: Option<String>,
        passphrase: Zeroizing<String>,
        response: mpsc::Sender<Result<SpaceJoined, BindingError>>,
    },
    SendText {
        text: Zeroizing<String>,
        target_devices: Vec<String>,
        response: mpsc::Sender<Result<SendReport, BindingError>>,
    },
    SendImage {
        bytes: Zeroizing<Vec<u8>>,
        mime_type: String,
        target_devices: Vec<String>,
        response: mpsc::Sender<Result<SendReport, BindingError>>,
    },
    SendFiles {
        file_handles: Zeroizing<Vec<String>>,
        target_devices: Vec<String>,
        response: mpsc::Sender<Result<SendReport, BindingError>>,
    },
    CaptureCurrentClipboard {
        response: mpsc::Sender<Result<Option<String>, BindingError>>,
    },
    RestoreClipboard {
        entry_id: String,
        mode: BindingClipboardRestoreMode,
        response: mpsc::Sender<Result<BindingClipboardRestoreOutcome, BindingError>>,
    },
    ExportEntry {
        entry_id: String,
        destination_handle: Zeroizing<String>,
        response: mpsc::Sender<Result<(), BindingError>>,
    },
    Suspend {
        response: mpsc::Sender<Result<(), BindingError>>,
    },
    Resume {
        response: mpsc::Sender<Result<(), BindingError>>,
    },
    Shutdown {
        deadline: Duration,
        response: mpsc::Sender<Result<(), BindingError>>,
    },
}

#[derive(uniffi::Object)]
pub struct MobileEngine {
    commands: Mutex<Option<tokio::sync::mpsc::UnboundedSender<WorkerCommand>>>,
    events: Arc<EventQueue>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

struct EventQueueState {
    events: VecDeque<BindingEvent>,
    lagged: bool,
    closed: bool,
}

struct EventQueue {
    capacity: usize,
    state: Mutex<EventQueueState>,
    ready: Condvar,
}

impl EventQueue {
    fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            state: Mutex::new(EventQueueState {
                events: VecDeque::new(),
                lagged: false,
                closed: false,
            }),
            ready: Condvar::new(),
        }
    }

    fn push(&self, event: BindingEvent) {
        let mut state = lock(&self.state);
        if state.closed {
            return;
        }
        if state.events.len() == self.capacity {
            state.events.pop_front();
            state.lagged = true;
        }
        state.events.push_back(event);
        self.ready.notify_one();
    }

    fn close(&self) {
        lock(&self.state).closed = true;
        self.ready.notify_all();
    }

    fn next(&self, timeout: Duration) -> Option<BindingEvent> {
        let deadline = Instant::now().checked_add(timeout);
        let mut state = lock(&self.state);
        loop {
            if state.lagged {
                state.lagged = false;
                return Some(BindingEvent::RefreshRequired {
                    reason: BindingRefreshReason::ConsumerLagged,
                });
            }
            if let Some(event) = state.events.pop_front() {
                return Some(event);
            }
            if state.closed || timeout.is_zero() {
                return None;
            }
            let remaining = deadline
                .map(|deadline| deadline.saturating_duration_since(Instant::now()))
                .unwrap_or(Duration::from_secs(60));
            if remaining.is_zero() {
                return None;
            }
            state = match self.ready.wait_timeout(state, remaining) {
                Ok((state, _)) => state,
                Err(poisoned) => poisoned.into_inner().0,
            };
        }
    }
}

#[uniffi::export]
impl MobileEngine {
    #[uniffi::constructor]
    pub fn start(
        config: BindingConfig,
        host: Arc<dyn BindingHost>,
    ) -> Result<Arc<Self>, BindingError> {
        let capabilities = host_capabilities(Arc::clone(&host))?;
        let config = EngineConfig::new(config.app_version).with_profile_id(config.profile_id);
        let (commands, requests) = tokio::sync::mpsc::unbounded_channel();
        let events = Arc::new(EventQueue::new(256));
        let (started, start_result) = mpsc::channel();
        let worker_events = Arc::clone(&events);
        let worker = std::thread::Builder::new()
            .name("uc-engine-uniffi".to_owned())
            .spawn(move || run_worker(config, capabilities, requests, worker_events, started))
            .map_err(|_| BindingError::RuntimeUnavailable)?;

        match start_result.recv() {
            Ok(Ok(())) => Ok(Arc::new(Self {
                commands: Mutex::new(Some(commands)),
                events,
                worker: Mutex::new(Some(worker)),
            })),
            Ok(Err(error)) => {
                let _ = worker.join();
                Err(error)
            }
            Err(_) => {
                let _ = worker.join();
                Err(BindingError::RuntimeUnavailable)
            }
        }
    }

    pub fn create_space(
        &self,
        device_name: Option<String>,
        passphrase: String,
    ) -> Result<SpaceCreated, BindingError> {
        let passphrase = Zeroizing::new(passphrase);
        let commands = self.command_sender()?;
        let (response, result) = mpsc::channel();
        commands
            .send(WorkerCommand::CreateSpace {
                device_name,
                passphrase,
                response,
            })
            .map_err(|_| BindingError::RuntimeUnavailable)?;
        result
            .recv()
            .map_err(|_| BindingError::RuntimeUnavailable)?
    }

    pub fn issue_invitation(&self) -> Result<InvitationIssued, BindingError> {
        let commands = self.command_sender()?;
        let (response, result) = mpsc::channel();
        commands
            .send(WorkerCommand::IssueInvitation { response })
            .map_err(|_| BindingError::RuntimeUnavailable)?;
        result
            .recv()
            .map_err(|_| BindingError::RuntimeUnavailable)?
    }

    pub fn join_space(
        &self,
        invitation_code: String,
        device_name: Option<String>,
        passphrase: String,
    ) -> Result<SpaceJoined, BindingError> {
        let commands = self.command_sender()?;
        let (response, result) = mpsc::channel();
        commands
            .send(WorkerCommand::JoinSpace {
                invitation_code: Zeroizing::new(invitation_code),
                device_name,
                passphrase: Zeroizing::new(passphrase),
                response,
            })
            .map_err(|_| BindingError::RuntimeUnavailable)?;
        result
            .recv()
            .map_err(|_| BindingError::RuntimeUnavailable)?
    }

    pub fn send_text(
        &self,
        text: String,
        target_devices: Vec<String>,
    ) -> Result<SendReport, BindingError> {
        let commands = self.command_sender()?;
        let (response, result) = mpsc::channel();
        commands
            .send(WorkerCommand::SendText {
                text: Zeroizing::new(text),
                target_devices,
                response,
            })
            .map_err(|_| BindingError::RuntimeUnavailable)?;
        result
            .recv()
            .map_err(|_| BindingError::RuntimeUnavailable)?
    }

    pub fn send_image(
        &self,
        bytes: Vec<u8>,
        mime_type: String,
        target_devices: Vec<String>,
    ) -> Result<SendReport, BindingError> {
        let commands = self.command_sender()?;
        let (response, result) = mpsc::channel();
        commands
            .send(WorkerCommand::SendImage {
                bytes: Zeroizing::new(bytes),
                mime_type,
                target_devices,
                response,
            })
            .map_err(|_| BindingError::RuntimeUnavailable)?;
        result
            .recv()
            .map_err(|_| BindingError::RuntimeUnavailable)?
    }

    pub fn send_files(
        &self,
        file_handles: Vec<String>,
        target_devices: Vec<String>,
    ) -> Result<SendReport, BindingError> {
        let commands = self.command_sender()?;
        let (response, result) = mpsc::channel();
        commands
            .send(WorkerCommand::SendFiles {
                file_handles: Zeroizing::new(file_handles),
                target_devices,
                response,
            })
            .map_err(|_| BindingError::RuntimeUnavailable)?;
        result
            .recv()
            .map_err(|_| BindingError::RuntimeUnavailable)?
    }

    pub fn capture_current_clipboard(&self) -> Result<Option<String>, BindingError> {
        let commands = self.command_sender()?;
        let (response, result) = mpsc::channel();
        commands
            .send(WorkerCommand::CaptureCurrentClipboard { response })
            .map_err(|_| BindingError::RuntimeUnavailable)?;
        result
            .recv()
            .map_err(|_| BindingError::RuntimeUnavailable)?
    }

    pub fn restore_clipboard(
        &self,
        entry_id: String,
        mode: BindingClipboardRestoreMode,
    ) -> Result<BindingClipboardRestoreOutcome, BindingError> {
        let commands = self.command_sender()?;
        let (response, result) = mpsc::channel();
        commands
            .send(WorkerCommand::RestoreClipboard {
                entry_id,
                mode,
                response,
            })
            .map_err(|_| BindingError::RuntimeUnavailable)?;
        result
            .recv()
            .map_err(|_| BindingError::RuntimeUnavailable)?
    }

    pub fn export_entry(
        &self,
        entry_id: String,
        destination_handle: String,
    ) -> Result<(), BindingError> {
        let commands = self.command_sender()?;
        let (response, result) = mpsc::channel();
        commands
            .send(WorkerCommand::ExportEntry {
                entry_id,
                destination_handle: Zeroizing::new(destination_handle),
                response,
            })
            .map_err(|_| BindingError::RuntimeUnavailable)?;
        result
            .recv()
            .map_err(|_| BindingError::RuntimeUnavailable)?
    }

    pub fn shutdown(&self, deadline_ms: u64) -> Result<(), BindingError> {
        self.shutdown_inner(Duration::from_millis(deadline_ms), true)
    }

    pub fn suspend(&self) -> Result<(), BindingError> {
        let commands = self.command_sender()?;
        let (response, result) = mpsc::channel();
        commands
            .send(WorkerCommand::Suspend { response })
            .map_err(|_| BindingError::RuntimeUnavailable)?;
        result
            .recv()
            .map_err(|_| BindingError::RuntimeUnavailable)?
    }

    pub fn resume(&self) -> Result<(), BindingError> {
        let commands = self.command_sender()?;
        let (response, result) = mpsc::channel();
        commands
            .send(WorkerCommand::Resume { response })
            .map_err(|_| BindingError::RuntimeUnavailable)?;
        result
            .recv()
            .map_err(|_| BindingError::RuntimeUnavailable)?
    }

    pub fn next_event(&self, timeout_ms: u64) -> Option<BindingEvent> {
        self.events.next(Duration::from_millis(timeout_ms))
    }
}

impl MobileEngine {
    fn command_sender(
        &self,
    ) -> Result<tokio::sync::mpsc::UnboundedSender<WorkerCommand>, BindingError> {
        lock(&self.commands)
            .as_ref()
            .cloned()
            .ok_or(BindingError::AlreadyStopped)
    }

    fn shutdown_inner(&self, deadline: Duration, join: bool) -> Result<(), BindingError> {
        let commands = lock(&self.commands)
            .take()
            .ok_or(BindingError::AlreadyStopped)?;
        let (response, result) = mpsc::channel();
        let shutdown_result = commands
            .send(WorkerCommand::Shutdown { deadline, response })
            .map_err(|_| BindingError::RuntimeUnavailable)
            .and_then(|()| {
                result
                    .recv()
                    .map_err(|_| BindingError::RuntimeUnavailable)?
            });
        let join_result = if join { self.join_worker() } else { Ok(()) };
        shutdown_result.and(join_result)
    }

    fn join_worker(&self) -> Result<(), BindingError> {
        if let Some(worker) = lock(&self.worker).take() {
            worker
                .join()
                .map_err(|_| BindingError::RuntimeUnavailable)?;
        }
        Ok(())
    }
}

impl Drop for MobileEngine {
    fn drop(&mut self) {
        let _ = self.shutdown_inner(Duration::ZERO, true);
    }
}

fn run_worker(
    config: EngineConfig,
    host: HostCapabilities,
    requests: tokio::sync::mpsc::UnboundedReceiver<WorkerCommand>,
    events: Arc<EventQueue>,
    started: mpsc::Sender<Result<(), BindingError>>,
) {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => {
            let _ = started.send(Err(BindingError::RuntimeUnavailable));
            return;
        }
    };
    runtime.block_on(run_worker_loop(config, host, requests, events, started));
}

async fn run_worker_loop(
    config: EngineConfig,
    host: HostCapabilities,
    mut requests: tokio::sync::mpsc::UnboundedReceiver<WorkerCommand>,
    events: Arc<EventQueue>,
    started: mpsc::Sender<Result<(), BindingError>>,
) {
    let (engine, mut engine_events) = match Engine::start(config, host).await {
        Ok(started_engine) => started_engine,
        Err(error) => {
            let _ = started.send(Err(error.into()));
            return;
        }
    };
    if started.send(Ok(())).is_err() {
        let _ = engine.shutdown(Duration::ZERO).await;
        return;
    }

    let event_task = tokio::spawn(async move {
        while let Some(event) = engine_events.next().await {
            events.push(map_engine_event(event));
        }
        events.close();
    });

    let mut shutdown_response = None;

    while let Some(command) = requests.recv().await {
        match command {
            WorkerCommand::CreateSpace {
                device_name,
                passphrase,
                response,
            } => {
                let result = engine
                    .execute(Operation::CreateSpace(CreateSpaceInput {
                        device_name,
                        passphrase: SecretString::new(passphrase.as_str()),
                        passphrase_confirmation: SecretString::new(passphrase.as_str()),
                    }))
                    .await
                    .map_err(BindingError::from)
                    .and_then(map_space_created);
                let _ = response.send(result);
            }
            WorkerCommand::IssueInvitation { response } => {
                let result = engine
                    .execute(Operation::IssueInvitation)
                    .await
                    .map_err(BindingError::from)
                    .and_then(map_invitation_issued);
                let _ = response.send(result);
            }
            WorkerCommand::JoinSpace {
                mut invitation_code,
                device_name,
                passphrase,
                response,
            } => {
                let result = engine
                    .execute(Operation::JoinSpace(JoinSpaceInput {
                        invitation_code: std::mem::take(&mut *invitation_code),
                        device_name,
                        passphrase: SecretString::new(passphrase.as_str()),
                    }))
                    .await
                    .map_err(BindingError::from)
                    .and_then(map_space_joined);
                let _ = response.send(result);
            }
            WorkerCommand::SendText {
                mut text,
                target_devices,
                response,
            } => {
                let result = engine
                    .execute(Operation::SendText(SendTextInput {
                        text: std::mem::take(&mut *text),
                        target_devices,
                    }))
                    .await
                    .map_err(BindingError::from)
                    .and_then(map_send_report);
                let _ = response.send(result);
            }
            WorkerCommand::SendImage {
                mut bytes,
                mime_type,
                target_devices,
                response,
            } => {
                let result = engine
                    .execute(Operation::SendImage(SendImageInput {
                        bytes: std::mem::take(&mut *bytes),
                        mime_type,
                        target_devices,
                    }))
                    .await
                    .map_err(BindingError::from)
                    .and_then(map_send_report);
                let _ = response.send(result);
            }
            WorkerCommand::SendFiles {
                file_handles,
                target_devices,
                response,
            } => {
                let result = engine
                    .execute(Operation::SendFiles(SendFilesInput {
                        files: file_handles
                            .iter()
                            .map(|handle| HostFileHandle::new(handle.to_owned()))
                            .collect(),
                        target_devices,
                    }))
                    .await
                    .map_err(BindingError::from)
                    .and_then(map_send_report);
                let _ = response.send(result);
            }
            WorkerCommand::CaptureCurrentClipboard { response } => {
                let result = engine
                    .execute(Operation::CaptureCurrentClipboard)
                    .await
                    .map_err(BindingError::from)
                    .and_then(map_clipboard_captured);
                let _ = response.send(result);
            }
            WorkerCommand::RestoreClipboard {
                entry_id,
                mode,
                response,
            } => {
                let result = engine
                    .execute(Operation::RestoreClipboard(RestoreClipboardInput {
                        entry_id,
                        mode: map_restore_mode(mode),
                    }))
                    .await
                    .map_err(BindingError::from)
                    .and_then(map_clipboard_restored);
                let _ = response.send(result);
            }
            WorkerCommand::ExportEntry {
                entry_id,
                destination_handle,
                response,
            } => {
                let result = engine
                    .execute(Operation::ExportEntry(ExportEntryInput {
                        entry_id,
                        destination: HostFileHandle::new(destination_handle.as_str()),
                    }))
                    .await
                    .map_err(BindingError::from)
                    .and_then(map_entry_exported);
                let _ = response.send(result);
            }
            WorkerCommand::Suspend { response } => {
                let result = engine.suspend().await.map_err(BindingError::from);
                let _ = response.send(result);
            }
            WorkerCommand::Resume { response } => {
                let result = engine.resume().await.map_err(BindingError::from);
                let _ = response.send(result);
            }
            WorkerCommand::Shutdown { deadline, response } => {
                let result = engine.shutdown(deadline).await.map_err(BindingError::from);
                shutdown_response = Some((response, result));
                break;
            }
        }
    }
    if shutdown_response.is_none() {
        let _ = engine.shutdown(Duration::ZERO).await;
    }
    let _ = event_task.await;
    if let Some((response, result)) = shutdown_response {
        let _ = response.send(result);
    }
}

fn map_engine_event(event: uc_engine::EngineEvent) -> BindingEvent {
    match event {
        uc_engine::EngineEvent::StateChanged { state } => BindingEvent::StateChanged {
            state: map_engine_state(state),
        },
        uc_engine::EngineEvent::OperationFinished {
            operation_id,
            terminal,
        } => {
            let (terminal, failure) = match terminal {
                uc_engine::OperationTerminal::Succeeded => {
                    (BindingOperationTerminal::Succeeded, None)
                }
                uc_engine::OperationTerminal::Failed(error) => (
                    BindingOperationTerminal::Failed,
                    Some(BindingFailure::from(error)),
                ),
                uc_engine::OperationTerminal::Cancelled => {
                    (BindingOperationTerminal::Cancelled, None)
                }
            };
            BindingEvent::OperationFinished {
                operation_id,
                terminal,
                failure,
            }
        }
        uc_engine::EngineEvent::RefreshRequired { reason } => BindingEvent::RefreshRequired {
            reason: match reason {
                uc_engine::RefreshReason::ConsumerLagged => BindingRefreshReason::ConsumerLagged,
                uc_engine::RefreshReason::StateInvalidated => {
                    BindingRefreshReason::StateInvalidated
                }
            },
        },
        uc_engine::EngineEvent::Fatal { error } => BindingEvent::Fatal {
            failure: BindingFailure::from(error),
        },
        other => BindingEvent::Changed {
            kind: other.kind().to_owned(),
        },
    }
}

fn map_engine_state(state: uc_engine::EngineState) -> BindingEngineState {
    match state {
        uc_engine::EngineState::Running => BindingEngineState::Running,
        uc_engine::EngineState::Quiescing => BindingEngineState::Quiescing,
        uc_engine::EngineState::Quiesced => BindingEngineState::Quiesced,
        uc_engine::EngineState::Suspended => BindingEngineState::Suspended,
        uc_engine::EngineState::ShuttingDown => BindingEngineState::ShuttingDown,
        uc_engine::EngineState::Stopped => BindingEngineState::Stopped,
    }
}

fn map_space_created(result: OperationResult) -> Result<SpaceCreated, BindingError> {
    match result {
        OperationResult::SpaceCreated {
            space_id,
            self_device_id,
            identity_fingerprint,
        } => Ok(SpaceCreated {
            space_id,
            self_device_id,
            identity_fingerprint,
        }),
        _ => Err(BindingError::UnexpectedResult),
    }
}

fn map_invitation_issued(result: OperationResult) -> Result<InvitationIssued, BindingError> {
    match result {
        OperationResult::InvitationIssued {
            invitation_code,
            expires_at_ms,
        } => Ok(InvitationIssued {
            invitation_code,
            expires_at_ms,
        }),
        _ => Err(BindingError::UnexpectedResult),
    }
}

fn map_space_joined(result: OperationResult) -> Result<SpaceJoined, BindingError> {
    match result {
        OperationResult::SpaceJoined {
            sponsor_device_id,
            sponsor_identity_fingerprint,
            space_id,
            self_device_id,
            self_identity_fingerprint,
            migrated_records,
        } => Ok(SpaceJoined {
            sponsor_device_id,
            sponsor_identity_fingerprint,
            space_id,
            self_device_id,
            self_identity_fingerprint,
            migrated_records,
        }),
        _ => Err(BindingError::UnexpectedResult),
    }
}

fn map_send_report(result: OperationResult) -> Result<SendReport, BindingError> {
    match result {
        OperationResult::EntrySent(report) => Ok(SendReport {
            entry_id: report.entry_id,
            at_ms: report.at_ms,
            total_accepted: count_to_u64(report.total_accepted)?,
            total_duplicate: count_to_u64(report.total_duplicate)?,
            total_offline: count_to_u64(report.total_offline)?,
            total_errored: count_to_u64(report.total_errored)?,
            total_pending: count_to_u64(report.total_pending)?,
        }),
        _ => Err(BindingError::UnexpectedResult),
    }
}

fn map_clipboard_captured(result: OperationResult) -> Result<Option<String>, BindingError> {
    match result {
        OperationResult::ClipboardCaptured { entry_id } => Ok(entry_id),
        _ => Err(BindingError::UnexpectedResult),
    }
}

fn map_restore_mode(mode: BindingClipboardRestoreMode) -> ClipboardRestoreMode {
    match mode {
        BindingClipboardRestoreMode::Standard => ClipboardRestoreMode::Standard,
        BindingClipboardRestoreMode::PlainText => ClipboardRestoreMode::PlainText,
        BindingClipboardRestoreMode::FilePaths => ClipboardRestoreMode::FilePaths,
    }
}

fn map_clipboard_restored(
    result: OperationResult,
) -> Result<BindingClipboardRestoreOutcome, BindingError> {
    match result {
        OperationResult::ClipboardRestored(ClipboardRestoreOutcome::Restored) => {
            Ok(BindingClipboardRestoreOutcome::Restored)
        }
        OperationResult::ClipboardRestored(ClipboardRestoreOutcome::PayloadUnavailable {
            ..
        }) => Ok(BindingClipboardRestoreOutcome::PayloadUnavailable),
        OperationResult::ClipboardRestored(ClipboardRestoreOutcome::NotApplicable { .. }) => {
            Ok(BindingClipboardRestoreOutcome::NotApplicable)
        }
        _ => Err(BindingError::UnexpectedResult),
    }
}

fn map_entry_exported(result: OperationResult) -> Result<(), BindingError> {
    match result {
        OperationResult::EntryExported => Ok(()),
        _ => Err(BindingError::UnexpectedResult),
    }
}

fn count_to_u64(value: usize) -> Result<u64, BindingError> {
    u64::try_from(value).map_err(|_| BindingError::UnexpectedResult)
}

fn host_capabilities(host: Arc<dyn BindingHost>) -> Result<HostCapabilities, BindingError> {
    let directories = HostDirectories::new(
        host_path(host.private_data_directory())?,
        host_path(host.cache_directory())?,
        host_path(host.temporary_directory())?,
    );
    for directory in [
        directories.private_data(),
        directories.cache(),
        directories.temporary(),
    ] {
        std::fs::create_dir_all(directory).map_err(|_| BindingError::HostIo)?;
    }
    Ok(HostCapabilities::new(
        directories,
        Box::new(BindingSecureStorage {
            host: Arc::clone(&host),
        }),
        Box::new(BindingClipboard {
            host: Arc::clone(&host),
        }),
        Box::new(BindingFiles { host }),
    ))
}

fn host_path(result: Result<String, HostBindingError>) -> Result<PathBuf, BindingError> {
    result.map(PathBuf::from).map_err(map_binding_host_error)
}

fn map_binding_host_error(error: HostBindingError) -> BindingError {
    match error {
        HostBindingError::Unavailable => BindingError::HostUnavailable,
        HostBindingError::PermissionDenied => BindingError::HostPermissionDenied,
        HostBindingError::InvalidHandle => BindingError::HostInvalidHandle,
        HostBindingError::Io => BindingError::HostIo,
    }
}

fn map_host_capability_error(error: HostBindingError) -> HostCapabilityError {
    let category = match error {
        HostBindingError::Unavailable => HostCapabilityErrorCategory::Unavailable,
        HostBindingError::PermissionDenied => HostCapabilityErrorCategory::PermissionDenied,
        HostBindingError::InvalidHandle => HostCapabilityErrorCategory::InvalidHandle,
        HostBindingError::Io => HostCapabilityErrorCategory::Io,
    };
    HostCapabilityError::new(category, "binding host callback failed")
}

struct BindingSecureStorage {
    host: Arc<dyn BindingHost>,
}

impl HostSecureStorage for BindingSecureStorage {
    fn get(&self, key: &str) -> Result<Option<Vec<u8>>, HostCapabilityError> {
        self.host
            .secure_storage_get(key.to_owned())
            .map_err(map_host_capability_error)
    }

    fn set(&self, key: &str, value: &[u8]) -> Result<(), HostCapabilityError> {
        self.host
            .secure_storage_set(key.to_owned(), value.to_vec())
            .map_err(map_host_capability_error)
    }

    fn delete(&self, key: &str) -> Result<(), HostCapabilityError> {
        self.host
            .secure_storage_delete(key.to_owned())
            .map_err(map_host_capability_error)
    }
}

struct BindingClipboard {
    host: Arc<dyn BindingHost>,
}

impl HostClipboard for BindingClipboard {
    fn read(&self) -> Result<HostClipboardSnapshot, HostCapabilityError> {
        self.host
            .clipboard_read()
            .map(|snapshot| HostClipboardSnapshot {
                observed_at_ms: snapshot.observed_at_ms,
                representations: snapshot
                    .representations
                    .into_iter()
                    .map(map_clipboard_representation)
                    .collect(),
            })
            .map_err(map_host_capability_error)
    }

    fn write(&self, snapshot: HostClipboardSnapshot) -> Result<(), HostCapabilityError> {
        self.host
            .clipboard_write(BindingClipboardSnapshot {
                observed_at_ms: snapshot.observed_at_ms,
                representations: snapshot
                    .representations
                    .into_iter()
                    .map(map_engine_clipboard_representation)
                    .collect(),
            })
            .map_err(map_host_capability_error)
    }
}

fn map_clipboard_representation(
    representation: BindingClipboardRepresentation,
) -> HostClipboardRepresentation {
    match representation {
        BindingClipboardRepresentation::Inline {
            format,
            mime_type,
            bytes,
        } => HostClipboardRepresentation::Inline {
            format,
            mime_type,
            bytes,
        },
        BindingClipboardRepresentation::File {
            format,
            handle,
            display_name,
            mime_type,
            size_bytes,
        } => HostClipboardRepresentation::File {
            format,
            handle: HostFileHandle::new(handle),
            display_name,
            mime_type,
            size_bytes,
        },
    }
}

fn map_engine_clipboard_representation(
    representation: HostClipboardRepresentation,
) -> BindingClipboardRepresentation {
    match representation {
        HostClipboardRepresentation::Inline {
            format,
            mime_type,
            bytes,
        } => BindingClipboardRepresentation::Inline {
            format,
            mime_type,
            bytes,
        },
        HostClipboardRepresentation::File {
            format,
            handle,
            display_name,
            mime_type,
            size_bytes,
        } => BindingClipboardRepresentation::File {
            format,
            handle: handle.as_str().to_owned(),
            display_name,
            mime_type,
            size_bytes,
        },
    }
}

struct BindingFiles {
    host: Arc<dyn BindingHost>,
}

impl HostFileAccess for BindingFiles {
    fn metadata(&self, handle: &HostFileHandle) -> Result<HostFileMetadata, HostCapabilityError> {
        self.host
            .file_metadata(handle.as_str().to_owned())
            .map(|metadata: BindingFileMetadata| HostFileMetadata {
                display_name: metadata.display_name,
                size_bytes: metadata.size_bytes,
                mime_type: metadata.mime_type,
            })
            .map_err(map_host_capability_error)
    }

    fn read_chunk(
        &self,
        handle: &HostFileHandle,
        offset: u64,
        max_bytes: u32,
    ) -> Result<Vec<u8>, HostCapabilityError> {
        self.host
            .file_read_chunk(handle.as_str().to_owned(), offset, max_bytes)
            .map_err(map_host_capability_error)
    }

    fn write_chunk(
        &self,
        handle: &HostFileHandle,
        offset: u64,
        bytes: &[u8],
    ) -> Result<(), HostCapabilityError> {
        self.host
            .file_write_chunk(handle.as_str().to_owned(), offset, bytes.to_vec())
            .map_err(map_host_capability_error)
    }

    fn finish_write(&self, handle: &HostFileHandle) -> Result<(), HostCapabilityError> {
        self.host
            .file_finish_write(handle.as_str().to_owned())
            .map_err(map_host_capability_error)
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(value) => value,
        Err(poisoned) => poisoned.into_inner(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_event_queue_reports_lag_and_keeps_the_latest_events() {
        let queue = EventQueue::new(2);
        queue.push(BindingEvent::Changed {
            kind: "first".to_owned(),
        });
        queue.push(BindingEvent::Changed {
            kind: "second".to_owned(),
        });
        queue.push(BindingEvent::Changed {
            kind: "third".to_owned(),
        });

        assert_eq!(
            queue.next(Duration::ZERO),
            Some(BindingEvent::RefreshRequired {
                reason: BindingRefreshReason::ConsumerLagged,
            })
        );
        assert_eq!(
            queue.next(Duration::ZERO),
            Some(BindingEvent::Changed {
                kind: "second".to_owned(),
            })
        );
        assert_eq!(
            queue.next(Duration::ZERO),
            Some(BindingEvent::Changed {
                kind: "third".to_owned(),
            })
        );
        assert_eq!(queue.next(Duration::ZERO), None);
    }

    #[test]
    fn event_queue_handles_the_largest_foreign_timeout_without_panicking() {
        let queue = EventQueue::new(1);
        queue.close();

        assert_eq!(queue.next(Duration::MAX), None);
    }
}
