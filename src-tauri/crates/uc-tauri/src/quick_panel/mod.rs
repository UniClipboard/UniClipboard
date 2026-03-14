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
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
use tracing::{debug, error, info, warn};

/// Default global shortcut for the quick panel (Tauri format).
pub const DEFAULT_SHORTCUT: &str = "super+ctrl+v";

/// Settings key used to store the quick panel shortcut override.
pub const SHORTCUT_SETTINGS_KEY: &str = "global.toggleQuickPanel";

/// Panel dimensions (logical pixels).
const PANEL_WIDTH: f64 = 360.0;
const PANEL_HEIGHT: f64 = 420.0;

/// Tauri window label for the quick panel.
pub(crate) const PANEL_LABEL: &str = "quick-panel";

// ── Cross-platform helpers ─────────────────────────────────────────────

/// Get screen center position for the panel (top-left corner of the panel
/// such that it appears centered on screen, like Raycast/Spotlight).
///
/// 获取面板在屏幕居中时的左上角坐标（类似 Raycast/Spotlight 的位置）。
fn screen_center_position(app: &tauri::AppHandle) -> (f64, f64) {
    #[cfg(target_os = "macos")]
    {
        let _ = app; // used only on non-macOS
        macos::get_screen_center(PANEL_WIDTH, PANEL_HEIGHT)
    }
    #[cfg(not(target_os = "macos"))]
    {
        // Query the primary monitor for its size; fall back to 800x600 if unavailable.
        let (screen_w, screen_h) = app
            .get_webview_window(PANEL_LABEL)
            .and_then(|w| w.primary_monitor().ok().flatten())
            .map(|m| {
                let size = m.size();
                let scale = m.scale_factor();
                (size.width as f64 / scale, size.height as f64 / scale)
            })
            .unwrap_or_else(|| {
                warn!("No primary monitor detected, using 800x600 fallback for panel centering");
                (800.0, 600.0)
            });
        (
            (screen_w - PANEL_WIDTH) / 2.0,
            (screen_h - PANEL_HEIGHT) / 2.0,
        )
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

/// Check whether the quick panel is currently visible.
///
/// 检查快捷面板是否当前可见。
pub fn is_visible(app: &tauri::AppHandle) -> bool {
    app.get_webview_window(PANEL_LABEL)
        .and_then(|w| w.is_visible().ok())
        .unwrap_or(false)
}

/// Toggle the quick panel: show if hidden, dismiss if visible.
///
/// 切换快捷面板：隐藏时显示，显示时关闭。
pub fn toggle(app: &tauri::AppHandle) {
    if is_visible(app) {
        dismiss(app);
    } else {
        show(app);
    }
}

/// Show the quick panel centered on screen (like Raycast).
///
/// Expects the panel to already exist (via `pre_create`). Falls back to
/// creating inline if it doesn't exist yet.
///
/// 在屏幕中央显示快捷面板（类似 Raycast）。
pub fn show(app: &tauri::AppHandle) {
    let (panel_x, panel_y) = screen_center_position(app);
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
///
/// Returns an error on platforms where simulated paste is not yet implemented.
pub fn paste(app: &tauri::AppHandle) -> Result<(), String> {
    dismiss(app);

    #[cfg(target_os = "macos")]
    {
        // Small delay for the panel to fully hide before simulating keystrokes
        std::thread::sleep(std::time::Duration::from_millis(50));
        macos::simulate_paste()?;
        Ok(())
    }

    #[cfg(not(target_os = "macos"))]
    {
        Err("Paste to previous app is not yet supported on this platform".into())
    }
}

// ── Global shortcut management ────────────────────────────────────────

/// Resolve the quick panel shortcut string from settings (in Tauri format).
///
/// Falls back to [`DEFAULT_SHORTCUT`] if not configured.
pub fn resolve_shortcut_from_settings(
    settings: &uc_core::settings::model::Settings,
) -> Vec<String> {
    use uc_core::settings::model::ShortcutKey;

    match settings.keyboard_shortcuts.get(SHORTCUT_SETTINGS_KEY) {
        Some(ShortcutKey::Single(s)) => vec![normalize_shortcut_for_tauri(s)],
        Some(ShortcutKey::Multiple(v)) => {
            let shortcuts: Vec<String> = v
                .iter()
                .map(|s| normalize_shortcut_for_tauri(s))
                .filter(|s| !s.is_empty())
                .collect();
            if shortcuts.is_empty() {
                vec![DEFAULT_SHORTCUT.to_string()]
            } else {
                shortcuts
            }
        }
        _ => vec![DEFAULT_SHORTCUT.to_string()],
    }
}

/// Convert a frontend shortcut string (e.g. `mod+ctrl+v`) to the Tauri
/// global-shortcut format (e.g. `super+ctrl+v` on macOS).
///
/// 将前端快捷键字符串转换为 Tauri 全局快捷键格式。
pub fn normalize_shortcut_for_tauri(key: &str) -> String {
    key.split('+')
        .map(|part| {
            match part.trim().to_lowercase().as_str() {
                // `mod` = platform modifier: Cmd on macOS, Ctrl on others
                "mod" | "cmd" | "command" => {
                    if cfg!(target_os = "macos") {
                        "super"
                    } else {
                        "ctrl"
                    }
                }
                other => return other.to_string(),
            }
            .to_string()
        })
        .collect::<Vec<_>>()
        .join("+")
}

/// Register a global shortcut that toggles the quick panel.
///
/// 注册一个用于切换快捷面板的全局快捷键。
pub fn register_global_shortcut(app: &tauri::AppHandle, shortcut_str: &str) -> Result<(), String> {
    let app_handle = app.clone();
    app.global_shortcut()
        .on_shortcut(shortcut_str, move |_app, _shortcut, event| {
            if event.state == ShortcutState::Pressed {
                info!("Global shortcut triggered for quick panel");
                toggle(&app_handle);
            }
        })
        .map_err(|e| {
            error!(error = %e, shortcut = %shortcut_str, "Failed to register global shortcut for quick panel");
            format!("Failed to register shortcut '{}': {}", shortcut_str, e)
        })?;
    info!(shortcut = %shortcut_str, "Global shortcut registered for quick panel");
    Ok(())
}

/// Unregister old shortcuts and register new ones atomically.
///
/// If registering any new shortcut fails, attempts to re-register all old
/// shortcuts so the system is not left without a working shortcut.
///
/// 原子地注销旧快捷键并注册新快捷键。如果注册新快捷键失败，
/// 尝试重新注册旧快捷键以避免系统处于无快捷键状态。
pub fn update_global_shortcut(
    app: &tauri::AppHandle,
    old: &[String],
    new: &[String],
) -> Result<(), String> {
    // Unregister all old shortcuts
    for shortcut in old {
        if let Err(e) = app.global_shortcut().unregister(shortcut.as_str()) {
            warn!(error = %e, shortcut = %shortcut, "Failed to unregister old global shortcut");
        }
    }

    // Register all new shortcuts; on failure, rollback to old shortcuts
    for shortcut in new {
        if let Err(e) = register_global_shortcut(app, shortcut) {
            warn!(error = %e, shortcut = %shortcut, "New shortcut registration failed, rolling back");
            // Unregister any new shortcuts that were successfully registered
            for already in new {
                if already == shortcut {
                    break;
                }
                let _ = app.global_shortcut().unregister(already.as_str());
            }
            // Re-register old shortcuts
            for old_shortcut in old {
                if let Err(rb_err) = register_global_shortcut(app, old_shortcut) {
                    error!(
                        error = %rb_err,
                        shortcut = %old_shortcut,
                        "Failed to rollback old global shortcut"
                    );
                }
            }
            return Err(e);
        }
    }
    Ok(())
}
