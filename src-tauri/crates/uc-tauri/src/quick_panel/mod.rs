//! Cross-platform quick clipboard panel.
//!
//! Provides a Spotlight-like floating panel for clipboard history.
//! On macOS, the panel uses NSPanel with `NonactivatingPanel` so the
//! previously focused application stays frontmost — no PID tracking needed.
//!
//! 跨平台快捷剪贴板面板。macOS 上使用 NSPanel，不会抢夺前台应用焦点。

#[cfg(target_os = "macos")]
mod macos;
#[cfg(any(target_os = "windows", test))]
mod paste_sequence;
#[cfg(target_os = "windows")]
mod windows;

use std::sync::Mutex;
use std::time::Instant;
use tauri::{Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
use tracing::{debug, error, info, warn};

/// Timestamp of the last `show()` call. Blur events within
/// [`BLUR_DEBOUNCE_MS`] of this timestamp are ignored to prevent
/// the "show → instant blur → hide" race on Windows/Linux.
static LAST_SHOW_TIME: Mutex<Option<Instant>> = Mutex::new(None);

/// The logical top-left origin used for the currently shown quick panel.
///
/// `show()` centers the history-only panel and records that origin. Later
/// inline preview expand/collapse operations reuse the same origin so the
/// window grows and shrinks relative to the history pane instead of jumping
/// to re-center the full width.
static PANEL_ORIGIN: Mutex<Option<(f64, f64)>> = Mutex::new(None);

/// How long (ms) after `show()` to suppress blur events.
const BLUR_DEBOUNCE_MS: u128 = 300;

/// How long (ms) to wait before verifying focus is actually gone.
///
/// When a `Focused(false)` event arrives, we don't hide immediately.
/// Instead we wait this long and then check `is_focused()`. Spurious
/// blur events (AttachThreadInput detach, IME popups, Windows system
/// notifications) are transient and focus returns within a few ms.
/// A real "user clicked elsewhere" loss persists past this delay.
const BLUR_VERIFY_DELAY_MS: u64 = 100;

/// Default global shortcut for the quick panel (Tauri format).
/// macOS: Cmd+Ctrl+V, Windows/Linux: Ctrl+Alt+V
#[cfg(target_os = "macos")]
pub const DEFAULT_SHORTCUT: &str = "super+ctrl+v";
#[cfg(not(target_os = "macos"))]
pub const DEFAULT_SHORTCUT: &str = "ctrl+alt+v";

/// Settings key used to store the quick panel shortcut override.
pub const SHORTCUT_SETTINGS_KEY: &str = "global.toggleQuickPanel";

/// Panel base dimensions (logical pixels at 100% UI scale).
const BASE_PANEL_WIDTH: f64 = 360.0;
const BASE_PANEL_HEIGHT: f64 = 420.0;
const BASE_PREVIEW_WIDTH: f64 = 360.0;
const PANEL_GAP: f64 = 8.0;
const MIN_UI_SCALE: f64 = 0.8;
const MAX_UI_SCALE: f64 = 1.5;

/// Space (logical pixels) reserved around the cards for shadows and rounded corners.
/// This padding is included in the window size but remains transparent in the UI.
const WINDOW_PADDING: f64 = 16.0;

/// Tauri window label for the quick panel.
pub(crate) const PANEL_LABEL: &str = "quick-panel";

// ── Cross-platform helpers ─────────────────────────────────────────────

fn centered_panel_position_from_monitor(
    monitor_origin_x: i32,
    monitor_origin_y: i32,
    monitor_width_px: u32,
    monitor_height_px: u32,
    scale_factor: f64,
    panel_width: f64,
    panel_height: f64,
) -> (f64, f64) {
    let monitor_x = monitor_origin_x as f64 / scale_factor;
    let monitor_y = monitor_origin_y as f64 / scale_factor;
    let monitor_width = monitor_width_px as f64 / scale_factor;
    let monitor_height = monitor_height_px as f64 / scale_factor;

    (
        monitor_x + (monitor_width - panel_width) / 2.0,
        monitor_y + (monitor_height - panel_height) / 2.0,
    )
}

/// Get the quick panel position centered on the monitor that currently
/// contains the mouse cursor.
///
/// 获取鼠标所在屏幕上的面板居中位置。
fn panel_position_for_cursor_screen(app: &tauri::AppHandle, width: f64, height: f64) -> (f64, f64) {
    let target_monitor = match app.cursor_position() {
        Ok(cursor) => match app.monitor_from_point(cursor.x, cursor.y) {
            Ok(Some(monitor)) => Some(monitor),
            Ok(None) => {
                // Normal fallback path: cursor is between monitors / on a
                // virtual display / a monitor was just hot-unplugged. The
                // primary-monitor fallback below is the intended behavior,
                // so this is debug-level diagnostic, not a warning.
                debug!(
                    cursor_x = cursor.x,
                    cursor_y = cursor.y,
                    "No monitor found for cursor position; falling back to primary monitor"
                );
                None
            }
            Err(error) => {
                warn!(
                    error = %error,
                    cursor_x = cursor.x,
                    cursor_y = cursor.y,
                    "Failed to resolve monitor from cursor position; falling back to primary monitor"
                );
                None
            }
        },
        Err(error) => {
            warn!(
                error = %error,
                "Failed to read cursor position; falling back to primary monitor"
            );
            None
        }
    }
    .or_else(|| match app.primary_monitor() {
        Ok(monitor) => monitor,
        Err(error) => {
            warn!(
                error = %error,
                "Failed to resolve primary monitor for quick panel positioning"
            );
            None
        }
    });

    target_monitor
        .map(|monitor| {
            let size = monitor.size();
            let position = monitor.position();
            centered_panel_position_from_monitor(
                position.x,
                position.y,
                size.width,
                size.height,
                monitor.scale_factor(),
                width,
                height,
            )
        })
        .unwrap_or_else(|| {
            warn!("No monitor detected, using 800x600 fallback for quick panel positioning");
            centered_panel_position_from_monitor(0, 0, 800, 600, 1.0, width, height)
        })
}

fn normalize_ui_scale(scale: f64) -> f64 {
    if !scale.is_finite() {
        return 1.0;
    }

    scale.clamp(MIN_UI_SCALE, MAX_UI_SCALE)
}

fn panel_dimensions(scale: f64, preview_expanded: bool) -> (f64, f64) {
    let normalized_scale = normalize_ui_scale(scale);
    let width = if preview_expanded {
        (BASE_PANEL_WIDTH + PANEL_GAP + BASE_PREVIEW_WIDTH) * normalized_scale
            + (WINDOW_PADDING * 2.0)
    } else {
        (BASE_PANEL_WIDTH * normalized_scale) + (WINDOW_PADDING * 2.0)
    };

    (
        width,
        (BASE_PANEL_HEIGHT * normalized_scale) + (WINDOW_PADDING * 2.0),
    )
}

fn remember_panel_origin(x: f64, y: f64) {
    if let Ok(mut guard) = PANEL_ORIGIN.lock() {
        *guard = Some((x, y));
    }
}

fn resolve_panel_origin(
    remembered_origin: Option<(f64, f64)>,
    centered_origin: (f64, f64),
) -> (f64, f64) {
    remembered_origin.unwrap_or(centered_origin)
}

fn panel_origin_or_center(app: &tauri::AppHandle, width: f64, height: f64) -> (f64, f64) {
    let remembered_origin = PANEL_ORIGIN.lock().ok().and_then(|guard| *guard);
    let centered_origin = panel_position_for_cursor_screen(app, width, height);
    resolve_panel_origin(remembered_origin, centered_origin)
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
    let (initial_width, initial_height) = panel_dimensions(1.0, false);
    match WebviewWindowBuilder::new(app, PANEL_LABEL, url)
        .title("Quick Panel")
        .inner_size(initial_width, initial_height)
        .position(-9999.0, -9999.0)
        .decorations(false)
        .transparent(true)
        .shadow(false)
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
            let win_clone = window.clone();
            window.on_window_event(move |event| {
                if let tauri::WindowEvent::Focused(false) = event {
                    // Debounce: ignore blur events shortly after show() to prevent
                    // the "show → instant blur → hide" race on Windows/Linux.
                    if let Ok(guard) = LAST_SHOW_TIME.lock() {
                        if let Some(t) = *guard {
                            if t.elapsed().as_millis() < BLUR_DEBOUNCE_MS {
                                debug!(
                                    elapsed_ms = t.elapsed().as_millis(),
                                    "Quick panel blur suppressed (within show debounce window)"
                                );
                                return;
                            }
                        }
                    }

                    // Verify the focus loss is real, not a transient glitch.
                    //
                    // Spurious WM_KILLFOCUS messages arrive from many sources on
                    // Windows (AttachThreadInput detach, IME composition windows,
                    // system notifications, WebView2 internal focus shuffles).
                    // All of them are brief — focus returns within a few ms.
                    // A genuine "user clicked elsewhere" loss persists.
                    //
                    // Strategy: spawn a task, wait BLUR_VERIFY_DELAY_MS, then
                    // check is_focused(). If focus is back, discard the event.
                    let win_verify = win_clone.clone();
                    tauri::async_runtime::spawn(async move {
                        tokio::time::sleep(tokio::time::Duration::from_millis(
                            BLUR_VERIFY_DELAY_MS,
                        ))
                        .await;

                        if win_verify.is_focused().unwrap_or(false) {
                            debug!("Quick panel focus returned after blur — spurious event, not hiding");
                            return;
                        }
                        debug!("Quick panel lost focus (verified), hiding");
                        let _ = win_verify.hide();
                    });
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
/// Two-phase show: this function prepares the window (size, position) and
/// emits a `quick-panel://prepare-show` event.  The frontend clears stale
/// state and then calls the `finalize_quick_panel_show` command, which
/// makes the window visible.  This prevents a one-frame flash of old
/// preview content when the panel reopens.
///
/// 在屏幕中央显示快捷面板（类似 Raycast）。
pub fn show(app: &tauri::AppHandle) {
    let (width, height) = panel_dimensions(1.0, false);
    let (panel_x, panel_y) = panel_position_for_cursor_screen(app, width, height);

    info!(
        panel_x,
        panel_y, "Showing quick panel centered on the monitor containing the cursor"
    );

    // If panel doesn't exist yet (pre_create wasn't called), create it now
    if app.get_webview_window(PANEL_LABEL).is_none() {
        warn!("Quick panel not pre-created, creating inline (may activate app)");
        pre_create(app);
    }

    if let Some(window) = app.get_webview_window(PANEL_LABEL) {
        #[cfg(target_os = "windows")]
        windows::remember_previous_foreground(&window);
        // macOS: capture frontmost app *before* the panel becomes key window,
        // so paste() can explicitly send focus back instead of relying on
        // NSPanel's NonactivatingPanel hint (which races on busy systems).
        #[cfg(target_os = "macos")]
        macos::remember_previous_app();

        if let Err(e) = window.set_size(tauri::LogicalSize::new(width, height)) {
            warn!(error = %e, "Failed to reset quick panel size");
        }

        // Reposition to screen center
        if let Err(e) = window.set_position(tauri::Position::Logical(tauri::LogicalPosition::new(
            panel_x, panel_y,
        ))) {
            warn!(error = %e, "Failed to set quick panel position");
        } else {
            remember_panel_origin(panel_x, panel_y);
        }

        // Record show timestamp *before* the frontend finalizes show so the
        // blur handler can debounce spurious Focused(false) events.
        if let Ok(mut guard) = LAST_SHOW_TIME.lock() {
            *guard = Some(Instant::now());
        }

        // Ask frontend to clear stale state; it will call
        // `finalize_quick_panel_show` once it has repainted.
        if let Err(e) = app.emit_to(PANEL_LABEL, "quick-panel://prepare-show", ()) {
            warn!(error = %e, "Failed to emit prepare-show event to quick panel");
        }
    }
}

/// Actually make the quick panel window visible.
///
/// Called by the frontend after it has cleared stale preview state and
/// repainted in response to the `quick-panel://prepare-show` event.
///
/// 实际显示快捷面板窗口（由前端在状态清理后调用）。
pub fn finalize_show(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window(PANEL_LABEL) {
        #[cfg(target_os = "macos")]
        macos::show_panel(&window);
        #[cfg(target_os = "windows")]
        {
            let _ = window.show();
            windows::force_foreground(&window);
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            let _ = window.show();
            let _ = window.set_focus();
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

    #[cfg(target_os = "windows")]
    if let Err(error) = windows::restore_previous_foreground() {
        debug!(%error, "Quick panel dismiss could not restore previous foreground window");
    }
}

/// Update quick panel size and center position from the current UI scale.
pub fn set_layout(app: &tauri::AppHandle, scale: f64, preview_expanded: bool) {
    let Some(window) = app.get_webview_window(PANEL_LABEL) else {
        return;
    };

    let (width, height) = panel_dimensions(scale, preview_expanded);
    let (panel_x, panel_y) = panel_origin_or_center(app, width, height);

    if let Err(e) = window.set_size(tauri::LogicalSize::new(width, height)) {
        warn!(
            error = %e,
            preview_expanded,
            scale,
            width,
            height,
            "Failed to update quick panel size"
        );
    }

    if let Err(e) = window.set_position(tauri::Position::Logical(tauri::LogicalPosition::new(
        panel_x, panel_y,
    ))) {
        warn!(
            error = %e,
            preview_expanded,
            scale,
            panel_x,
            panel_y,
            "Failed to update quick panel position"
        );
    } else {
        remember_panel_origin(panel_x, panel_y);
    }
}
/// Dismiss the quick panel, then paste clipboard content to the previous app.
///
/// 关闭快捷面板，然后将剪贴板内容粘贴到之前的应用。
///
/// Returns an error on platforms where simulated paste is not yet implemented.
pub fn paste(app: &tauri::AppHandle) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        // Hide the panel first so it stops being the key window.
        if let Some(window) = app.get_webview_window(PANEL_LABEL) {
            let _ = window.hide();
        }
        // Explicitly send focus back to the previously frontmost app and wait
        // until it is observed as frontmost. Relying on NonactivatingPanel
        // alone races: when show_panel() called makeKeyWindow(), key status
        // doesn't always return to the target app within the previous fixed
        // 50ms sleep, and Cmd+V is then sent to the wrong responder (or
        // dropped entirely).
        let restored = macos::restore_previous_app();
        if !restored {
            // Either no previous app was recorded, the app is gone, or the
            // 200ms poll timed out. Fall back to a small sleep — the system
            // may still finish the focus handoff a moment later.
            warn!("Quick panel paste: previous app focus not confirmed; posting Cmd+V anyway");
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        macos::simulate_paste()?;
        Ok(())
    }

    #[cfg(target_os = "windows")]
    {
        if let Some(window) = app.get_webview_window(PANEL_LABEL) {
            let _ = window.hide();
        }

        windows::restore_previous_foreground()?;
        windows::simulate_paste()?;
        Ok(())
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        dismiss(app);
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

/// Convert a frontend shortcut string to the Tauri global-shortcut format.
///
/// Mapping rules:
///   - `meta` (the physical Meta/Win/Cmd key) → `super` (Tauri's name for that key)
///   - `mod`/`cmd`/`command` (abstract platform modifier) → `super` on macOS, `ctrl` on others
///   - everything else passes through unchanged
///
/// 将前端快捷键字符串转换为 Tauri 全局快捷键格式。
pub fn normalize_shortcut_for_tauri(key: &str) -> String {
    key.split('+')
        .map(|part| {
            match part.trim().to_lowercase().as_str() {
                // Physical Meta key (Cmd on macOS, Win on Windows) → Tauri `super`
                "meta" | "super" => "super".to_string(),
                // Abstract platform modifier → Cmd on macOS, Ctrl on others
                "mod" | "cmd" | "command" => if cfg!(target_os = "macos") {
                    "super"
                } else {
                    "ctrl"
                }
                .to_string(),
                other => other.to_string(),
            }
        })
        .collect::<Vec<_>>()
        .join("+")
}

/// Register a global shortcut that toggles the quick panel.
///
/// 注册一个用于切换快捷面板的全局快捷键。
pub fn register_global_shortcut(app: &tauri::AppHandle, shortcut_str: &str) -> Result<(), String> {
    // Defensively unregister first — on Windows the OS-level hotkey may survive
    // a crash or force-kill of the previous app instance, causing
    // "HotKey already registered" on the next startup.
    match app.global_shortcut().unregister(shortcut_str) {
        Ok(()) => {}
        Err(e) => {
            warn!(
                error = %e,
                shortcut = %shortcut_str,
                "Defensive unregister before registering global shortcut failed"
            );
        }
    }

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

    // Also unregister new shortcuts defensively, in case they are already
    // registered (e.g. from startup or a previous partial update).
    for shortcut in new {
        if !old.contains(shortcut) {
            let _ = app.global_shortcut().unregister(shortcut.as_str());
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
