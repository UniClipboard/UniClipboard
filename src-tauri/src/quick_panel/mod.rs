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

/// Get cursor position in screen coordinates (top-left origin).
///
/// 获取光标位置（屏幕坐标，左上角原点）。
fn cursor_position() -> (f64, f64) {
    #[cfg(target_os = "macos")]
    {
        macos::get_cursor_position()
    }
    #[cfg(not(target_os = "macos"))]
    {
        // Fallback: center of screen
        (400.0, 300.0)
    }
}

// ── Public API ─────────────────────────────────────────────────────────

/// Show (or create) the quick panel at the current cursor position.
///
/// 在当前光标位置显示（或创建）快捷面板。
pub fn show(app: &tauri::AppHandle) {
    let (cursor_x, cursor_y) = cursor_position();
    info!(cursor_x, cursor_y, "Showing quick panel at cursor position");

    // Position panel so its top-left corner is at the cursor
    let panel_x = cursor_x;
    let panel_y = cursor_y;

    match app.get_webview_window(PANEL_LABEL) {
        Some(window) => {
            // Panel already exists — reposition and show
            if let Err(e) = window.set_position(tauri::Position::Logical(
                tauri::LogicalPosition::new(panel_x, panel_y),
            )) {
                warn!(error = %e, "Failed to set quick panel position");
            }
            let _ = window.show();

            // Make panel key window for keyboard input (macOS-specific,
            // avoids NSApp.activate that Tauri's set_focus would trigger)
            #[cfg(target_os = "macos")]
            macos::make_panel_key(&window);
            #[cfg(not(target_os = "macos"))]
            let _ = window.set_focus();

            // Notify the frontend to refresh data
            if let Err(e) = app.emit_to(PANEL_LABEL, "quick-panel://refresh", ()) {
                warn!(error = %e, "Failed to emit refresh event to quick panel");
            }
        }
        None => {
            // Create a new panel window (hidden initially for NSPanel conversion)
            let url = WebviewUrl::App("quick-panel.html".into());
            match WebviewWindowBuilder::new(app, PANEL_LABEL, url)
                .title("Quick Panel")
                .inner_size(PANEL_WIDTH, PANEL_HEIGHT)
                .position(panel_x, panel_y)
                .decorations(false)
                .transparent(true)
                .always_on_top(true)
                .visible(false) // Hidden until platform setup is complete
                .resizable(false)
                .skip_taskbar(true)
                .build()
            {
                Ok(window) => {
                    info!("Quick panel window created");

                    // ── Platform-specific panel conversion ──
                    // On macOS: convert NSWindow → NSPanel with NonactivatingPanel.
                    // This makes the panel receive keyboard input without
                    // activating our app, so the previous app stays frontmost.
                    #[cfg(target_os = "macos")]
                    macos::convert_to_panel(&window);

                    // Auto-hide when the panel loses focus (user clicks elsewhere)
                    let win_clone = window.clone();
                    window.on_window_event(move |event| {
                        if let tauri::WindowEvent::Focused(false) = event {
                            debug!("Quick panel lost focus, hiding");
                            let _ = win_clone.hide();
                        }
                    });

                    // Now show the fully-configured panel
                    let _ = window.show();

                    #[cfg(target_os = "macos")]
                    macos::make_panel_key(&window);
                    #[cfg(not(target_os = "macos"))]
                    let _ = window.set_focus();
                }
                Err(e) => {
                    error!(error = %e, "Failed to create quick panel window");
                }
            }
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
