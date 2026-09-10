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

use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use tauri::webview::PageLoadEvent;
use tauri::Manager;
use tracing::{error, info, warn, Instrument};

/// Label of the main window as declared in `tauri.conf.json`.
pub const MAIN_WINDOW_LABEL: &str = "main";
// A broken frontend must not leave an explicitly opened window hidden forever.
const MAIN_WINDOW_REVEAL_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Default)]
struct MainWindowLoadState {
    generation: u64,
    page_loaded: bool,
    frontend_ready: bool,
    reveal_requested: bool,
    reveal_timeout_elapsed: bool,
}

impl MainWindowLoadState {
    fn mark_created(&mut self) -> u64 {
        self.generation += 1;
        self.page_loaded = false;
        self.frontend_ready = false;
        self.reveal_requested = false;
        self.reveal_timeout_elapsed = false;
        self.generation
    }

    fn request_reveal(&mut self) -> bool {
        self.reveal_requested = true;
        self.consume_reveal_request()
    }

    fn mark_loaded(&mut self, generation: u64) -> bool {
        if self.generation != generation {
            return false;
        }
        self.page_loaded = true;
        self.consume_reveal_request()
    }

    fn mark_frontend_ready(&mut self, generation: u64) -> bool {
        if self.generation != generation {
            return false;
        }
        self.frontend_ready = true;
        self.consume_reveal_request()
    }

    fn consume_reveal_request(&mut self) -> bool {
        if self.generation == 0
            || (!(self.page_loaded && self.frontend_ready) && !self.reveal_timeout_elapsed)
            || !self.reveal_requested
        {
            return false;
        }
        self.reveal_requested = false;
        true
    }

    fn mark_reveal_timeout(&mut self, generation: u64) -> bool {
        if self.generation != generation || self.reveal_timeout_elapsed {
            return false;
        }
        self.reveal_timeout_elapsed = true;
        self.consume_reveal_request()
    }
}

static MAIN_WINDOW_LOAD_STATE: Mutex<MainWindowLoadState> = Mutex::new(MainWindowLoadState {
    generation: 0,
    page_loaded: false,
    frontend_ready: false,
    reveal_requested: false,
    reveal_timeout_elapsed: false,
});
static MAIN_WINDOW_CREATION_LOCK: Mutex<()> = Mutex::new(());

fn load_state() -> MutexGuard<'static, MainWindowLoadState> {
    MAIN_WINDOW_LOAD_STATE.lock().unwrap_or_else(|poisoned| {
        warn!("Main window load state mutex poisoned; recovering ownership");
        poisoned.into_inner()
    })
}

/// Request the main window: recreate it if needed, then reveal it once its
/// page and frontend are ready, or the generation's readiness deadline expires.
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
            let generation = load_state().mark_created();
            match create_main_window(app, generation) {
                Ok(window) => window,
                Err(error) => {
                    error!(error = %error, "Failed to recreate main window from config");
                    return;
                }
            }
        }
    };

    if !load_state().request_reveal() {
        info!("Main window reveal deferred until page and frontend are ready");
        return;
    }

    reveal_main_window(&window);
}

fn handle_page_load_finished(window: &tauri::WebviewWindow, generation: u64) {
    if !load_state().mark_loaded(generation) {
        return;
    }

    reveal_main_window(window);
    info!(
        generation,
        "Main window revealed after page and frontend became ready"
    );
}

pub(crate) fn handle_frontend_ready(window: &tauri::WebviewWindow, generation: u64) {
    if load_state().mark_frontend_ready(generation) {
        reveal_main_window(window);
        info!(
            generation,
            "Main window revealed after page and frontend became ready"
        );
    }
}

fn reveal_main_window(window: &tauri::WebviewWindow) {
    let _ = window.unminimize();
    let _ = window.show();
    let _ = window.set_focus();
}

fn schedule_reveal_fallback(window: &tauri::WebviewWindow, generation: u64) {
    let window = window.clone();
    let app = window.app_handle().clone();
    tauri::async_runtime::spawn(
        async move {
            tokio::time::sleep(MAIN_WINDOW_REVEAL_TIMEOUT).await;
            if let Err(error) = app.run_on_main_thread(move || {
                // Never recreate a window from a timer. Keep the original handle
                // so a concurrent recreation cannot redirect this reveal.
                if window
                    .app_handle()
                    .get_webview_window(MAIN_WINDOW_LABEL)
                    .is_none()
                    || !load_state().mark_reveal_timeout(generation)
                {
                    return;
                }
                warn!(
                    generation,
                    error_kind = "main_window_readiness_timeout",
                    retryable = false,
                    "Main window readiness timed out; revealing the existing window"
                );
                reveal_main_window(&window);
            }) {
                warn!(
                    generation,
                    error_kind = "main_window_fallback_dispatch_failed",
                    retryable = false,
                    error = %error,
                    "Failed to dispatch main window fallback reveal"
                );
            }
        }
        .in_current_span(),
    );
}

