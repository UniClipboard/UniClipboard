use std::sync::Arc;
use std::time::Duration;

use napi::bindgen_prelude::Buffer;
use napi::Status;
use napi_derive::napi;
use uc_engine::{
    ClipboardRestoreMode, ClipboardRestoreOutcome, CreateSpaceInput, Engine, EngineConfig,
    EngineError, EngineEvent, EngineState, EventStream, ExportEntryInput, HostFileHandle,
    InvitationAvailability, JoinSpaceInput, Operation, OperationResult, OperationTerminal,
    RecoverSessionInput, RefreshReason, RestoreClipboardInput, SecretString, SendFilesInput,
    SendImageInput, SendReportSummary, SendTextInput,
};
use zeroize::Zeroizing;

use crate::{
    host, OhEngineConfig, OhEngineEvent, OhHost, OhInvitationIssued, OhLocalDevice, OhSendReport,
    OhSessionRecovery, OhSpaceCreated, OhSpaceJoined,
};

#[napi]
pub struct OhEngine {
    engine: Arc<Engine>,
    events: tokio::sync::Mutex<EventStream>,
}

impl OhEngine {
    pub(crate) async fn start(config: OhEngineConfig, host: OhHost) -> napi::Result<Self> {
        let capabilities = host::capabilities(host)?;
        let config = EngineConfig::new(config.app_version).with_profile_id(config.profile_id);
        let (engine, events) = Engine::start(config, capabilities)
            .await
            .map_err(engine_error)?;
        Ok(Self {
            engine: Arc::new(engine),
            events: tokio::sync::Mutex::new(events),
        })
    }
}

#[napi]
impl OhEngine {
    #[napi]
    pub async fn create_space(
        &self,
        device_name: Option<String>,
        passphrase: String,
    ) -> napi::Result<OhSpaceCreated> {
        let passphrase = Zeroizing::new(passphrase);
        let result = self
            .engine
            .execute(Operation::CreateSpace(CreateSpaceInput {
                device_name,
                passphrase: SecretString::new(passphrase.as_str()),
                passphrase_confirmation: SecretString::new(passphrase.as_str()),
            }))
            .await
            .map_err(engine_error)?;
        match result {
            OperationResult::SpaceCreated {
                space_id,
                self_device_id,
                identity_fingerprint,
            } => Ok(OhSpaceCreated {
                space_id,
                self_device_id,
                identity_fingerprint,
            }),
            _ => Err(unexpected_result()),
        }
    }

    #[napi]
    pub async fn recover_session(
        &self,
        allow_secure_storage_unlock: bool,
    ) -> napi::Result<OhSessionRecovery> {
        let result = self
            .engine
            .execute(Operation::RecoverSession(RecoverSessionInput {
                allow_secure_storage_unlock,
            }))
            .await
            .map_err(engine_error)?;
        match result {
            OperationResult::SessionRecovered { unlocked, resumed } => {
                Ok(OhSessionRecovery { unlocked, resumed })
            }
            _ => Err(unexpected_result()),
        }
    }

    #[napi]
    pub async fn query_local_device(&self) -> napi::Result<OhLocalDevice> {
        let result = self
            .engine
            .execute(Operation::QueryLocalDevice)
            .await
            .map_err(engine_error)?;
        match result {
            OperationResult::LocalDevice(device) => Ok(OhLocalDevice {
                device_id: device.device_id,
                display_name: device.display_name,
            }),
            _ => Err(unexpected_result()),
        }
    }

    #[napi]
    pub async fn issue_invitation(&self) -> napi::Result<OhInvitationIssued> {
        let result = self
            .engine
            .execute(Operation::IssueInvitation)
            .await
            .map_err(engine_error)?;
        match result {
            OperationResult::InvitationIssued {
                invitation_code,
                expires_at_ms,
                availability,
            } => Ok(OhInvitationIssued {
                invitation_code,
                expires_at_ms: expires_at_ms as f64,
                availability: invitation_availability(availability).to_owned(),
            }),
            _ => Err(unexpected_result()),
        }
    }

