//! GUI-only authentication. Background encryption and synchronization are independent.

use tauri::{AppHandle, Emitter, State};
use tokio::sync::Mutex;
use tracing::{debug, info, info_span, warn, Instrument};
use uc_daemon_client::{DaemonConnectionState, DaemonQueryClient, DaemonSettingsClient};

use super::{record_trace_fields, CommandError, TraceMetadata};

#[derive(Default)]
struct ContentGrant {
    value: Option<bool>,
    generation: u64,
}

impl ContentGrant {
    fn replace(&mut self, value: Option<bool>) {
        self.value = value;
        self.generation = self.generation.wrapping_add(1);
    }

    fn replace_if_generation(&mut self, generation: u64, value: Option<bool>) {
        if self.generation == generation {
            self.replace(value);
        }
    }
}

/// A process-local grant shared by every webview; never persisted or set by a webview.
#[derive(Default)]
pub struct ContentLockState(Mutex<ContentGrant>);

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

#[tauri::command]
#[specta::specta]
pub async fn get_content_unlocked(
    state: State<'_, ContentLockState>,
    connection: State<'_, DaemonConnectionState>,
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
        let (grant_generation, cached_grant) = {
            let grant = state.0.lock().await;
            (grant.generation, grant.value)
        };
        if !query
            .get_profile_recovery()
            .await
            .map_err(CommandError::internal)?
            .background_ready
        {
            let mut grant = state.0.lock().await;
            grant.replace_if_generation(grant_generation, None);
            debug!("Content remains hidden during profile recovery");
            return Ok(false);
        }
        let encryption = query
            .get_encryption_state()
            .await
            .map_err(CommandError::internal)?;
        if !encryption.initialized {
            let mut grant = state.0.lock().await;
            grant.replace_if_generation(grant_generation, None);
            debug!("Content remains hidden until setup completes");
            return Ok(false);
        }
        let queried_grant = if cached_grant.is_none() {
            let settings = DaemonSettingsClient::new(connection.inner().clone())
                .map_err(CommandError::internal)?
                .get_settings()
                .await
                .map_err(CommandError::internal)?;
            Some(settings.security.auto_unlock_enabled)
        } else {
            cached_grant
        };
        let mut grant = state.0.lock().await;
        grant.replace_if_generation(grant_generation, queried_grant);
        let unlocked = grant.value.unwrap_or(false) && encryption.session_ready;
        debug!(unlocked, "Content lock status queried");
        Ok(unlocked)
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
    state: State<'_, ContentLockState>,
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
        let resumed = query
            .unlock_encryption()
            .await
            .map_err(CommandError::internal)?;
        if !resumed {
            debug!("Keyring did not contain a usable encryption key");
            return Ok(false);
        }

        state.0.lock().await.replace(Some(true));
        info!("Content unlocked with the keyring after explicit user action");
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
    state: State<'_, ContentLockState>,
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
            .unlock_with_passphrase(&request.passphrase)
            .await
            .map_err(ContentUnlockError::from_daemon)?;
        state.0.lock().await.replace(Some(true));
        info!("Content unlocked after passphrase verification");
        // Events invalidate cached views, never convey authentication authority.
        if let Err(error) = app.emit("content-lock-changed", ()) {
            warn!(error = %error, "Content lock notification failed");
        }
        Ok(())
    }
    .instrument(span)
    .await
}

#[cfg(test)]
mod tests {
    use super::ContentGrant;

    #[test]
    fn replacing_a_grant_advances_its_generation() {
        let mut grant = ContentGrant::default();
        let initial_generation = grant.generation;

        grant.replace(Some(true));

        assert_eq!(grant.value, Some(true));
        assert_ne!(grant.generation, initial_generation);
    }

    #[test]
    fn stale_query_cannot_replace_a_newer_grant() {
        let mut grant = ContentGrant::default();
        let query_generation = grant.generation;
        grant.replace(Some(true));

        grant.replace_if_generation(query_generation, Some(false));

        assert_eq!(grant.value, Some(true));
    }
}
