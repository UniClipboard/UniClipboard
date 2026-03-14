//! macOS NSPanel implementation for the preview panel.
//!
//! Unlike the quick panel, the preview panel does NOT need to become key window
//! (no keyboard input needed). It uses a standard NSPanel with NonactivatingPanel
//! so it won't steal focus from the quick panel.

use objc2::ffi::object_setClass;
use objc2::runtime::AnyObject;
use objc2::{ClassType, MainThreadMarker};
use objc2_app_kit::{NSPanel, NSScreen, NSWindowStyleMask};
use tauri::WebviewWindow;
use tracing::{error, info};

/// Convert a Tauri WebviewWindow's underlying NSWindow into an NSPanel
/// with `NonactivatingPanel` behavior. Unlike the quick panel's UCKeyablePanel,
/// this does NOT override `canBecomeKeyWindow` — the preview panel should not
/// become key window to avoid stealing keyboard focus from the quick panel.
///
/// # Safety
/// - NSPanel is a direct subclass of NSWindow with no extra ivars,
///   so `object_setClass` is safe.
/// - Must be called from the main thread.
pub fn convert_to_non_key_panel(window: &WebviewWindow) {
    if MainThreadMarker::new().is_none() {
        error!("convert_to_non_key_panel called from a non-main thread");
        return;
    }

    let ns_window = match window.ns_window() {
        Ok(ptr) => ptr,
        Err(e) => {
            error!(error = %e, "Failed to get ns_window pointer for preview panel");
            return;
        }
    };

    unsafe {
        // Swap ObjC class from NSWindow → NSPanel (no custom subclass needed)
        let panel_class = NSPanel::class();
        object_setClass(ns_window as *mut AnyObject, panel_class as *const _);

        let panel: &NSPanel = &*(ns_window as *const NSPanel);

        // Add NonactivatingPanel to the existing style mask
        let mut style = panel.styleMask();
        style |= NSWindowStyleMask::NonactivatingPanel;
        panel.setStyleMask(style);

        // Configure panel behavior
        panel.setFloatingPanel(true);
        panel.setBecomesKeyOnlyIfNeeded(true); // Don't accept keyboard input
        panel.setHidesOnDeactivate(false);
    }

    info!("Converted NSWindow → NSPanel (non-key) for preview panel");
}

/// Show the preview panel without activating the app or making it key.
pub fn show_panel(window: &WebviewWindow) {
    if MainThreadMarker::new().is_none() {
        error!("show_panel called from a non-main thread");
        return;
    }

    let ns_window = match window.ns_window() {
        Ok(ptr) => ptr,
        Err(e) => {
            error!(error = %e, "Failed to get ns_window for show_panel (preview)");
            return;
        }
    };

    unsafe {
        let panel: &NSPanel = &*(ns_window as *const NSPanel);
        panel.orderFrontRegardless();
        // Do NOT call makeKeyWindow — preview should not steal keyboard focus
    }
}

/// Get the screen dimensions for positioning calculations.
///
/// Returns `Err` if not called from the main thread or if no main screen is available.
pub fn get_screen_size() -> Result<(f64, f64), String> {
    let mtm = MainThreadMarker::new().ok_or_else(|| {
        let msg = "get_screen_size called from a non-main thread";
        error!(msg);
        msg.to_string()
    })?;
    let screen = NSScreen::mainScreen(mtm).ok_or_else(|| {
        let msg = "NSScreen::mainScreen returned nil";
        error!(msg);
        msg.to_string()
    })?;
    let frame = screen.frame();
    Ok((frame.size.width, frame.size.height))
}
