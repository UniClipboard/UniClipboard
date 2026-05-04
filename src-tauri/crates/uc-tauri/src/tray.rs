//! System tray icon management.
//!
//! This module provides [`TrayState`] which manages the system tray icon,
//! its context menu, and language-dependent menu item labels.

use std::sync::Mutex;

use tauri::menu::{MenuBuilder, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Emitter, Manager};
use tracing::{debug, info, warn};

/// Managed state that holds the tray icon and its menu item handles.
///
/// Stored via `app.manage(TrayState::default())` and accessed from
/// Tauri commands with `State<'_, TrayState>`.
#[derive(Default)]
pub struct TrayState {
    inner: Mutex<Option<TrayHandles>>,
}

/// Internal handles for the tray icon and its menu items.
struct TrayHandles {
    #[allow(dead_code)]
    tray: tauri::tray::TrayIcon,
    open: MenuItem<tauri::Wry>,
    settings: MenuItem<tauri::Wry>,
    quit: MenuItem<tauri::Wry>,
    language: String,
}

impl TrayState {
    /// Initialize the system tray icon with a context menu.
    ///
    /// This method is idempotent: if the tray is already initialized,
    /// it returns `Ok(())` immediately.
    pub fn init(&self, app: &tauri::AppHandle, initial_language: &str) -> tauri::Result<()> {
        let mut guard = self
            .inner
            .lock()
            .map_err(|e| tauri::Error::Anyhow(anyhow::anyhow!("TrayState lock poisoned: {}", e)))?;

        // Idempotent: already initialized
        if guard.is_some() {
            return Ok(());
        }

        let language = normalize_language(initial_language);
        let (open_label, settings_label, quit_label) = labels_for_language(language);

        // Create menu items with well-known IDs
        let open = MenuItem::with_id(app, "tray.open", open_label, true, None::<&str>)?;
        let settings = MenuItem::with_id(app, "tray.settings", settings_label, true, None::<&str>)?;
        let quit = MenuItem::with_id(app, "tray.quit", quit_label, true, None::<&str>)?;

        // Build the context menu
        let menu = MenuBuilder::new(app)
            .item(&open)
            .item(&settings)
            .separator()
            .item(&quit)
            .build()?;

        // Build the tray icon
        let mut builder = TrayIconBuilder::with_id("uc-tray")
            .tooltip("UniClipboard")
            .show_menu_on_left_click(false)
            .menu(&menu)
            .on_menu_event(|app, event| match event.id().as_ref() {
                "tray.open" => {
                    show_main_window(app);
                }
                "tray.settings" => {
                    show_main_window(app);
                    if let Err(e) = app.emit("ui://navigate", "/settings") {
                        warn!("Failed to emit ui://navigate event: {}", e);
                    }
                }
                "tray.quit" => {
                    app.exit(0);
                }
                _ => {}
            })
            .on_tray_icon_event(|tray, event| {
                if let TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } = event
                {
                    show_main_window(tray.app_handle());
                }
            });

        // Set the tray icon from the app's default window icon
        match app.default_window_icon() {
            Some(icon) => {
                builder = builder.icon(icon.clone());
            }
            None => {
                warn!("No default window icon available for tray icon");
            }
        }

        let tray = builder.build(app)?;

        info!("System tray initialized with language: {}", language);

        *guard = Some(TrayHandles {
            tray,
            open,
            settings,
            quit,
            language: language.to_string(),
        });

        Ok(())
    }

    /// Returns `true` once the tray icon has been successfully built.
    ///
    /// Used by the main-window close handler to decide whether hiding to
    /// tray is safe — without a tray there would be no way to bring the
    /// window back, so we let the close proceed normally instead.
    pub fn is_initialized(&self) -> bool {
        self.inner
            .lock()
            .map(|guard| guard.is_some())
            .unwrap_or(false)
    }

    /// Update the tray menu labels to match the given language.
    ///
    /// If the tray has not been initialized yet, this is a no-op.
    pub fn set_language(&self, language: &str) -> tauri::Result<()> {
        let mut guard = self
            .inner
            .lock()
            .map_err(|e| tauri::Error::Anyhow(anyhow::anyhow!("TrayState lock poisoned: {}", e)))?;

        let handles = match guard.as_mut() {
            Some(h) => h,
            None => {
                debug!("Tray not initialized, skipping language update");
                return Ok(());
            }
        };

        let language = normalize_language(language);
        let (open_label, settings_label, quit_label) = labels_for_language(language);

        handles.open.set_text(open_label)?;
        handles.settings.set_text(settings_label)?;
        handles.quit.set_text(quit_label)?;
        handles.language = language.to_string();

        debug!("Tray language updated to: {}", language);
        Ok(())
    }
}

/// Show the main window: make Dock icon visible on macOS, then unminimize, show, and focus.
pub fn show_main_window(app: &tauri::AppHandle) {
    #[cfg(target_os = "macos")]
    if let Err(error) = app.set_dock_visibility(true) {
        warn!(error = %error, "Failed to show Dock icon before showing main window");
    }

    match app.get_webview_window("main") {
        Some(window) => {
            let _ = window.unminimize();
            let _ = window.show();
            let _ = window.set_focus();
        }
        None => {
            warn!("Main window not found");
        }
    }
}

/// Normalize a language string to a supported locale.
///
/// If the language starts with "zh" (case-insensitive), returns `"zh-CN"`.
/// Otherwise returns `"en-US"`.
fn normalize_language(language: &str) -> &'static str {
    if language.to_lowercase().starts_with("zh") {
        "zh-CN"
    } else {
        "en-US"
    }
}

/// Return `(open, settings, quit)` labels for the given normalized language.
fn labels_for_language(language: &str) -> (&'static str, &'static str, &'static str) {
    match language {
        "zh-CN" => ("打开 UniClipboard", "设置", "退出"),
        _ => ("Open UniClipboard", "Settings", "Quit"),
    }
}
