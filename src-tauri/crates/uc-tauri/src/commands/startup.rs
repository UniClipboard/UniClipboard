//! Startup orchestration commands
//! 启动流程编排命令

use std::sync::{Arc, Mutex};

use serde::Serialize;
use tracing::{info_span, Instrument};
use uc_core::ports::observability::TraceMetadata;
use uc_daemon_client::{DaemonClientContext, DaemonConnectionState};
use uc_daemon_contract::api::auth::DaemonConnectionInfo;
use uc_daemon_process::contract::DaemonBootstrapError;

use crate::commands::record_trace_fields;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DaemonConnectionPayload {
    base_url: String,
    ws_url: String,
}

impl From<&DaemonConnectionInfo> for DaemonConnectionPayload {
    fn from(value: &DaemonConnectionInfo) -> Self {
        Self {
            base_url: value.base_url.clone(),
            ws_url: value.ws_url.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DaemonSessionPayload {
    session_token: String,
    #[specta(type = specta_typescript::Number<i64>)]
    expires_in_secs: i64,
    #[specta(type = specta_typescript::Number<i64>)]
    refresh_at_secs: i64,
}

pub fn read_daemon_connection_info(
    state: &DaemonConnectionState,
) -> Option<DaemonConnectionPayload> {
    state.get().as_ref().map(DaemonConnectionPayload::from)
}

/// Read the daemon connection info from managed state.
///
/// Pure status read from managed state; no usecase orchestration is required.
#[tauri::command]
#[specta::specta]
pub async fn get_daemon_connection_info(
    state: tauri::State<'_, DaemonConnectionState>,
    _trace: Option<TraceMetadata>,
) -> Result<Option<DaemonConnectionPayload>, crate::commands::CommandError> {
    let span = info_span!(
        "command.startup.get_daemon_connection_info",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);

    async move { Ok(read_daemon_connection_info(&state)) }
        .instrument(span)
        .await
}

/// Exchange daemon bearer credentials for a short-lived webview session.
///
/// The raw bearer token stays in the native Tauri side; the webview only receives
/// the daemon's session JWT and its refresh metadata.
#[tauri::command]
#[specta::specta]
pub async fn get_daemon_session(
    state: tauri::State<'_, DaemonConnectionState>,
    _trace: Option<TraceMetadata>,
) -> Result<Option<DaemonSessionPayload>, crate::commands::CommandError> {
    let span = info_span!(
        "command.startup.get_daemon_session",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);

    async move {
        let Some(connection_info) = state.get() else {
            return Ok(None);
        };

        let context = DaemonClientContext::with_connection_info(connection_info, "gui".to_string());
        let session = context
            .exchange_session_token(std::process::id(), "gui")
            .await
            .map_err(crate::commands::CommandError::internal)?;

        Ok(Some(DaemonSessionPayload {
            session_token: session.session_token,
            expires_in_secs: session.expires_in_secs,
            refresh_at_secs: session.refresh_at_secs,
        }))
    }
    .instrument(span)
    .await
}

/// Machine-readable classification of a daemon-bootstrap failure, surfaced to
/// the frontend so it can show an actionable message instead of an endless
/// loading screen. `bootstrap_daemon_in_process` only populates the daemon
/// connection state on success; on failure the connection stays unset, so
/// without this signal the frontend's `get_daemon_connection_info` poll would
/// just spin until its own timeout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum DaemonBootstrapFailureKind {
    /// The running daemon is a strictly-newer version than this GUI client.
    /// ADR-008 P4-7 downgrade protection refuses to take it over, so the fix is
    /// to update the app — a distinct kind so the UI can say "update" rather
    /// than "restart".
    VersionTooOld,
    /// Any other terminal bootstrap failure (spawn failure, health-check
    /// timeout, probe error, or an older-or-equal daemon that couldn't be
    /// replaced).
    Unavailable,
}

/// Frontend-facing daemon-bootstrap failure payload. `detail` carries the
/// original error message (English, already user-safe) for diagnostics; the
/// version fields are populated only for
/// [`DaemonBootstrapFailureKind::VersionTooOld`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DaemonBootstrapFailure {
    pub kind: DaemonBootstrapFailureKind,
    pub detail: String,
    pub observed_version: Option<String>,
    pub expected_version: Option<String>,
}

/// Classify a [`DaemonBootstrapError`] into the frontend-facing payload.
///
/// Only `RefusedNewerDaemon` maps to `VersionTooOld` (the one case the user
/// fixes by updating, not restarting); everything else is `Unavailable`.
pub fn classify_bootstrap_failure(error: &DaemonBootstrapError) -> DaemonBootstrapFailure {
    match error {
        DaemonBootstrapError::RefusedNewerDaemon { observed, expected } => DaemonBootstrapFailure {
            kind: DaemonBootstrapFailureKind::VersionTooOld,
            detail: error.to_string(),
            observed_version: Some(observed.clone()),
            expected_version: Some(expected.clone()),
        },
        _ => DaemonBootstrapFailure {
            kind: DaemonBootstrapFailureKind::Unavailable,
            detail: error.to_string(),
            observed_version: None,
            expected_version: None,
        },
    }
}

/// Shared, GUI-process-local record of a terminal daemon-bootstrap failure.
///
/// `bootstrap_daemon_in_process` runs once at startup and only populates the
/// daemon connection state on success. On failure it leaves the connection
/// unset; this state carries the failure reason to the frontend over the same
/// poll loop (via [`get_daemon_bootstrap_failure`]) so the UI can fail fast and
/// explain why, instead of waiting on a connection that will never arrive.
#[derive(Default, Clone)]
pub struct DaemonBootstrapStatus {
    failure: Arc<Mutex<Option<DaemonBootstrapFailure>>>,
}

impl DaemonBootstrapStatus {
    /// Record a terminal bootstrap failure. Overwrites any previous value.
    pub fn record_failure(&self, failure: DaemonBootstrapFailure) {
        if let Ok(mut guard) = self.failure.lock() {
            *guard = Some(failure);
        }
    }