    #[napi]
    pub async fn join_space(
        &self,
        invitation_code: String,
        device_name: Option<String>,
        passphrase: String,
    ) -> napi::Result<OhSpaceJoined> {
        let invitation_code = Zeroizing::new(invitation_code);
        let passphrase = Zeroizing::new(passphrase);
        let result = self
            .engine
            .execute(Operation::JoinSpace(JoinSpaceInput {
                invitation_code: invitation_code.to_string(),
                device_name,
                passphrase: SecretString::new(passphrase.as_str()),
            }))
            .await
            .map_err(engine_error)?;
        match result {
            OperationResult::SpaceJoined {
                sponsor_device_id,
                sponsor_identity_fingerprint,
                space_id,
                self_device_id,
                self_identity_fingerprint,
                migrated_records,
            } => Ok(OhSpaceJoined {
                sponsor_device_id,
                sponsor_identity_fingerprint,
                space_id,
                self_device_id,
                self_identity_fingerprint,
                migrated_records: migrated_records.map(|count| count.to_string()),
            }),
            _ => Err(unexpected_result()),
        }
    }

    #[napi]
    pub async fn send_text(
        &self,
        text: String,
        target_devices: Vec<String>,
    ) -> napi::Result<OhSendReport> {
        let text = Zeroizing::new(text);
        let result = self
            .engine
            .execute(Operation::SendText(SendTextInput {
                text: text.to_string(),
                target_devices,
            }))
            .await
            .map_err(engine_error)?;
        match result {
            OperationResult::EntrySent(report) => send_report(report),
            _ => Err(unexpected_result()),
        }
    }

    #[napi]
    pub async fn send_image(
        &self,
        bytes: Buffer,
        mime_type: String,
        target_devices: Vec<String>,
    ) -> napi::Result<OhSendReport> {
        let bytes = Zeroizing::new(bytes.to_vec());
        let result = self
            .engine
            .execute(Operation::SendImage(SendImageInput {
                bytes: bytes.to_vec(),
                mime_type,
                target_devices,
            }))
            .await
            .map_err(engine_error)?;
        match result {
            OperationResult::EntrySent(report) => send_report(report),
            _ => Err(unexpected_result()),
        }
    }

    #[napi]
    pub async fn send_files(
        &self,
        file_handles: Vec<String>,
        target_devices: Vec<String>,
    ) -> napi::Result<OhSendReport> {
        let file_handles = Zeroizing::new(file_handles);
        let result = self
            .engine
            .execute(Operation::SendFiles(SendFilesInput {
                files: file_handles
                    .iter()
                    .cloned()
                    .map(HostFileHandle::new)
                    .collect(),
                target_devices,
            }))
            .await
            .map_err(engine_error)?;
        match result {
            OperationResult::EntrySent(report) => send_report(report),
            _ => Err(unexpected_result()),
        }
    }

    #[napi]
    pub async fn capture_current_clipboard(&self) -> napi::Result<Option<String>> {
        let result = self
            .engine
            .execute(Operation::CaptureCurrentClipboard)
            .await
            .map_err(engine_error)?;
        match result {
            OperationResult::ClipboardCaptured { entry_id } => Ok(entry_id),
            _ => Err(unexpected_result()),
        }
    }

    #[napi]
    pub async fn restore_clipboard(&self, entry_id: String, mode: String) -> napi::Result<String> {
        let mode = match mode.as_str() {
            "standard" => ClipboardRestoreMode::Standard,
            "plain_text" => ClipboardRestoreMode::PlainText,
            "file_paths" => ClipboardRestoreMode::FilePaths,
            _ => return Err(invalid_restore_mode()),
        };
        let result = self
            .engine
            .execute(Operation::RestoreClipboard(RestoreClipboardInput {
                entry_id,
                mode,
            }))
            .await
            .map_err(engine_error)?;
        match result {
            OperationResult::ClipboardRestored(ClipboardRestoreOutcome::Restored) => {
                Ok("restored".to_owned())
            }
            OperationResult::ClipboardRestored(ClipboardRestoreOutcome::PayloadUnavailable {
                ..
            }) => Ok("payload_unavailable".to_owned()),
            OperationResult::ClipboardRestored(ClipboardRestoreOutcome::NotApplicable {
                ..
            }) => Ok("not_applicable".to_owned()),
            _ => Err(unexpected_result()),
        }
    }

