//! Configuration import/export Tauri commands (issue #1110).
//!
//! Thin OS-interaction layer over the daemon's `/config/*` endpoints: this
//! module owns the native save/open file dialogs and forwards the actual
//! packaging / staging work to the daemon over loopback HTTP (the daemon holds
//! the DB + secure storage, so it does the real work — see the design doc
//! §5.5). Passwords flow through here verbatim and MUST never be logged.
//!
//! ## Restart boundary
//!
//! Importing only *stages* the bundle ([`import_config_package`] returns
//! `stagedOk`); the daemon applies it on its next boot. These commands do
//! **not** restart anything. The frontend (Unit 7) owns the restart UX: after a
//! successful stage it shows progress and drives the existing
//! `restart_daemon` → `restart_app` flow (same as `DiagnosticsSettings`). This
//! keeps the command boundary a pure "stage" operation and lets the UI present
//! the irreversible device-identity-move confirmation and progress itself.

use serde::Serialize;
use tauri::AppHandle;
use tauri_plugin_dialog::DialogExt;
use tracing::{info_span, Instrument};
use uc_core::ports::observability::TraceMetadata;
use uc_daemon_client::{DaemonConfigClient, DaemonConnectionState, DaemonRequestError};

use crate::commands::record_trace_fields;

/// Default file name suggested in the export save dialog.
const DEFAULT_BUNDLE_FILE_NAME: &str = "uniclipboard-config.ucbundle";
/// Bundle extension (without the leading dot) used for dialog filters.
const BUNDLE_EXTENSION: &str = "ucbundle";

/// Typed error for the config import/export commands.
///
/// Serializes to a discriminated union `{ kind, ... }` so the frontend (Unit 7)
/// can branch the import/export UX without scraping message strings. The
/// `Daemon` variant preserves the daemon's stable error `code` token
/// (`LOCKED` / `NOT_INITIALIZED` / `INVALID_PASSWORD_OR_CORRUPT` /
/// `INCOMPATIBLE_BUNDLE` / `confirmation_required` / `IO` / `INTERNAL`) parsed
/// from the canonical
/// `{ code, message }` error body, which is exactly what the daemon's config
/// endpoints emit. `message` is never logged with secrets — these come from the
/// daemon's already-redacted error text.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ConfigCommandError {
    /// The user cancelled the native save/open file dialog. Not an error
    /// condition the UI needs to surface as a failure.
    Cancelled,
    /// The daemon rejected the request. `code` is the daemon's stable token the
    /// frontend switches on; `status` is the HTTP status for coarse grouping.
    Daemon {
        status: u16,
        code: Option<String>,
        message: String,
    },
    /// Transport / authorization / decode failure talking to the daemon, or a
    /// local OS fault. No stable code; surface a generic failure to the user.
    Internal { message: String },
}

impl From<DaemonRequestError> for ConfigCommandError {
    fn from(err: DaemonRequestError) -> Self {
        match err {
            DaemonRequestError::Status {
                status,
                code,
                message,
                ..
            } => ConfigCommandError::Daemon {
                status: status.as_u16(),
                code,
                message,
            },
            other => ConfigCommandError::Internal {
                message: other.to_string(),
            },
        }
    }
}

/// Result of [`export_config_package`].
///
/// Local specta-typed mirror of the daemon-contract `ExportConfigResponse`
/// (the contract crate derives serde/utoipa, not `specta::Type`; the Tauri
/// boundary owns its own wire types — same pattern as `settings.rs`).
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ExportConfigResult {
    /// Absolute path the bundle was written to.
    pub path: String,
}

/// Descriptive (non-secret) metadata read from a bundle manifest by
/// [`preview_config_import`]. Local specta-typed mirror of the daemon-contract
/// `PreviewImportResponse`.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ConfigImportPreview {
    /// Application version string of the installation that produced the bundle.
    pub app_version: String,
    /// Storage layout the bundle was produced under (`portable` / `installed`).
    pub source_mode: String,
    /// Bundle creation time, milliseconds since the Unix epoch. Exported to TS
    /// as `number` (well within `Number.MAX_SAFE_INTEGER`); see the project
    /// convention in `startup.rs` / `tests/specta_export.rs`.
    #[specta(type = specta_typescript::Number<i64>)]
    pub created_at_unix_ms: i64,
    /// Profile the bundle's configuration belongs to.
    pub profile_id: String,
    /// Stable identity fingerprint of the producing device. Adopting this
    /// bundle makes the target device present itself under this same identity.
    pub device_fingerprint: String,
}

