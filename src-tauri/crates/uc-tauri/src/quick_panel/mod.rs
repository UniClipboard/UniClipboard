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
mod shortcut_registry;
#[cfg(target_os = "windows")]
mod windows;

pub use shortcut_registry::TauriGlobalShortcutRegistry;

use std::sync::Mutex;
use std::time::Instant;
use tauri::{Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use tracing::{debug, error, info, warn};
use uc_core::settings::model::QuickPanelPosition;

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

/// The user's preferred placement for the quick panel.
///
/// Mirrors the persisted `quick_panel.position` setting so `show()` —— which
/// runs synchronously on the main thread off a global-shortcut callback —— can
/// pick a placement without an async settings read. Seeded at startup from the
/// loaded settings and updated whenever the user changes the preference.
static PANEL_POSITION: Mutex<QuickPanelPosition> = Mutex::new(QuickPanelPosition::Center);

/// Gap (logical pixels) between the cursor and the nearest panel edge when the
/// panel is anchored to the cursor. Keeps the pointer from overlapping the
/// panel's border on open.
const CURSOR_ANCHOR_GAP: f64 = 6.0;

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

/// Resolve the monitor that should host the panel: the one under `cursor`,
/// falling back to the primary monitor.
///
/// `cursor` is the physical cursor position, or `None` when it couldn't be
/// read. A `None` result means no monitor could be resolved at all (no cursor
/// monitor *and* no primary) —— callers fall back to a fixed rectangle.
///
/// 解析承载面板的屏幕：优先鼠标所在屏，回退到主屏。
fn resolve_panel_monitor(
    app: &tauri::AppHandle,
    cursor: Option<tauri::PhysicalPosition<f64>>,
) -> Option<tauri::Monitor> {
    cursor
        .and_then(|cursor| match app.monitor_from_point(cursor.x, cursor.y) {
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
        })
        .or_else(|| match app.primary_monitor() {
            Ok(monitor) => monitor,
            Err(error) => {
                warn!(
                    error = %error,
                    "Failed to resolve primary monitor for quick panel positioning"
                );
                None
            }
        })
}

/// Read the physical cursor position, logging on failure.
fn cursor_position(app: &tauri::AppHandle) -> Option<tauri::PhysicalPosition<f64>> {
    match app.cursor_position() {
        Ok(cursor) => Some(cursor),
        Err(error) => {
            warn!(error = %error, "Failed to read cursor position for quick panel positioning");
            None
        }
    }
}

/// Get the quick panel position centered on the monitor that currently
/// contains the mouse cursor.
///
/// 获取鼠标所在屏幕上的面板居中位置。
fn panel_position_for_cursor_screen(app: &tauri::AppHandle, width: f64, height: f64) -> (f64, f64) {
    resolve_panel_monitor(app, cursor_position(app))
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

/// Get the quick panel top-left so the panel opens from the cursor.
///
/// The panel prefers to open down-right with its top-left near the cursor; if
/// it would overflow the monitor's right/bottom edge it flips to open up/left
/// (far edge near the cursor); if it fits on neither side it is clamped fully
/// onto the monitor. Falls back to centering when the cursor or its monitor
/// can't be resolved.
///
/// 让面板从光标处展开：默认向右下、必要时翻转向左上，始终夹取在屏幕内。
fn cursor_anchored_position(app: &tauri::AppHandle, width: f64, height: f64) -> (f64, f64) {
    let Some(cursor) = cursor_position(app) else {
        return panel_position_for_cursor_screen(app, width, height);
    };
    let Some(monitor) = resolve_panel_monitor(app, Some(cursor)) else {
        warn!("No monitor detected for cursor-anchored quick panel; centering instead");
        return panel_position_for_cursor_screen(app, width, height);
    };

    let scale = monitor.scale_factor();
    let origin = monitor.position();
    let size = monitor.size();
    cursor_anchored_position_from_monitor(
        cursor.x / scale,
        cursor.y / scale,
        origin.x as f64 / scale,
        origin.y as f64 / scale,
        size.width as f64 / scale,
        size.height as f64 / scale,
        width,
        height,
    )
}

/// Pure placement math (all logical pixels) for cursor-anchored positioning.
/// Each axis is resolved independently by [`axis_anchored_position`].
fn cursor_anchored_position_from_monitor(
    cursor_x: f64,
    cursor_y: f64,
    monitor_x: f64,
    monitor_y: f64,
    monitor_width: f64,
    monitor_height: f64,
    panel_width: f64,
    panel_height: f64,
) -> (f64, f64) {
    (
        axis_anchored_position(cursor_x, monitor_x, monitor_width, panel_width),
        axis_anchored_position(cursor_y, monitor_y, monitor_height, panel_height),
    )
}

/// One-axis cursor anchoring:
/// 1. Prefer opening forward: panel near edge at `cursor + gap`.
/// 2. Else flip backward: panel far edge at `cursor - gap`.
/// 3. Else clamp the panel fully within the monitor span.
fn axis_anchored_position(
    cursor: f64,
    monitor_origin: f64,
    monitor_extent: f64,
    panel_extent: f64,
) -> f64 {
    let monitor_end = monitor_origin + monitor_extent;

    let forward = cursor + CURSOR_ANCHOR_GAP;
    if forward + panel_extent <= monitor_end {
        return forward;
    }

    let backward = cursor - CURSOR_ANCHOR_GAP - panel_extent;
    if backward >= monitor_origin {
        return backward;
    }

    // Panel is wider/taller than either side of the cursor leaves; keep it on
    // the monitor. `max(monitor_origin)` guards the case where the panel is
    // larger than the monitor itself (prefer showing the top-left).
    (monitor_end - panel_extent).max(monitor_origin)
}

/// Which side the inline preview pane opens toward, relative to the history pane.
///
/// `Right` is the default (preview grows rightward, history pinned at its
/// anchor). `Left` is the flipped layout used when the right edge can't fit the
/// expanded panel: the preview grows leftward instead, the history pane staying
/// put. The frontend mirrors this by reversing the flex order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpandSide {
    Right,
    Left,
}

/// Logical-pixel bounds of the monitor that hosts the panel (cursor's monitor,
/// primary fallback). `(x, y, width, height)`. `None` when none resolves.
fn panel_monitor_logical_rect(app: &tauri::AppHandle) -> Option<(f64, f64, f64, f64)> {
    let monitor = resolve_panel_monitor(app, cursor_position(app))?;
    let scale = monitor.scale_factor();
    let origin = monitor.position();
    let size = monitor.size();
    Some((
        origin.x as f64 / scale,
        origin.y as f64 / scale,
        size.width as f64 / scale,
        size.height as f64 / scale,
    ))
}

/// Decide the horizontal window position and preview side for a layout change.
///
/// `anchor_x` is the history-pane window-left (the narrow-panel origin recorded
/// at show time). The history pane must stay at `anchor_x` regardless of side:
/// - `Right`: window-left = `anchor_x`, preview grows rightward.
/// - `Left`:  window-left = `anchor_x - (wide - narrow)`, so the history pane
///   (now the right child) still lands on `anchor_x` and the preview occupies
///   the freed space on the left.
///
/// Returns `(window_x, side)`. When collapsed, always `(anchor_x, Right)`.
/// Falls back to a clamped right layout when neither side fits.
fn resolve_horizontal_layout(
    anchor_x: f64,
    monitor_x: f64,
    monitor_width: f64,
    narrow_width: f64,
    wide_width: f64,
    preview_expanded: bool,
) -> (f64, ExpandSide) {
    if !preview_expanded {
        return (anchor_x, ExpandSide::Right);
    }

    let monitor_right = monitor_x + monitor_width;
    let delta = wide_width - narrow_width;

    if anchor_x + wide_width <= monitor_right {
        // Room on the right: history pinned, preview grows rightward.
        (anchor_x, ExpandSide::Right)
    } else if anchor_x - delta >= monitor_x {
        // No room right but room left: history pinned, preview grows leftward.
        (anchor_x - delta, ExpandSide::Left)
    } else {
        // Fits on neither side (panel wider than the monitor allows around the
        // anchor): keep the window on-screen, accepting that the history pane
        // shifts. Prefer the top-left when even that overflows.
        (
            (monitor_right - wide_width).max(monitor_x),
            ExpandSide::Right,
        )
    }
}

/// Resolve the preview side for the current remembered anchor and UI scale,
/// without moving the window. Lets the frontend reverse its layout *before* the
/// window is repositioned, so the history pane never visibly jumps.
pub fn resolve_expand_side(app: &tauri::AppHandle, scale: f64) -> ExpandSide {
    let (narrow_width, height) = panel_dimensions(scale, false);
    let (wide_width, _) = panel_dimensions(scale, true);
    let (anchor_x, _) = panel_origin_or_default(app, narrow_width, height);

    match panel_monitor_logical_rect(app) {
        Some((monitor_x, _, monitor_width, _)) => {
            resolve_horizontal_layout(
                anchor_x,
                monitor_x,
                monitor_width,
                narrow_width,
                wide_width,
                true,
            )
            .1
        }
        None => ExpandSide::Right,
    }
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

/// Update the cached placement preference. Cheap (a mutex write); safe to call
/// off any thread, including the main thread.
///
/// 更新缓存的面板出现位置偏好。
pub fn set_position(position: QuickPanelPosition) {
    if let Ok(mut guard) = PANEL_POSITION.lock() {
        *guard = position;
    }
}

fn current_position() -> QuickPanelPosition {
    PANEL_POSITION
        .lock()
        .map(|guard| *guard)
        .unwrap_or(QuickPanelPosition::Center)
}

/// Compute the panel top-left for a fresh show, honoring the user's placement
/// preference.
fn default_panel_position(app: &tauri::AppHandle, width: f64, height: f64) -> (f64, f64) {
    match current_position() {
        QuickPanelPosition::Center => panel_position_for_cursor_screen(app, width, height),
        QuickPanelPosition::FollowCursor => cursor_anchored_position(app, width, height),
    }
}

/// Reuse the remembered origin (so inline preview expand/collapse doesn't jump)
/// or fall back to the preference-appropriate default position.
fn panel_origin_or_default(app: &tauri::AppHandle, width: f64, height: f64) -> (f64, f64) {
    let remembered_origin = PANEL_ORIGIN.lock().ok().and_then(|guard| *guard);
    match remembered_origin {
        Some(origin) => origin,
        None => default_panel_position(app, width, height),
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
    let position = current_position();
    let (panel_x, panel_y) = default_panel_position(app, width, height);

    info!(
        panel_x,
        panel_y,
        ?position,
        "Showing quick panel at the resolved position on the monitor containing the cursor"
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

/// Update quick panel size and position from the current UI scale and whether
/// the inline preview is expanded.
///
/// The history pane is pinned to its anchor (the narrow-panel origin recorded
/// at show time). When the preview expands and the right edge can't fit it, the
/// window is shifted left by the preview's extra width so the preview opens to
/// the *left* while the history pane stays put — the frontend reverses its flex
/// order to match. See [`resolve_horizontal_layout`].
pub fn set_layout(app: &tauri::AppHandle, scale: f64, preview_expanded: bool) {
    let Some(window) = app.get_webview_window(PANEL_LABEL) else {
        return;
    };

    let (narrow_width, height) = panel_dimensions(scale, false);
    let (wide_width, _) = panel_dimensions(scale, true);
    let display_width = if preview_expanded {
        wide_width
    } else {
        narrow_width
    };

    // History-pane anchor (narrow-panel window-left). Remembered from show();
    // never overwritten with the shifted left-open position, so collapsing or
    // re-expanding keeps the history pane stable.
    let (anchor_x, anchor_y) = panel_origin_or_default(app, narrow_width, height);

    let (window_x, side) = match panel_monitor_logical_rect(app) {
        Some((monitor_x, _, monitor_width, _)) => resolve_horizontal_layout(
            anchor_x,
            monitor_x,
            monitor_width,
            narrow_width,
            wide_width,
            preview_expanded,
        ),
        None => (anchor_x, ExpandSide::Right),
    };

    if let Err(e) = window.set_size(tauri::LogicalSize::new(display_width, height)) {
        warn!(
            error = %e,
            preview_expanded,
            scale,
            display_width,
            height,
            "Failed to update quick panel size"
        );
    }

    if let Err(e) = window.set_position(tauri::Position::Logical(tauri::LogicalPosition::new(
        window_x, anchor_y,
    ))) {
        warn!(
            error = %e,
            preview_expanded,
            scale,
            window_x,
            ?side,
            "Failed to update quick panel position"
        );
    } else {
        remember_panel_origin(anchor_x, anchor_y);
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

#[cfg(test)]
mod tests {
    use super::*;

    // Monitor spanning logical [0, 1000) on each axis for readability.
    const MON_ORIGIN: f64 = 0.0;
    const MON_EXTENT: f64 = 1000.0;
    const PANEL: f64 = 360.0;

    #[test]
    fn axis_opens_forward_when_room() {
        // Cursor with plenty of room ahead: panel opens at cursor + gap.
        let pos = axis_anchored_position(200.0, MON_ORIGIN, MON_EXTENT, PANEL);
        assert_eq!(pos, 200.0 + CURSOR_ANCHOR_GAP);
    }

    #[test]
    fn axis_flips_backward_near_far_edge() {
        // Cursor close to the far edge: panel can't open forward (would overflow
        // 1000), so it flips and its far edge sits at cursor - gap.
        let cursor = 900.0;
        let pos = axis_anchored_position(cursor, MON_ORIGIN, MON_EXTENT, PANEL);
        assert_eq!(pos, cursor - CURSOR_ANCHOR_GAP - PANEL);
        // Fully on-screen.
        assert!(pos >= MON_ORIGIN);
        assert!(pos + PANEL <= MON_ORIGIN + MON_EXTENT);
    }

    #[test]
    fn axis_clamps_when_fits_neither_side() {
        // Panel as wide as the monitor: neither side has room → clamp on-screen.
        let pos = axis_anchored_position(500.0, MON_ORIGIN, MON_EXTENT, MON_EXTENT);
        assert_eq!(pos, MON_ORIGIN);
    }

    #[test]
    fn axis_respects_monitor_origin_offset() {
        // Secondary monitor offset to [2000, 3000): forward placement stays in
        // that monitor's coordinate space.
        let origin = 2000.0;
        let pos = axis_anchored_position(2100.0, origin, MON_EXTENT, PANEL);
        assert_eq!(pos, 2100.0 + CURSOR_ANCHOR_GAP);
    }

    #[test]
    fn axis_flips_backward_on_offset_monitor() {
        let origin = 2000.0;
        let cursor = 2950.0; // near far edge (3000)
        let pos = axis_anchored_position(cursor, origin, MON_EXTENT, PANEL);
        assert_eq!(pos, cursor - CURSOR_ANCHOR_GAP - PANEL);
        assert!(pos >= origin);
    }

    #[test]
    fn both_axes_resolved_independently() {
        // Cursor near the right edge but with room below: x flips, y opens forward.
        let (x, y) = cursor_anchored_position_from_monitor(
            950.0, 100.0, // cursor
            0.0, 0.0, 1000.0, 1000.0, // monitor
            PANEL, PANEL, // panel
        );
        assert_eq!(x, 950.0 - CURSOR_ANCHOR_GAP - PANEL);
        assert_eq!(y, 100.0 + CURSOR_ANCHOR_GAP);
    }

    // Realistic panel widths (logical px at scale 1): narrow = history only,
    // wide = history + gap + preview. delta = wide - narrow = 368.
    const NARROW: f64 = 392.0;
    const WIDE: f64 = 760.0;

    #[test]
    fn layout_collapsed_keeps_anchor_and_right() {
        let (x, side) = resolve_horizontal_layout(300.0, 0.0, 1000.0, NARROW, WIDE, false);
        assert_eq!(x, 300.0);
        assert_eq!(side, ExpandSide::Right);
    }

    #[test]
    fn layout_opens_right_when_room() {
        // Anchor with room on the right: window stays at anchor, opens right.
        let (x, side) = resolve_horizontal_layout(100.0, 0.0, 1000.0, NARROW, WIDE, true);
        assert_eq!(x, 100.0);
        assert_eq!(side, ExpandSide::Right);
        assert!(x + WIDE <= 1000.0); // preview fully on-screen
    }

    #[test]
    fn layout_flips_left_when_right_edge_blocks() {
        // Narrow panel anchored near the right edge: wide panel can't fit on the
        // right, so flip left. Window shifts left by delta; the history pane
        // (right child) lands back on the anchor, preview fully on-screen.
        let anchor = axis_anchored_position(998.0, 0.0, 1000.0, NARROW);
        assert!(anchor + WIDE > 1000.0); // would overflow opening right
        let (x, side) = resolve_horizontal_layout(anchor, 0.0, 1000.0, NARROW, WIDE, true);
        assert_eq!(side, ExpandSide::Left);
        assert_eq!(x, anchor - (WIDE - NARROW));
        assert!(x >= 0.0); // window stays on-screen
                           // History pane (window-right minus narrow card) is unchanged: the wide
                           // window's right edge equals the narrow panel's right edge at the anchor.
        assert_eq!(x + WIDE, anchor + NARROW);
    }

    #[test]
    fn layout_clamps_when_neither_side_fits() {
        // Tiny monitor (800) where the wide panel (760) fits neither opening
        // right (anchor 100 + 760 = 860 > 800) nor flipping left (100 - 368 < 0).
        // Fall back to a clamped right layout flush against the right edge.
        let (x, side) = resolve_horizontal_layout(100.0, 0.0, 800.0, NARROW, WIDE, true);
        assert_eq!(side, ExpandSide::Right);
        assert_eq!(x, 800.0 - WIDE); // flush against the right edge
        assert!(x >= 0.0);
    }

    #[test]
    fn layout_respects_offset_monitor_when_flipping() {
        // Secondary monitor at [2000, 3000). Anchor near its right edge flips
        // left within that monitor's coordinate space.
        let anchor = axis_anchored_position(2998.0, 2000.0, 1000.0, NARROW);
        let (x, side) = resolve_horizontal_layout(anchor, 2000.0, 1000.0, NARROW, WIDE, true);
        assert_eq!(side, ExpandSide::Left);
        assert_eq!(x, anchor - (WIDE - NARROW));
        assert!(x >= 2000.0);
    }
}