    #[napi]
    pub async fn export_entry(
        &self,
        entry_id: String,
        destination_handle: String,
    ) -> napi::Result<()> {
        let destination_handle = Zeroizing::new(destination_handle);
        let result = self
            .engine
            .execute(Operation::ExportEntry(ExportEntryInput {
                entry_id,
                destination: HostFileHandle::new(destination_handle.to_string()),
            }))
            .await
            .map_err(engine_error)?;
        match result {
            OperationResult::EntryExported => Ok(()),
            _ => Err(unexpected_result()),
        }
    }

    #[napi]
    pub async fn suspend(&self) -> napi::Result<()> {
        self.engine.suspend().await.map_err(engine_error)
    }

    #[napi]
    pub async fn lifecycle_state(&self) -> String {
        engine_state(self.engine.lifecycle_state().await).to_owned()
    }

    #[napi]
    pub async fn resume(&self) -> napi::Result<()> {
        self.engine.resume().await.map_err(engine_error)
    }

    #[napi]
    pub async fn next_event(&self, timeout_ms: u32) -> napi::Result<Option<OhEngineEvent>> {
        let mut events = self.events.lock().await;
        match tokio::time::timeout(Duration::from_millis(u64::from(timeout_ms)), events.next())
            .await
        {
            Ok(Some(event)) => Ok(Some(map_event(event))),
            Ok(None) | Err(_) => Ok(None),
        }
    }

    #[napi]
    pub async fn shutdown(&self, deadline_ms: u32) -> napi::Result<()> {
        self.engine
            .shutdown(Duration::from_millis(u64::from(deadline_ms)))
            .await
            .map_err(engine_error)
    }
}

fn engine_error(error: EngineError) -> napi::Error {
    napi::Error::new(
        Status::GenericFailure,
        format!(
            "UC_ENGINE:{}:{}:{}",
            error.code(),
            error.category(),
            error.is_retryable()
        ),
    )
}

fn invitation_availability(availability: InvitationAvailability) -> &'static str {
    match availability {
        InvitationAvailability::CrossNetwork => "cross_network",
        InvitationAvailability::SameLocalNetwork => "same_local_network",
    }
}

fn send_report(report: SendReportSummary) -> napi::Result<OhSendReport> {
    Ok(OhSendReport {
        entry_id: report.entry_id,
        at_ms: report.at_ms as f64,
        total_accepted: count(report.total_accepted)?,
        total_duplicate: count(report.total_duplicate)?,
        total_offline: count(report.total_offline)?,
        total_errored: count(report.total_errored)?,
        total_pending: count(report.total_pending)?,
    })
}

fn count(value: usize) -> napi::Result<u32> {
    u32::try_from(value).map_err(|_| unexpected_result())
}

fn map_event(event: EngineEvent) -> OhEngineEvent {
    let kind = event.kind().to_owned();
    let mut mapped = OhEngineEvent {
        kind,
        state: None,
        refresh_reason: None,
        operation_id: None,
        terminal: None,
        lifecycle_action: None,
        error_code: None,
        error_category: None,
        retryable: None,
    };
    match event {
        EngineEvent::StateChanged { state } => mapped.state = Some(engine_state(state).to_owned()),
        EngineEvent::RefreshRequired { reason } => {
            mapped.refresh_reason = Some(refresh_reason(reason).to_owned());
        }
        EngineEvent::OperationFinished {
            operation_id,
            terminal,
        } => {
            mapped.operation_id = Some(operation_id);
            map_terminal(terminal, &mut mapped);
        }
        EngineEvent::LifecycleFailed { action, error } => {
            mapped.lifecycle_action = Some(
                match action {
                    uc_engine::LifecycleAction::Suspend => "suspend",
                    uc_engine::LifecycleAction::Resume => "resume",
                }
                .to_owned(),
            );
            map_event_error(error, &mut mapped);
        }
        EngineEvent::Fatal { error } => map_event_error(error, &mut mapped),
        _ => {}
    }
    mapped
}

