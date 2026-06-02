//! Startup orchestration commands
//! 启动流程编排命令

use std::sync::atomic::{AtomicBool, Ordering};

use serde::Serialize;
use tauri::AppHandle;
use tracing::{info, info_span, Instrument};
use uc_daemon_client::{DaemonClientContext, DaemonConnectionState};
use uc_daemon_contract::api::auth::DaemonConnectionInfo;
use uc_platform::ports::observability::TraceMetadata;

use crate::commands::record_trace_fields;
use crate::tray::show_main_window;

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

/// Startup barrier used to coordinate backend readiness.
///
/// 用于协调后端就绪的启动门闩。
///
/// # Behavior / 行为
/// - When backend is ready, it shows the main window.
/// - 当后端就绪时，显示主窗口。
#[derive(Default)]
pub struct StartupBarrier {
    backend_ready: AtomicBool,
    finished: AtomicBool,
}

impl StartupBarrier {
    /// Mark the backend as ready.
    ///
    /// 标记后端已就绪。
    pub fn mark_backend_ready(&self) {
        self.backend_ready.store(true, Ordering::SeqCst);
    }

    /// Try to finish startup once (idempotent).
    ///
    /// 尝试完成启动收尾（幂等）。
    pub fn try_finish(&self, app_handle: &AppHandle) {
        if self.finished.load(Ordering::SeqCst) {
            return;
        }

        let backend_ready = self.backend_ready.load(Ordering::SeqCst);
        if !backend_ready {
            info!(backend_ready, "StartupBarrier not ready to finish yet");
            return;
        }

        if self
            .finished
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return;
        }

        show_main_window(app_handle);
        info!("Main window show requested (startup barrier)");
    }
}
