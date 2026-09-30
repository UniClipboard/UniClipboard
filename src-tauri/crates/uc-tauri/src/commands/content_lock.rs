//! GUI commands for the content lock. The lock itself lives in the daemon
//! (`uc-webserver` `api/content_lock.rs`): it decides, per request, whether GUI-class clients may
//! read history-derived content. These commands only ask it and tell it the user's choices, so
//! there is no second copy of the state in this process.

use tauri::{AppHandle, Emitter, State};
use tracing::{debug, info, info_span, warn, Instrument};
use uc_daemon_client::DaemonQueryClient;

use super::{record_trace_fields, CommandError, TraceMetadata};

#[derive(serde::Deserialize, specta::Type)]
pub struct ContentUnlockRequest {
    passphrase: String,
}

#[derive(Debug, serde::Serialize, specta::Type)]
#[serde(tag = "code", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ContentUnlockError {
    WrongPassphrase,
    CorruptedKeyMaterial,
    SetupNotCompleted,
    SpaceNotInitialized,
    ProfileRecoveryRequired,
    ProfileRecoveryPartial,
    ProfileRecoveryUnsupported,
    ProfileRecoveryPersistenceFailed,
    Internal,
}

impl ContentUnlockError {
    fn from_daemon(error: anyhow::Error) -> Self {
        let code = error
            .downcast_ref::<uc_daemon_client::http::DaemonRequestError>()
            .and_then(|error| error.code());
        let mapped = match code {
            Some("WRONG_PASSPHRASE") => Self::WrongPassphrase,
            Some("CORRUPTED_KEY_MATERIAL") => Self::CorruptedKeyMaterial,
            Some("SETUP_NOT_COMPLETED") => Self::SetupNotCompleted,
            Some("SPACE_NOT_INITIALIZED") => Self::SpaceNotInitialized,
            Some("PROFILE_RECOVERY_REQUIRED") => Self::ProfileRecoveryRequired,
            Some("PROFILE_RECOVERY_PARTIAL") => Self::ProfileRecoveryPartial,
            Some("PROFILE_RECOVERY_UNSUPPORTED") => Self::ProfileRecoveryUnsupported,
            Some("PROFILE_RECOVERY_PERSISTENCE_FAILED") => Self::ProfileRecoveryPersistenceFailed,
            _ => Self::Internal,
        };
        // Only the stable code crosses this boundary; server text may contain private data.
        warn!(error_code = ?mapped, "Content unlock rejected");
        mapped
    }
}

#[tauri::command]
#[specta::specta]
pub async fn show_content_unlock(app: AppHandle, _trace: Option<TraceMetadata>) {
    let span = info_span!(
        "command.content_lock.show",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty
    );
    record_trace_fields(&span, &_trace);
    async {
        debug!("Opening main window for content authentication");
        crate::main_window::show_main_window(&app);
    }
    .instrument(span)
    .await
}

/// Whether the daemon lets this GUI show content right now.
#[tauri::command]
#[specta::specta]
pub async fn get_content_unlocked(
    query: State<'_, DaemonQueryClient>,
    _trace: Option<TraceMetadata>,
) -> Result<bool, CommandError> {
    let span = info_span!(
        "command.content_lock.status",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty
    );
    record_trace_fields(&span, &_trace);
    async {
        let status = query
            .get_content_lock()
            .await
            .map_err(CommandError::internal)?;
        debug!(unlocked = status.unlocked, "Content lock status queried");
        Ok(status.unlocked)
    }
    .instrument(span)
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn get_profile_recovery(
    query: State<'_, DaemonQueryClient>,
    _trace: Option<TraceMetadata>,
) -> Result<uc_daemon_contract::api::dto::encryption::ProfileRecoveryResponse, CommandError> {
    let span = info_span!(
        "command.profile_recovery.status",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty
    );
    record_trace_fields(&span, &_trace);
    async {
        let result = query
            .get_profile_recovery()
            .await
            .map_err(CommandError::internal)?;
        debug!(state = ?result.state, "Profile recovery status received");
        Ok(result)
    }
    .instrument(span)
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn unlock_content_from_keyring(
    app: AppHandle,
    query: State<'_, DaemonQueryClient>,
    _trace: Option<TraceMetadata>,
) -> Result<bool, CommandError> {
    let span = info_span!(
        "command.content_lock.unlock_from_keyring",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty
    );
    record_trace_fields(&span, &_trace);
    async {
        let status = query
            .unlock_content_from_keyring()
            .await
            .map_err(CommandError::internal)?;
        if !status.unlocked {
            debug!("Keyring did not contain a usable encryption key");
            return Ok(false);
        }
        info!("Content unlocked with the keyring after explicit user action");
        // Events invalidate cached views, never convey authentication authority.
        if let Err(error) = app.emit("content-lock-changed", ()) {
            warn!(error = %error, "Content lock notification failed");
        }
        Ok(true)
    }
    .instrument(span)
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn unlock_content(
    app: AppHandle,
    query: State<'_, DaemonQueryClient>,
    request: ContentUnlockRequest,
    _trace: Option<TraceMetadata>,
) -> Result<(), ContentUnlockError> {
    let span = info_span!(
        "command.content_lock.unlock",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty
    );
    record_trace_fields(&span, &_trace);
    async {
        query
            .unlock_content(&request.passphrase)
            .await
            .map_err(ContentUnlockError::from_daemon)?;
        info!("Content unlocked after passphrase verification");
        if let Err(error) = app.emit("content-lock-changed", ()) {
            warn!(error = %error, "Content lock notification failed");
        }
        Ok(())
    }
    .instrument(span)
    .await
}
