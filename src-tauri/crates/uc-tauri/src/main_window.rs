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

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use tauri::webview::PageLoadEvent;
use tauri::Manager;
use tracing::{error, info, warn};

/// Label of the main window as declared in `tauri.conf.json`.
pub const MAIN_WINDOW_LABEL: &str = "main";

#[derive(Default)]
struct MainWindowLoadState {
    generation: AtomicU64,
    loaded_generation: AtomicU64,
    reveal_requested_generation: AtomicU64,
}

impl MainWindowLoadState {
    fn mark_created(&self) -> u64 {
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        self.loaded_generation.store(0, Ordering::SeqCst);
        self.reveal_requested_generation.store(0, Ordering::SeqCst);
        generation
    }

    fn request_reveal(&self) -> bool {
        let generation = self.generation.load(Ordering::SeqCst);
        self.reveal_requested_generation
            .store(generation, Ordering::SeqCst);

        self.loaded_generation.load(Ordering::SeqCst) == generation
            && self.generation.load(Ordering::SeqCst) == generation
            && self.consume_reveal_request(generation)
    }

    fn mark_loaded(&self, generation: u64) -> bool {
        if self.generation.load(Ordering::SeqCst) != generation {
            return false;
        }

        self.loaded_generation.store(generation, Ordering::SeqCst);

        self.generation.load(Ordering::SeqCst) == generation
            && self.consume_reveal_request(generation)
    }

    fn consume_reveal_request(&self, generation: u64) -> bool {
        self.reveal_requested_generation
            .compare_exchange(generation, 0, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }
}

static MAIN_WINDOW_LOAD_STATE: MainWindowLoadState = MainWindowLoadState {
    generation: AtomicU64::new(0),
    loaded_generation: AtomicU64::new(0),
    reveal_requested_generation: AtomicU64::new(0),
};
static MAIN_WINDOW_CREATION_LOCK: Mutex<()> = Mutex::new(());

/// Request the main window: recreate it if needed, then reveal it once its
/// initial page load has finished.
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

    // Serialize the complete get-or-create transition. Tray, single-instance,
    // and lightweight-mode callbacks can request the window concurrently; two
    // independent creations would detach the load generation from the window
    // instance that actually won the duplicate-label race.
    let _creation_guard = match MAIN_WINDOW_CREATION_LOCK.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            warn!("Main window creation lock was poisoned; recovering ownership");
            poisoned.into_inner()
        }
    };

    let window = match app.get_webview_window(MAIN_WINDOW_LABEL) {
        Some(window) => window,
        None => {
            let generation = MAIN_WINDOW_LOAD_STATE.mark_created();
            match create_main_window(app, generation) {
                Ok(window) => window,
                Err(error) => {
                    error!(error = %error, "Failed to recreate main window from config");
                    return;
                }
            }
        }
    };

    if !MAIN_WINDOW_LOAD_STATE.request_reveal() {
        info!("Main window reveal deferred until initial page load finishes");
        return;
    }

    reveal_main_window(&window);
}

fn handle_page_load_finished(window: &tauri::WebviewWindow, generation: u64) {
    if !MAIN_WINDOW_LOAD_STATE.mark_loaded(generation) {
        return;
    }

    reveal_main_window(window);
    info!(generation, "Main window revealed after initial page load");
}

fn reveal_main_window(window: &tauri::WebviewWindow) {
    let _ = window.unminimize();
    let _ = window.show();
    let _ = window.set_focus();
}

/// Create the main window from its `tauri.conf.json` entry (`create: false`
/// keeps Tauri from doing this automatically at startup).
///
/// The config declares `visible: false`; [`show_main_window`] keeps a newly
/// created window hidden until its first page load finishes. This avoids
/// exposing WebView2's unpainted surface during a Windows cold start.
fn create_main_window(
    app: &tauri::AppHandle,
    generation: u64,
) -> tauri::Result<tauri::WebviewWindow> {
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

    let window = tauri::WebviewWindowBuilder::from_config(app, &config)?
        .on_page_load(move |window, payload| {
            if matches!(payload.event(), PageLoadEvent::Finished) {
                handle_page_load_finished(&window, generation);
            }
        })
        .build()?;
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

#[cfg(test)]
mod tests {
    use super::MainWindowLoadState;

    #[test]
    fn reveal_waits_for_initial_page_load() {
        let state = MainWindowLoadState::default();
        let generation = state.mark_created();

        assert!(!state.request_reveal());
        assert!(state.mark_loaded(generation));
    }

    #[test]
    fn stale_page_load_does_not_reveal_recreated_window() {
        let state = MainWindowLoadState::default();
        let old_generation = state.mark_created();
        assert!(!state.request_reveal());

        let current_generation = state.mark_created();
        assert!(!state.request_reveal());

        assert!(!state.mark_loaded(old_generation));
        assert!(state.mark_loaded(current_generation));
    }

    #[test]
    fn page_load_consumes_reveal_request() {
        let state = MainWindowLoadState::default();
        let generation = state.mark_created();
        assert!(!state.request_reveal());

        assert!(state.mark_loaded(generation));
        assert!(!state.mark_loaded(generation));
    }

    #[test]
    fn loaded_window_can_be_explicitly_revealed_again() {
        let state = MainWindowLoadState::default();
        let generation = state.mark_created();
        assert!(!state.request_reveal());
        assert!(state.mark_loaded(generation));

        assert!(state.request_reveal());
        assert!(!state.mark_loaded(generation));
    }
}