/// Show the native "save" dialog and return the chosen absolute path, or `None`
/// when the user cancels. Runs the blocking dialog off the Tauri main thread.
async fn pick_save_path(app: AppHandle) -> Result<Option<String>, ConfigCommandError> {
    let result = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_file_name(DEFAULT_BUNDLE_FILE_NAME)
            .add_filter("UniClipboard config bundle", &[BUNDLE_EXTENSION])
            .blocking_save_file()
    })
    .await
    .map_err(|e| ConfigCommandError::Internal {
        message: format!("save dialog task failed to join: {e}"),
    })?;

    map_dialog_path(result)
}

/// Show the native "open" dialog and return the chosen absolute path, or `None`
/// when the user cancels. Runs the blocking dialog off the Tauri main thread.
async fn pick_open_path(app: AppHandle) -> Result<Option<String>, ConfigCommandError> {
    let result = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .add_filter("UniClipboard config bundle", &[BUNDLE_EXTENSION])
            .blocking_pick_file()
    })
    .await
    .map_err(|e| ConfigCommandError::Internal {
        message: format!("open dialog task failed to join: {e}"),
    })?;

    map_dialog_path(result)
}

/// Resolve a dialog `FilePath` selection into an absolute filesystem string.
/// `None` (user cancelled) maps to `Ok(None)`.
fn map_dialog_path(
    selection: Option<tauri_plugin_dialog::FilePath>,
) -> Result<Option<String>, ConfigCommandError> {
    match selection {
        None => Ok(None),
        Some(file_path) => {
            let path = file_path
                .into_path()
                .map_err(|e| ConfigCommandError::Internal {
                    message: format!("selected path is not a local file: {e}"),
                })?;
            Ok(Some(path.to_string_lossy().into_owned()))
        }
    }
}

/// Prompt for a save location and export the current configuration to an
/// encrypted `.ucbundle` there.
///
/// Pops a native save dialog (default name `uniclipboard-config.ucbundle`); if
/// the user cancels, returns [`ConfigCommandError::Cancelled`]. Otherwise calls
/// the daemon `POST /config/export` with the chosen `target_path`, returning the
/// absolute path the bundle landed at. No export password is taken — the daemon
/// seals the bundle with the installation's own key material (opening it later
/// needs the space passphrase). Requires an unlocked session (the daemon
/// enforces this; a locked session surfaces as a `Daemon { code: "LOCKED" }`
/// error).
#[tauri::command]
#[specta::specta]
pub async fn export_config_package(
    app: AppHandle,
    connection_state: tauri::State<'_, DaemonConnectionState>,
    _trace: Option<TraceMetadata>,
) -> Result<ExportConfigResult, ConfigCommandError> {
    let span = info_span!(
        "command.config.export_config_package",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);

    async move {
        let target_path = match pick_save_path(app).await? {
            Some(path) => path,
            None => return Err(ConfigCommandError::Cancelled),
        };

        let client = DaemonConfigClient::new(connection_state.inner().clone());
        let response = client.export(target_path).await?;
        tracing::info!("config export bundle written");
        Ok(ExportConfigResult {
            path: response.path,
        })
    }
    .instrument(span)
    .await
}

/// Show the native open dialog and return the chosen `.ucbundle` path.
///
/// Returns `Some(absolute_path)` on selection, `None` when the user cancels.
/// Separated from [`preview_config_import`] so the UI can drive
/// "pick file → preview → confirm" as distinct steps.
#[tauri::command]
#[specta::specta]
pub async fn pick_config_bundle_path(
    app: AppHandle,
    _trace: Option<TraceMetadata>,
) -> Result<Option<String>, ConfigCommandError> {
    let span = info_span!(
        "command.config.pick_config_bundle_path",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);

    async move { pick_open_path(app).await }
        .instrument(span)
        .await
}

