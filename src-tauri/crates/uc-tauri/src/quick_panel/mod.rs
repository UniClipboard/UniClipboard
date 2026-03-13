//! Cross-platform quick clipboard panel.
//!
//! Provides a Spotlight-like floating panel for clipboard history.
//! On macOS, the panel uses NSPanel with `NonactivatingPanel` so the
//! previously focused application stays frontmost — no PID tracking needed.
//!
//! 跨平台快捷剪贴板面板。macOS 上使用 NSPanel，不会抢夺前台应用焦点。

#[cfg(target_os = "macos")]
mod macos;

use tauri::{Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use tracing::{debug, error, info, warn};

/// Panel dimensions (logical pixels).
const PANEL_WIDTH: f64 = 360.0;
const PANEL_HEIGHT: f64 = 420.0;

/// Tauri window label for the quick panel.
const PANEL_LABEL: &str = "quick-panel";

// ── Cross-platform helpers ─────────────────────────────────────────────

/// Get screen center position for the panel (top-left corner of the panel
/// such that it appears centered on screen, like Raycast/Spotlight).
///
/// 获取面板在屏幕居中时的左上角坐标（类似 Raycast/Spotlight 的位置）。
fn screen_center_position() -> (f64, f64) {
    #[cfg(target_os = "macos")]
    {
        macos::get_screen_center(PANEL_WIDTH, PANEL_HEIGHT)
    }
    #[cfg(not(target_os = "macos"))]
    {
        // Fallback: rough center assuming 1440x900
        ((1440.0 - PANEL_WIDTH) / 2.0, (900.0 - PANEL_HEIGHT) / 2.0)
    }
}

// ── Public API ─────────────────────────────────────────────────────────

/// Pre-create the quick panel window (hidden) during app startup.
///
/// This avoids the first-invocation activation problem: `WebviewWindowBuilder::build()`
/// creates a regular NSWindow which activates the app. By pre-creating and converting
/// to NSPanel at startup, the first shortcut press follows the same "already exists"
/// path as subsequent presses.
///
/// 在应用启动时预创建快捷面板（隐藏状态），避免首次调用时激活应用。
pub fn pre_create(app: &tauri::AppHandle) {
    if app.get_webview_window(PANEL_LABEL).is_some() {
        return; // Already created
    }

    // Position off-screen; will be repositioned on first show()
    let url = WebviewUrl::App("quick-panel.html".into());
    match WebviewWindowBuilder::new(app, PANEL_LABEL, url)
        .title("Quick Panel")
        .inner_size(PANEL_WIDTH, PANEL_HEIGHT)
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
            info!("Quick panel window pre-created");

            #[cfg(target_os = "macos")]
            macos::convert_to_panel(&window);

            // Auto-hide when the panel loses focus (user clicks elsewhere).
            // If focus went to the preview panel, keep the quick panel visible;
            // otherwise dismiss both panels.
            let win_clone = window.clone();
            let app_for_focus = app.clone();
            window.on_window_event(move |event| {
                if let tauri::WindowEvent::Focused(false) = event {
                    // If focus transferred to the preview panel, keep quick panel visible
                    if crate::preview_panel::is_focused(&app_for_focus) {
                        debug!("Quick panel lost focus to preview panel — not hiding");
                        return;
                    }
                    debug!("Quick panel lost focus, hiding");
                    crate::preview_panel::dismiss(&app_for_focus);
                    let _ = win_clone.hide();
                }
            });
        }
        Err(e) => {
            error!(error = %e, "Failed to pre-create quick panel window");
        }
    }
}

/// Show the quick panel centered on screen (like Raycast).
///
/// Expects the panel to already exist (via `pre_create`). Falls back to
/// creating inline if it doesn't exist yet.
///
/// 在屏幕中央显示快捷面板（类似 Raycast）。
pub fn show(app: &tauri::AppHandle) {
    let (panel_x, panel_y) = screen_center_position();
    info!(panel_x, panel_y, "Showing quick panel centered on screen");

    // If panel doesn't exist yet (pre_create wasn't called), create it now
    if app.get_webview_window(PANEL_LABEL).is_none() {
        warn!("Quick panel not pre-created, creating inline (may activate app)");
        pre_create(app);
    }

    if let Some(window) = app.get_webview_window(PANEL_LABEL) {
        // Reposition to screen center
        if let Err(e) = window.set_position(tauri::Position::Logical(tauri::LogicalPosition::new(
            panel_x, panel_y,
        ))) {
            warn!(error = %e, "Failed to set quick panel position");
        }

        // Show panel without activating the app (macOS uses orderFrontRegardless)
        #[cfg(target_os = "macos")]
        macos::show_panel(&window);
        #[cfg(not(target_os = "macos"))]
        {
            let _ = window.show();
            let _ = window.set_focus();
        }

        // Notify the frontend to refresh data
        if let Err(e) = app.emit_to(PANEL_LABEL, "quick-panel://refresh", ()) {
            warn!(error = %e, "Failed to emit refresh event to quick panel");
        }
    }
}

/// Dismiss the quick panel and restore focus to the previous app.
///
/// On macOS (NSPanel): focus returns to the previous app automatically
/// because our app was never activated. On other platforms: TODO — manual
/// focus restoration.
///
/// 关闭快捷面板并恢复焦点到之前的应用。
pub fn dismiss(app: &tauri::AppHandle) {
    // Dismiss preview panel first
    crate::preview_panel::dismiss(app);

    if let Some(window) = app.get_webview_window(PANEL_LABEL) {
        let _ = window.hide();
    }
}

/// Dismiss the quick panel, then paste clipboard content to the previous app.
///
/// 关闭快捷面板，然后将剪贴板内容粘贴到之前的应用。
pub fn paste(app: &tauri::AppHandle) {
    dismiss(app);

    #[cfg(target_os = "macos")]
    {
        // Small delay for the panel to fully hide before simulating keystrokes
        std::thread::sleep(std::time::Duration::from_millis(50));
        macos::simulate_paste();
    }
}

// ── Tauri Commands ─────────────────────────────────────────────────────

/// Dismiss the quick panel and return focus to the previous app (no paste).
///
/// 关闭快捷面板并将焦点返回到之前的应用（不粘贴）。
#[tauri::command]
pub async fn dismiss_quick_panel(app: tauri::AppHandle) -> Result<(), String> {
    let handle = app.clone();
    app.run_on_main_thread(move || {
        dismiss(&handle);
    })
    .map_err(|e| format!("Failed to dispatch to main thread: {e}"))?;
    Ok(())
}

/// Hide the quick panel, re-activate the previous app, and paste.
///
/// 隐藏快捷面板，重新激活之前的应用，并粘贴。
#[tauri::command]
pub async fn paste_to_previous_app(app: tauri::AppHandle) -> Result<(), String> {
    let handle = app.clone();
    app.run_on_main_thread(move || {
        paste(&handle);
    })
    .map_err(|e| format!("Failed to dispatch to main thread: {e}"))?;
    Ok(())
}
