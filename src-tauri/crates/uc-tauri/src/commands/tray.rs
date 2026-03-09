//! Tray-related Tauri commands

use crate::commands::record_trace_fields;
use crate::tray::TrayState;
use tauri::State;
use tracing::info_span;
use uc_core::ports::observability::TraceMetadata;

/// Update tray menu labels to match the UI language.
///
/// This is a UI sync command only; it does not persist settings.
#[tauri::command]
pub async fn set_tray_language(
    tray: State<'_, TrayState>,
    language: String,
    _trace: Option<TraceMetadata>,
) -> Result<(), String> {
    let span = info_span!(
        "command.tray.set_language",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
        language = %language,
    );
    record_trace_fields(&span, &_trace);
    let _guard = span.enter();

    tray.set_language(&language).map_err(|e| {
        tracing::error!(error = %e, "Failed to set tray language");
        e.to_string()
    })
}
