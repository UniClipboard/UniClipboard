//! Main-window lifecycle: destroy-on-close, recreate-on-open.
//!
//! The `main` window is declared in `tauri.conf.json` with `"create": false`,
//! so Tauri never auto-creates it at startup. Every "open" entry point (tray
//! click / tray menu, macOS Dock `Reopen`, startup barrier, single-instance
//! second launch) funnels through [`show_main_window`], which recreates the
//! window from that same config entry when it is gone — the config stays the
//! single source of truth for the window's appearance.
//!
//! Closing the window is NOT intercepted anymore: the window (and its webview
//! process — JS heap, DOM, image caches, WS connections) is destroyed,
//! releasing the renderer's memory while the app stays resident in the tray.
//! The resulting `RunEvent::ExitRequested { code: None }` is intercepted in
//! `run.rs` (see [`crate::lightweight::should_stay_resident`]). Reopening is a
//! fresh frontend boot that lands on the home route, reusing the exact same
//! startup path as a cold start (daemon connection poll → session exchange →
//! WS connect).
//!
//! `silent_start` benefits for free: the window is simply never created until
//! the first explicit open, so a login autostart no longer pays the webview
//! cost up front.

use tauri::Manager;
use tracing::{error, info};
// Every `warn!` call in this file lives inside a macOS or Windows `cfg`
// block; on other platforms (e.g. the Linux coverage runner) the import is
// genuinely unused.
#[cfg_attr(
    not(any(target_os = "macos", target_os = "windows")),
    allow(unused_imports)
)]
use tracing::warn;

/// Label of the main window as declared in `tauri.conf.json`.
pub const MAIN_WINDOW_LABEL: &str = "main";

/// Show the main window: make the Dock icon visible on macOS, recreate the
/// window from config if it was destroyed, then unminimize, show, and focus.
pub fn show_main_window(app: &tauri::AppHandle) {
    #[cfg(target_os = "macos")]
    if let Err(error) = app.set_dock_visibility(true) {
        warn!(error = %error, "Failed to show Dock icon before showing main window");
    }

    // macOS:`set_dock_visibility(true)` 把 activation policy 从 `Accessory`
    // 翻回 `Regular`(典型路径:关闭主窗口 → Accessory → 从托盘重新打开)。
    // 但 macOS 把 app 重新塞回 Dock 时不会重读 bundle 图标,会留下空白图标 +
    // 运行小圆点。这里强制重绘 Dock 图标兜底。
    #[cfg(target_os = "macos")]
    refresh_dock_icon(app);

    let window = match app.get_webview_window(MAIN_WINDOW_LABEL) {
        Some(window) => window,
        None => match create_main_window(app) {
            Ok(window) => window,
            Err(error) => {
                error!(error = %error, "Failed to recreate main window from config");
                return;
            }
        },
    };

    let _ = window.unminimize();
    let _ = window.show();
    let _ = window.set_focus();
}

/// Create the main window from its `tauri.conf.json` entry (`create: false`
/// keeps Tauri from doing this automatically at startup).
///
/// The config declares `visible: false`; [`show_main_window`] shows the window
/// right after, matching the first-boot behavior (window appears while the
/// frontend is still loading — the transparent + vibrancy background covers
/// the load, no white flash).
fn create_main_window(app: &tauri::AppHandle) -> tauri::Result<tauri::WebviewWindow> {
    let config = app
        .config()
        .app
        .windows
        .iter()
        .find(|window| window.label == MAIN_WINDOW_LABEL)
        .cloned()
        .ok_or_else(|| {
            tauri::Error::Anyhow(anyhow::anyhow!(
                "main window config missing from tauri.conf.json"
            ))
        })?;

    let window = tauri::WebviewWindowBuilder::from_config(app, &config)?.build()?;
    configure_for_platform(&window);
    info!("Main window created from config");
    Ok(window)
}

/// Windows: the config uses `titleBarStyle: Overlay` for macOS; on Windows the
/// native decorations must be turned off after creation instead. This must run
/// on every (re)creation, not just at startup.
#[cfg(target_os = "windows")]
fn configure_for_platform(window: &tauri::WebviewWindow) {
    if let Err(error) = window.set_decorations(false) {
        warn!(error = %error, "Failed to disable Windows main window decorations");
    }
}

#[cfg(not(target_os = "windows"))]
fn configure_for_platform(_window: &tauri::WebviewWindow) {}

/// macOS: force the Dock to repaint this app's icon after flipping back to the
/// `Regular` activation policy.
///
/// `set_dock_visibility(true)` toggles `NSApplicationActivationPolicy` from
/// `Accessory` to `Regular`, but macOS (notably Sequoia/Tahoe) re-adds the app
/// to the Dock without re-reading the bundle icon — leaving the running-indicator
/// dot over a blank tile (and on some versions a template-mangled icon with a
/// white ring around it). Reassigning `applicationIconImage` to the bundle's own
/// `icon.icns` forces the Dock tile to redraw with the correct full-bleed art.
///
/// AppKit calls must run on the main thread. `show_main_window` is invoked from
/// tray events / startup on the main thread, but we still dispatch through
/// `run_on_main_thread` to stay consistent with `update_scheduler::window` and to
/// defend future callers. The dispatch also lets the policy change settle before
/// we re-push the icon.
#[cfg(target_os = "macos")]
fn refresh_dock_icon(app: &tauri::AppHandle) {
    use objc2::{AnyThread, MainThreadMarker};
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::{ns_string, NSBundle};

    if let Err(error) = app.run_on_main_thread(|| {
        let Some(mtm) = MainThreadMarker::new() else {
            warn!("refresh_dock_icon dispatched off the main thread; skipping");
            return;
        };
        // Load the bundle's own `icon.icns` directly. We must NOT use
        // `NSWorkspace::iconForFile`: for this full-bleed icon (no transparent
        // padding) macOS applies its icon-template rules, shrinking it into a
        // white rounded container — which shows up as a white ring around the
        // Dock tile. Loading the raw icns yields the full-bleed artwork as-is.
        let Some(icns_path) = NSBundle::mainBundle()
            .pathForResource_ofType(Some(ns_string!("icon")), Some(ns_string!("icns")))
        else {
            warn!("refresh_dock_icon: icon.icns missing from bundle resources");
            return;
        };
        let Some(icon) = NSImage::initWithContentsOfFile(NSImage::alloc(), &icns_path) else {
            warn!("refresh_dock_icon: failed to decode icon.icns");
            return;
        };
        // SAFETY: runs on the main thread (asserted by `mtm`); `icon` stays alive
        // for the call. Setting the application icon image only repaints the Dock
        // tile — no ownership transfer.
        unsafe {
            NSApplication::sharedApplication(mtm).setApplicationIconImage(Some(&icon));
        }
    }) {
        warn!(error = %error, "Failed to dispatch Dock icon refresh to the main thread");
    }
}