/// Decrypt only the bundle manifest at `source_path` and return its non-secret
/// descriptive metadata for operator confirmation (app version, source mode,
/// creation time, profile, device fingerprint).
///
/// Read-only: stages nothing. The UI uses this to show the irreversible
/// device-identity-move confirmation before [`import_config_package`].
#[tauri::command]
#[specta::specta]
pub async fn preview_config_import(
    connection_state: tauri::State<'_, DaemonConnectionState>,
    password: String,
    source_path: String,
    _trace: Option<TraceMetadata>,
) -> Result<ConfigImportPreview, ConfigCommandError> {
    let span = info_span!(
        "command.config.preview_config_import",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);

    async move {
        let client = DaemonConfigClient::new(connection_state.inner().clone());
        // `password` is moved into the request body and never logged.
        let preview = client.preview_import(password, source_path).await?;
        Ok(ConfigImportPreview {
            app_version: preview.app_version,
            source_mode: preview.source_mode,
            created_at_unix_ms: preview.created_at_unix_ms,
            profile_id: preview.profile_id,
            device_fingerprint: preview.device_fingerprint,
        })
    }
    .instrument(span)
    .await
}

/// Result of [`import_config_package`].
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ImportConfigStageResult {
    /// Always `true` on success: the bundle was validated and staged.
    pub staged_ok: bool,
    /// `true` when applying the staged migration on the next boot will require
    /// the operator to re-enter their passphrase to unlock afterwards.
    pub unlock_required_after_apply: bool,
}

/// Validate the bundle at `source_path` and stage it for the next daemon boot
/// to apply (`confirmed = true` is sent unconditionally — the caller is
/// expected to have shown the device-identity-move confirmation already).
///
/// This only stages: it does NOT restart anything. On success the frontend
/// drives the restart flow (`restart_daemon` → `restart_app`) so the staged
/// migration is applied on boot. Applying replaces whatever configuration the
/// target currently holds — there is no already-initialized rejection.
#[tauri::command]
#[specta::specta]
pub async fn import_config_package(
    connection_state: tauri::State<'_, DaemonConnectionState>,
    password: String,
    source_path: String,
    _trace: Option<TraceMetadata>,
) -> Result<ImportConfigStageResult, ConfigCommandError> {
    let span = info_span!(
        "command.config.import_config_package",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);

    async move {
        let client = DaemonConfigClient::new(connection_state.inner().clone());
        // `password` is moved into the request body and never logged. `confirmed`
        // is always true: the UI performs the explicit confirmation gate.
        let response = client.import(password, source_path, true).await?;
        tracing::info!(
            unlock_required_after_apply = response.unlock_required_after_apply,
            "config import staged for next boot"
        );
        Ok(ImportConfigStageResult {
            staged_ok: response.staged_ok,
            unlock_required_after_apply: response.unlock_required_after_apply,
        })
    }
    .instrument(span)
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_status_daemon_error_maps_to_internal() {
        let err: ConfigCommandError = DaemonRequestError::NotConnected.into();
        assert!(matches!(err, ConfigCommandError::Internal { .. }));
    }

    #[test]
    fn error_serializes_as_tagged_camel_case_union() {
        let json = serde_json::to_value(ConfigCommandError::Cancelled).expect("serializes");
        assert_eq!(json["kind"], "cancelled");

        let json = serde_json::to_value(ConfigCommandError::Daemon {
            status: 423,
            code: Some("LOCKED".to_string()),
            message: "locked".to_string(),
        })
        .expect("serializes");
        assert_eq!(json["kind"], "daemon");
        assert_eq!(json["status"], 423);
        assert_eq!(json["code"], "LOCKED");
    }

    #[test]
    fn stage_result_serializes_camel_case() {
        let json = serde_json::to_value(ImportConfigStageResult {
            staged_ok: true,
            unlock_required_after_apply: false,
        })
        .expect("serializes");
        assert_eq!(json["stagedOk"], true);
        assert_eq!(json["unlockRequiredAfterApply"], false);
        assert!(json.get("staged_ok").is_none());
    }
}