    /// Read the recorded failure, if any.
    pub fn get(&self) -> Option<DaemonBootstrapFailure> {
        self.failure.lock().ok().and_then(|guard| guard.clone())
    }
}

/// Read the recorded daemon-bootstrap failure, if the native bootstrap gave up.
///
/// Returns `None` while bootstrap is still in flight or after it succeeded; the
/// frontend poll treats `Some(_)` as a terminal signal to stop waiting for a
/// connection and surface the error.
///
/// Pure status read from managed state; no usecase orchestration is required.
#[tauri::command]
#[specta::specta]
pub async fn get_daemon_bootstrap_failure(
    state: tauri::State<'_, DaemonBootstrapStatus>,
    _trace: Option<TraceMetadata>,
) -> Result<Option<DaemonBootstrapFailure>, crate::commands::CommandError> {
    let span = info_span!(
        "command.startup.get_daemon_bootstrap_failure",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);

    async move { Ok(state.get()) }.instrument(span).await
}

/// Pending deep-link route recorded by native UI surfaces (tray menu) for the
/// frontend to consume once its router is ready.
///
/// The tray "Settings" item may have to recreate a destroyed main window
/// (destroy-on-close model, see `main_window`); an `app.emit("ui://navigate",
/// ...)` fired at that moment races the fresh webview's listener registration
/// and is lost. The route is recorded here instead: the frontend drains it via
/// [`take_pending_navigation`] during boot, and discards it after consuming a
/// live `ui://navigate` event (the duplicate-record case when the window was
/// already open).
#[derive(Default)]
pub struct PendingNavigation(Mutex<Option<String>>);

impl PendingNavigation {
    /// Record a route for the frontend to pick up. Overwrites any previous one.
    pub fn set(&self, route: &str) {
        if let Ok(mut guard) = self.0.lock() {
            *guard = Some(route.to_string());
        }
    }

    /// Consume the recorded route, if any.
    pub fn take(&self) -> Option<String> {
        self.0.lock().ok().and_then(|mut guard| guard.take())
    }
}

/// Consume the pending deep-link route recorded by native UI surfaces.
///
/// Returns `Some(route)` at most once per recording; the frontend calls this
/// on boot (to honor a deep-link that predates the webview) and after handling
/// a live `ui://navigate` event (to discard the duplicate record).
///
/// Pure state handoff from managed state; no usecase orchestration is required.
#[tauri::command]
#[specta::specta]
pub async fn take_pending_navigation(
    state: tauri::State<'_, PendingNavigation>,
    _trace: Option<TraceMetadata>,
) -> Result<Option<String>, crate::commands::CommandError> {
    let span = info_span!(
        "command.startup.take_pending_navigation",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);

    async move { Ok(state.take()) }.instrument(span).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_refused_newer_daemon_is_version_too_old() {
        let err = DaemonBootstrapError::RefusedNewerDaemon {
            observed: "0.15.0".to_string(),
            expected: "0.14.0".to_string(),
        };
        let failure = classify_bootstrap_failure(&err);
        assert_eq!(failure.kind, DaemonBootstrapFailureKind::VersionTooOld);
        assert_eq!(failure.observed_version.as_deref(), Some("0.15.0"));
        assert_eq!(failure.expected_version.as_deref(), Some("0.14.0"));
        // detail keeps the actionable original message (names both versions).
        assert!(failure.detail.contains("0.15.0") && failure.detail.contains("0.14.0"));
    }

    #[test]
    fn classify_other_errors_are_unavailable_without_versions() {
        let err = DaemonBootstrapError::StartupTimeout { timeout_ms: 8_000 };
        let failure = classify_bootstrap_failure(&err);
        assert_eq!(failure.kind, DaemonBootstrapFailureKind::Unavailable);
        assert!(failure.observed_version.is_none());
        assert!(failure.expected_version.is_none());
        assert!(failure.detail.contains("8000"));
    }

    #[test]
    fn pending_navigation_is_consumed_once() {
        let pending = PendingNavigation::default();
        assert!(pending.take().is_none(), "fresh state must be empty");

        pending.set("/settings");
        assert_eq!(pending.take().as_deref(), Some("/settings"));
        assert!(
            pending.take().is_none(),
            "route must not survive a second take (would leak into the next boot)"
        );
    }

    #[test]
    fn bootstrap_status_records_and_reads_back_failure() {
        let status = DaemonBootstrapStatus::default();
        assert!(status.get().is_none(), "fresh status must have no failure");

        let failure =
            classify_bootstrap_failure(&DaemonBootstrapError::StartupTimeout { timeout_ms: 1_000 });
        status.record_failure(failure.clone());
        assert_eq!(status.get(), Some(failure));
    }
}
