//! Preview panel for displaying full clipboard entry content.
//!
//! Shows a floating panel next to the quick panel with the complete content
//! of a clipboard entry. On macOS, uses NSPanel with NonactivatingPanel
//! so it doesn't steal focus from the quick panel.

#[cfg(target_os = "macos")]
mod macos;

use tauri::{Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use tracing::{debug, error, info, warn};

/// Preview panel dimensions (logical pixels).
const PREVIEW_WIDTH: f64 = 360.0;
const PREVIEW_HEIGHT: f64 = 420.0;

/// Gap between quick panel and preview panel.
const GAP: f64 = 4.0;

/// Quick panel width (must match quick_panel::PANEL_WIDTH).
const QUICK_PANEL_WIDTH: f64 = 360.0;

/// Tauri window label for the preview panel.
const PANEL_LABEL: &str = "preview-panel";

/// Quick panel label (for position queries).
const QUICK_PANEL_LABEL: &str = "quick-panel";

// ── Public API ─────────────────────────────────────────────────────────

/// Pre-create the preview panel window (hidden) during app startup.
pub fn pre_create(app: &tauri::AppHandle) {
    if app.get_webview_window(PANEL_LABEL).is_some() {
        return;
    }

    let url = WebviewUrl::App("preview-panel.html".into());
    match WebviewWindowBuilder::new(app, PANEL_LABEL, url)
        .title("Preview Panel")
        .inner_size(PREVIEW_WIDTH, PREVIEW_HEIGHT)
        .position(-9999.0, -9999.0)
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .visible(false)
        .resizable(false)
        .skip_taskbar(true)
        .build()
    {
        Ok(window) => {
            info!("Preview panel window pre-created");

            #[cfg(target_os = "macos")]
            macos::convert_to_non_key_panel(&window);

            // Preview panel does NOT auto-hide on focus loss.
            // Its lifecycle is managed by the quick panel.
            let _ = window;
        }
        Err(e) => {
            error!(error = %e, "Failed to pre-create preview panel window");
        }
    }
}

/// Show the preview panel next to the quick panel and send the entry ID
/// to the frontend for content loading.
pub fn show(app: &tauri::AppHandle, entry_id: &str) {
    // Don't show preview if the quick panel is not visible (e.g. the 500ms
    // debounce timer fired after the quick panel was already dismissed).
    let quick_panel_visible = app
        .get_webview_window(QUICK_PANEL_LABEL)
        .and_then(|w| w.is_visible().ok())
        .unwrap_or(false);
    if !quick_panel_visible {
        debug!("Quick panel not visible, skipping preview show");
        return;
    }

    // Ensure panel exists
    if app.get_webview_window(PANEL_LABEL).is_none() {
        warn!("Preview panel not pre-created, creating inline");
        pre_create(app);
    }

    let Some(preview_window) = app.get_webview_window(PANEL_LABEL) else {
        error!("Preview panel window not found after creation");
        return;
    };

    // Calculate position based on quick panel location
    let (preview_x, preview_y) = calculate_position(app);

    if let Err(e) = preview_window.set_position(tauri::Position::Logical(
        tauri::LogicalPosition::new(preview_x, preview_y),
    )) {
        warn!(error = %e, "Failed to set preview panel position");
    }

    // Show panel
    #[cfg(target_os = "macos")]
    macos::show_panel(&preview_window);
    #[cfg(not(target_os = "macos"))]
    {
        let _ = preview_window.show();
        // On Windows, show() activates the window and steals focus from the quick
        // panel. Immediately restore focus so the quick panel remains active.
        if let Some(quick_window) = app.get_webview_window(QUICK_PANEL_LABEL) {
            let _ = quick_window.set_focus();
        }
    }

    // Send entry ID to the frontend
    #[derive(Clone, serde::Serialize)]
    #[serde(rename_all = "camelCase")]
    struct ShowPayload {
        entry_id: String,
    }

    if let Err(e) = app.emit_to(
        PANEL_LABEL,
        "preview-panel://show",
        ShowPayload {
            entry_id: entry_id.to_string(),
        },
    ) {
        warn!(error = %e, "Failed to emit show event to preview panel");
    }

    debug!(entry_id, "Preview panel shown");
}

/// Hide the preview panel and notify the frontend to clear content.
pub fn dismiss(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window(PANEL_LABEL) {
        if let Err(e) = app.emit_to(PANEL_LABEL, "preview-panel://hide", ()) {
            warn!(error = %e, "Failed to emit hide event to preview panel");
        }
        let _ = window.hide();
        debug!("Preview panel dismissed");
    }
}

/// Check if the preview panel is currently visible.
pub fn is_visible(app: &tauri::AppHandle) -> bool {
    app.get_webview_window(PANEL_LABEL)
        .and_then(|w| w.is_visible().ok())
        .unwrap_or(false)
}

/// Check if the preview panel currently has focus (is the key window).
pub fn is_focused(app: &tauri::AppHandle) -> bool {
    app.get_webview_window(PANEL_LABEL)
        .and_then(|w| w.is_focused().ok())
        .unwrap_or(false)
}

// ── Position calculation ──────────────────────────────────────────────

/// Calculate the preview panel position based on the quick panel's position.
/// Places the preview to the right of the quick panel, or to the left if
/// there isn't enough screen space.
fn calculate_position(app: &tauri::AppHandle) -> (f64, f64) {
    // Get quick panel position
    let (qp_x, qp_y) = app
        .get_webview_window(QUICK_PANEL_LABEL)
        .and_then(|w| w.outer_position().ok())
        .map(|pos| {
            // Convert physical to logical (assume scale factor 1.0 for simplicity,
            // actual scale factor would need the monitor info)
            let scale = app
                .get_webview_window(QUICK_PANEL_LABEL)
                .and_then(|w| w.scale_factor().ok())
                .unwrap_or(1.0);
            (pos.x as f64 / scale, pos.y as f64 / scale)
        })
        .unwrap_or((540.0, 240.0)); // Fallback center-ish

    // Get screen width to decide left vs right placement
    #[cfg(target_os = "macos")]
    let (screen_width, _) = macos::get_screen_size().unwrap_or_else(|e| {
        warn!(error = %e, "Failed to get screen size, using fallback");
        (1440.0, 900.0)
    });
    #[cfg(not(target_os = "macos"))]
    let (screen_width, _) = (1440.0, 900.0);

    // Try right side first
    let right_x = qp_x + QUICK_PANEL_WIDTH + GAP;
    if right_x + PREVIEW_WIDTH <= screen_width {
        (right_x, qp_y)
    } else {
        // Fall back to left side
        let left_x = qp_x - PREVIEW_WIDTH - GAP;
        (left_x, qp_y)
    }
}

// Tauri commands for preview panel are in commands::preview_panel module.
