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
    tray: tauri::tray::TrayIcon,
    /// Phase 96 INDIC-04 状态行 —— 不可交互,用于让用户在不打开主窗口的
    /// 前提下确认 LAN-only Mode 是否已开启(差异图标 OR 状态徽章 二选一,
    /// 这里选状态文案 + tooltip 双重披露)。
    status: MenuItem<tauri::Wry>,
    open: MenuItem<tauri::Wry>,
    settings: MenuItem<tauri::Wry>,
    quit: MenuItem<tauri::Wry>,
    language: String,
    lan_only_active: bool,
}

impl TrayState {
    /// Initialize the system tray icon with a context menu.
    ///
    /// This method is idempotent: if the tray is already initialized,
    /// it returns `Ok(())` immediately.
    ///
    /// Phase 96 INDIC-04:`lan_only_active` 反映启动时的 LAN-only Mode 状态
    /// (后端 `settings.network.allow_relay_fallback == false ⇔ ON`),用于
    /// 渲染状态菜单行 + tooltip 后缀。本里程碑承担"重启生效"语义,所以
    /// 进程内不再随设置变化更新此状态(即便设置已切换,要等下次重启才生效)。
    pub fn init(
        &self,
        app: &tauri::AppHandle,
        initial_language: &str,
        lan_only_active: bool,
    ) -> tauri::Result<()> {
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
        let status_label = lan_only_status_label(language, lan_only_active);
        let tooltip = lan_only_tooltip(language, lan_only_active);

        // Create menu items with well-known IDs.
        // `tray.status` 是不可交互的状态展示行(`enabled = false`),
        // 用户右键 tray 时一眼可见 LAN-only Mode 是否已生效。
        let status = MenuItem::with_id(app, "tray.status", status_label, false, None::<&str>)?;
        let open = MenuItem::with_id(app, "tray.open", open_label, true, None::<&str>)?;
        let settings = MenuItem::with_id(app, "tray.settings", settings_label, true, None::<&str>)?;
        let quit = MenuItem::with_id(app, "tray.quit", quit_label, true, None::<&str>)?;

        // Build the context menu
        let menu = MenuBuilder::new(app)
            .item(&status)
            .separator()
            .item(&open)
            .item(&settings)
            .separator()
            .item(&quit)
            .build()?;

        // Build the tray icon
        let mut builder = TrayIconBuilder::with_id("uc-tray")
            .tooltip(&tooltip)
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

        info!(
            language = %language,
            lan_only_active,
            "System tray initialized"
        );

        *guard = Some(TrayHandles {
            tray,
            status,
            open,
            settings,
            quit,
            language: language.to_string(),
            lan_only_active,
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
        let status_label = lan_only_status_label(language, handles.lan_only_active);
        let tooltip = lan_only_tooltip(language, handles.lan_only_active);

        handles.open.set_text(open_label)?;
        handles.settings.set_text(settings_label)?;
        handles.quit.set_text(quit_label)?;
        handles.status.set_text(status_label)?;
        // Tray icon tooltip 也要随语言切换刷新。
        let _ = handles.tray.set_tooltip(Some(&tooltip));
        handles.language = language.to_string();

        debug!("Tray language updated to: {}", language);
        Ok(())
    }
}

/// Phase 96 INDIC-04:LAN-only Mode 状态文案(菜单状态行)。
fn lan_only_status_label(language: &str, lan_only_active: bool) -> &'static str {
    match (language, lan_only_active) {
        ("zh-CN", true) => "LAN-only Mode:已开启",
        ("zh-CN", false) => "LAN-only Mode:未开启",
        (_, true) => "LAN-only Mode: ON",
        (_, false) => "LAN-only Mode: OFF",
    }
}

/// Phase 96 INDIC-04:tray icon tooltip。hover 即可看到 LAN-only Mode 状态。
fn lan_only_tooltip(language: &str, lan_only_active: bool) -> String {
    match (language, lan_only_active) {
        ("zh-CN", true) => "UniClipboard — LAN-only Mode 已开启".to_string(),
        ("zh-CN", false) => "UniClipboard".to_string(),
        (_, true) => "UniClipboard — LAN-only Mode is ON".to_string(),
        (_, false) => "UniClipboard".to_string(),
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