fn engine_state(state: EngineState) -> &'static str {
    match state {
        EngineState::Running => "running",
        EngineState::Quiescing => "quiescing",
        EngineState::Quiesced => "quiesced",
        EngineState::Suspended => "suspended",
        EngineState::ShuttingDown => "shutting_down",
        EngineState::Stopped => "stopped",
    }
}

fn refresh_reason(reason: RefreshReason) -> &'static str {
    match reason {
        RefreshReason::ConsumerLagged => "consumer_lagged",
        RefreshReason::StateInvalidated => "state_invalidated",
    }
}

fn map_terminal(terminal: OperationTerminal, mapped: &mut OhEngineEvent) {
    match terminal {
        OperationTerminal::Succeeded => mapped.terminal = Some("succeeded".to_owned()),
        OperationTerminal::Cancelled => mapped.terminal = Some("cancelled".to_owned()),
        OperationTerminal::Failed(error) => {
            mapped.terminal = Some("failed".to_owned());
            map_event_error(error, mapped);
        }
    }
}

fn map_event_error(error: EngineError, mapped: &mut OhEngineEvent) {
    mapped.error_code = Some(error.code());
    mapped.error_category = Some(error.category().to_string());
    mapped.retryable = Some(error.is_retryable());
}

fn unexpected_result() -> napi::Error {
    napi::Error::new(Status::GenericFailure, "UC_ENGINE:UNEXPECTED_RESULT")
}

fn invalid_restore_mode() -> napi::Error {
    napi::Error::new(Status::InvalidArg, "OHOS_INVALID_CLIPBOARD_RESTORE_MODE")
}

#[cfg(test)]
mod tests {
    use uc_engine::{
        EngineError, EngineErrorCategory, EngineEvent, OperationTerminal, RefreshReason,
    };

    use super::{count, map_event};

    #[test]
    fn refresh_event_keeps_only_the_stable_reason() {
        let event = map_event(EngineEvent::RefreshRequired {
            reason: RefreshReason::ConsumerLagged,
        });

        assert_eq!(event.kind, "refresh_required");
        assert_eq!(event.refresh_reason.as_deref(), Some("consumer_lagged"));
        assert_eq!(event.operation_id, None);
        assert_eq!(event.error_code, None);
    }

    #[test]
    fn failed_operation_event_keeps_only_the_stable_error_summary() {
        let event = map_event(EngineEvent::OperationFinished {
            operation_id: "operation-1".to_owned(),
            terminal: OperationTerminal::Failed(EngineError::new(
                1214,
                EngineErrorCategory::Unavailable,
                true,
            )),
        });

        assert_eq!(event.kind, "operation_finished");
        assert_eq!(event.operation_id.as_deref(), Some("operation-1"));
        assert_eq!(event.terminal.as_deref(), Some("failed"));
        assert_eq!(event.error_code, Some(1214));
        assert_eq!(event.error_category.as_deref(), Some("unavailable"));
        assert_eq!(event.retryable, Some(true));
    }

    #[test]
    fn lifecycle_failure_event_keeps_the_action_and_stable_error_summary() {
        let event = map_event(EngineEvent::LifecycleFailed {
            action: uc_engine::LifecycleAction::Resume,
            error: EngineError::new(1214, EngineErrorCategory::Unavailable, true),
        });

        assert_eq!(event.kind, "lifecycle_failed");
        assert_eq!(event.lifecycle_action.as_deref(), Some("resume"));
        assert_eq!(event.error_code, Some(1214));
        assert_eq!(event.error_category.as_deref(), Some("unavailable"));
        assert_eq!(event.retryable, Some(true));
    }

    #[test]
    fn oversized_delivery_counts_are_rejected() {
        assert!(count(usize::MAX).is_err());
    }
}