/// Create the main window from its `tauri.conf.json` entry (`create: false`
/// keeps Tauri from doing this automatically at startup).
///
/// The config declares `visible: false`; [`show_main_window`] keeps a newly
/// created window hidden until its page and rendered frontend are ready. A bounded
/// fallback exposes failed frontends instead of leaving the window inaccessible.
fn create_main_window(
    app: &tauri::AppHandle,
    generation: u64,
) -> tauri::Result<tauri::WebviewWindow> {
    let mut config = app
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

    configure_main_window_config_for_platform(&mut config);

    let window = tauri::WebviewWindowBuilder::from_config(app, &config)?
        .initialization_script(crate::window_frame_environment::initialization_script())
        .initialization_script(format!(
            "window.__UC_MAIN_WINDOW_GENERATION__ = '{generation}';"
        ))
        .on_page_load(move |window, payload| {
            if matches!(payload.event(), PageLoadEvent::Finished) {
                handle_page_load_finished(&window, generation);
            }
        })
        .build()?;
    schedule_reveal_fallback(&window, generation);
    info!("Main window created from config");
    Ok(window)
}

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
    fn timeout_reveals_a_loaded_window_without_frontend_readiness() {
        let mut state = MainWindowLoadState::default();
        let generation = state.mark_created();
        assert!(!state.request_reveal());
        assert!(!state.mark_loaded(generation));
        assert!(state.mark_reveal_timeout(generation));
        assert!(!state.mark_reveal_timeout(generation));
        assert!(!state.mark_frontend_ready(generation));
    }

    #[test]
    fn timeout_bounds_wait_even_when_page_load_never_finishes() {
        let mut state = MainWindowLoadState::default();
        let generation = state.mark_created();
        assert!(!state.request_reveal());
        assert!(state.mark_reveal_timeout(generation));
        assert!(!state.mark_loaded(generation));
        assert!(!state.mark_frontend_ready(generation));
    }

    #[test]
    fn timeout_does_not_refocus_a_normally_revealed_window() {
        let mut state = MainWindowLoadState::default();
        let generation = state.mark_created();
        assert!(!state.request_reveal());
        assert!(!state.mark_loaded(generation));
        assert!(state.mark_frontend_ready(generation));
        assert!(!state.mark_reveal_timeout(generation));
    }

    #[test]
    fn recreated_window_gets_its_own_timeout_and_readiness() {
        let mut state = MainWindowLoadState::default();
        let old_generation = state.mark_created();
        assert!(!state.request_reveal());
        assert!(state.mark_reveal_timeout(old_generation));
        assert!(state.request_reveal());

        let generation = state.mark_created();
        assert!(!state.request_reveal());
        assert!(!state.mark_reveal_timeout(old_generation));
        assert!(!state.mark_frontend_ready(old_generation));
        assert!(!state.mark_loaded(generation));
        assert!(state.mark_reveal_timeout(generation));
    }

    #[test]
    fn timeout_without_open_request_does_not_show_a_window() {
        let mut state = MainWindowLoadState::default();
        let generation = state.mark_created();
        assert!(!state.mark_reveal_timeout(generation));
        assert!(state.request_reveal());
        assert!(!state.mark_reveal_timeout(generation));
    }

    #[test]
    fn page_load_does_not_reveal_before_frontend_commit() {
        let mut state = MainWindowLoadState::default();
        let generation = state.mark_created();

        assert!(!state.request_reveal());
        assert!(!state.mark_loaded(generation));
        assert!(state.mark_frontend_ready(generation));
    }

    #[test]
    fn frontend_commit_does_not_reveal_before_page_load() {
        let mut state = MainWindowLoadState::default();
        let generation = state.mark_created();
        assert!(!state.request_reveal());
        assert!(!state.mark_frontend_ready(generation));
        assert!(state.mark_loaded(generation));
    }

    #[test]
    fn readiness_before_open_request_does_not_show_window() {
        let mut state = MainWindowLoadState::default();
        let generation = state.mark_created();
        assert!(!state.mark_loaded(generation));
        assert!(!state.mark_frontend_ready(generation));
        assert!(state.request_reveal());
    }

    #[test]
    fn stale_page_load_does_not_reveal_recreated_window() {
        let mut state = MainWindowLoadState::default();
        let old_generation = state.mark_created();
        assert!(!state.request_reveal());

        let current_generation = state.mark_created();
        assert!(!state.request_reveal());

        assert!(!state.mark_loaded(old_generation));
        assert!(!state.mark_frontend_ready(old_generation));
        assert!(!state.mark_loaded(current_generation));
        assert!(!state.mark_frontend_ready(old_generation));
        assert!(state.mark_frontend_ready(current_generation));
    }

    #[test]
    fn page_load_consumes_reveal_request() {
        let mut state = MainWindowLoadState::default();
        let generation = state.mark_created();
        assert!(!state.request_reveal());

        assert!(!state.mark_loaded(generation));
        assert!(state.mark_frontend_ready(generation));
        assert!(!state.mark_loaded(generation));
        assert!(!state.mark_frontend_ready(generation));
    }

    #[test]
    fn loaded_window_can_be_explicitly_revealed_again() {
        let mut state = MainWindowLoadState::default();
        let generation = state.mark_created();
        assert!(!state.request_reveal());
        assert!(!state.mark_loaded(generation));
        assert!(state.mark_frontend_ready(generation));

        assert!(state.request_reveal());
        assert!(!state.mark_loaded(generation));
        assert!(!state.mark_frontend_ready(generation));
    }
}

/// Only macOS uses transparency and the shared native window effects.
fn configure_main_window_config_for_platform(config: &mut tauri::utils::config::WindowConfig) {
    // Start without native chrome; the webview applies the saved preference
    // before rendering, including on startup failure and window recreation.
    if cfg!(any(target_os = "linux", target_os = "windows")) {
        config.decorations = false;
    }
    if !cfg!(target_os = "macos") {
        config.transparent = false;
        config.window_effects = None;
    }
}

#[cfg(test)]
mod surface_tests {
    #[test]
    fn main_window_surface_matches_platform_support() {
        let mut config = tauri::utils::config::WindowConfig {
            transparent: true,
            window_effects: Some(Default::default()),
            ..Default::default()
        };
        super::configure_main_window_config_for_platform(&mut config);
        if cfg!(any(target_os = "linux", target_os = "windows")) {
            assert!(!config.decorations);
        }
        assert_eq!(config.transparent, cfg!(target_os = "macos"));
        assert_eq!(config.window_effects.is_some(), cfg!(target_os = "macos"));
    }
}
