//! Preview-panel Tauri commands
//! 预览面板相关的 Tauri 命令

use crate::commands::record_trace_fields;
use crate::preview_panel;
use tracing::{info_span, Instrument};
use uc_platform::ports::observability::TraceMetadata;

/// Show the preview panel with the specified entry's content.
///
/// 显示预览面板并展示指定条目的内容。
#[tauri::command]
pub async fn show_preview_panel(
    app: tauri::AppHandle,
    entry_id: String,
    _trace: Option<TraceMetadata>,
) -> Result<(), String> {
    let span = info_span!(
        "command.preview_panel.show",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
        entry_id = %entry_id,
    );
    record_trace_fields(&span, &_trace);

    async {
        let handle = app.clone();
        app.run_on_main_thread(move || {
            preview_panel::show(&handle, &entry_id);
        })
        .map_err(|e| format!("Failed to dispatch to main thread: {e}"))?;
        Ok(())
    }
    .instrument(span)
    .await
}

/// Hide the preview panel.
///
/// 隐藏预览面板。
#[tauri::command]
pub async fn dismiss_preview_panel(
    app: tauri::AppHandle,
    _trace: Option<TraceMetadata>,
) -> Result<(), String> {
    let span = info_span!(
        "command.preview_panel.dismiss",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);

    async {
        let handle = app.clone();
        app.run_on_main_thread(move || {
            preview_panel::dismiss(&handle);
        })
        .map_err(|e| format!("Failed to dispatch to main thread: {e}"))?;
        Ok(())
    }
    .instrument(span)
    .await
}
