//! Quick-panel Tauri commands
//! 快捷面板相关的 Tauri 命令

use crate::commands::record_trace_fields;
use crate::quick_panel;
use tracing::{info_span, Instrument};
use uc_platform::ports::observability::TraceMetadata;

/// Dismiss the quick panel and return focus to the previous app (no paste).
///
/// 关闭快捷面板并将焦点返回到之前的应用（不粘贴）。
#[tauri::command]
pub async fn dismiss_quick_panel(
    app: tauri::AppHandle,
    _trace: Option<TraceMetadata>,
) -> Result<(), String> {
    let span = info_span!(
        "command.quick_panel.dismiss",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);

    async {
        let handle = app.clone();
        app.run_on_main_thread(move || {
            quick_panel::dismiss(&handle);
        })
        .map_err(|e| format!("Failed to dispatch to main thread: {e}"))?;
        Ok(())
    }
    .instrument(span)
    .await
}

/// Hide the quick panel, re-activate the previous app, and paste.
///
/// 隐藏快捷面板，重新激活之前的应用，并粘贴。
#[tauri::command]
pub async fn paste_to_previous_app(
    app: tauri::AppHandle,
    _trace: Option<TraceMetadata>,
) -> Result<(), String> {
    let span = info_span!(
        "command.quick_panel.paste",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);

    async {
        let handle = app.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        app.run_on_main_thread(move || {
            let result = quick_panel::paste(&handle);
            let _ = tx.send(result);
        })
        .map_err(|e| format!("Failed to dispatch to main thread: {e}"))?;
        rx.await
            .map_err(|_| "Main thread dropped result".to_string())?
    }
    .instrument(span)
    .await
}
