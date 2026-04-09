// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::webview::PageLoadEvent;
use tauri::Manager;
use tauri_plugin_autostart::MacosLauncher;
use tauri_plugin_global_shortcut;
use tauri_plugin_shell;
use tauri_plugin_single_instance;
use tauri_plugin_stronghold;
use tracing::{error, info, warn};

use uc_bootstrap::GuiBootstrapContext;
use uc_daemon_client::DaemonConnectionState;
use uc_daemon_local::daemon_lifecycle::GuiOwnedDaemonState;
use uc_tauri::bootstrap::{
    bootstrap_daemon_connection, ensure_default_device_name, start_background_tasks,
    start_gui_pairing_lease_task, supervise_daemon, AppRuntime,
};
use uc_tauri::commands::updater::PendingUpdate;
use uc_tauri::tray::TrayState;

// Platform-specific command modules
mod plugins;

use uc_tauri::quick_panel;

const DAEMON_EXIT_CLEANUP_TIMEOUT: Duration = Duration::from_secs(3);
const DAEMON_EXIT_CLEANUP_POLL_INTERVAL: Duration = Duration::from_millis(100);

#[cfg(target_os = "windows")]
fn configure_main_window_for_platform(app: &tauri::AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        warn!("Main window not found during Windows window configuration");
        return;
    };

    if let Err(error) = window.set_decorations(false) {
        warn!(error = %error, "Failed to disable Windows main window decorations");
    }
}

#[cfg(not(target_os = "windows"))]
fn configure_main_window_for_platform(_app: &tauri::AppHandle) {}

fn main() {
    // Tracing and config are handled inside build_gui_app()
    let ctx = match uc_bootstrap::build_gui_app() {
        Ok(ctx) => ctx,
        Err(e) => {
            eprintln!("Bootstrap failed: {}", e);
            std::process::exit(1);
        }
    };

    run_app(ctx);
}

/// Run the Tauri application
fn run_app(ctx: GuiBootstrapContext) {
    use tauri::Builder;

    // Destructure context -- deps, orchestrators all come from build_gui_app()
    let GuiBootstrapContext {
        deps,
        background,
        setup_ports,
        storage_paths,
        pairing_orchestrator: _pairing_orchestrator,
        pairing_action_rx: _pairing_action_rx,
        staged_store: _staged_store,
        key_slot_store: _key_slot_store,
        config: _config,
    } = ctx;

    let daemon_connection_state = DaemonConnectionState::default();
    let gui_owned_daemon_state = GuiOwnedDaemonState::default();

    let event_emitter: std::sync::Arc<dyn uc_core::ports::HostEventEmitterPort> =
        std::sync::Arc::new(uc_bootstrap::LoggingHostEventEmitter);
    let runtime = AppRuntime::with_setup(deps, setup_ports, storage_paths, event_emitter)
        .with_clipboard_write_coordinator(background.clipboard_write_coordinator.clone());
    let runtime = Arc::new(runtime);

    // Startup barrier used to coordinate backend readiness and main window show timing.
    let startup_barrier = Arc::new(uc_tauri::commands::startup::StartupBarrier::default());

    let disable_single_instance = std::env::var("UC_DISABLE_SINGLE_INSTANCE").as_deref() == Ok("1");

    // Store TaskRegistry reference for exit hook registration
    let task_registry = runtime.task_registry().clone();
    let builder = Builder::default()
        // Register AppRuntime for Tauri commands
        .manage(runtime.clone())
        .manage(DaemonConnectionState::clone(&daemon_connection_state))
        .manage(GuiOwnedDaemonState::clone(&gui_owned_daemon_state))
        .manage(TrayState::default())
        .manage(task_registry.clone())
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    api.prevent_close();
                    let _ = window.hide();
                    #[cfg(target_os = "macos")]
                    if let Err(error) = window.app_handle().set_dock_visibility(false) {
                        warn!(error = %error, "Failed to hide Dock icon after hiding main window");
                    }
                    info!("Main window hidden to tray");
                }
            }
        })
        .on_page_load(move |webview, payload| {
            if webview.label() != "main" {
                return;
            }

            let event_label = match payload.event() {
                PageLoadEvent::Started => "started",
                PageLoadEvent::Finished => "finished",
            };

            info!(
                window_label = webview.label(),
                event = event_label,
                url = %payload.url(),
                "[StartupTiming] main webview page load"
            );

            if matches!(payload.event(), PageLoadEvent::Finished) {}
        })
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_shell::init());

    let builder = if disable_single_instance {
        info!("UC_DISABLE_SINGLE_INSTANCE=1 set; skipping single-instance plugin registration");
        builder
    } else {
        builder.plugin(tauri_plugin_single_instance::init(|_app, _args, _cwd| {}))
    };

    let task_registry_for_run = task_registry.clone();
    let gui_owned_daemon_state_for_run = gui_owned_daemon_state.clone();

    builder
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec![]),
        ))
        .plugin(
            tauri_plugin_stronghold::Builder::new(move |key| {
                // Use a simple password hash function
                // In production, this should use Argon2 or similar
                key.as_bytes().to_vec()
            })
            .build(),
        )
        .setup(move |app| {
            // Set AppHandle on runtime so it can emit events to frontend
            // In Tauri 2, use app.handle() to get the AppHandle
            runtime.set_app_handle(app.handle().clone());
            info!("AppHandle set on AppRuntime for event emission");
            configure_main_window_for_platform(app.handle());

            let daemon_connection_state_for_setup = daemon_connection_state.clone();
            let gui_owned_daemon_state_for_setup = gui_owned_daemon_state.clone();
            let app_handle_for_daemon = app.handle().clone();
            let supervisor_token = task_registry.token().clone();
            let runtime_for_daemon = runtime.clone();
            tauri::async_runtime::spawn(async move {
                match bootstrap_daemon_connection(
                    &app_handle_for_daemon,
                    &daemon_connection_state_for_setup,
                    &gui_owned_daemon_state_for_setup,
                )
                .await
                {
                    Ok(_connection_info) => {
                        start_gui_pairing_lease_task(
                            daemon_connection_state_for_setup.clone(),
                            runtime_for_daemon.task_registry(),
                        );

                        // Start daemon supervisor to respawn if daemon dies unexpectedly.
                        let supervisor_state = daemon_connection_state_for_setup.clone();
                        let supervisor_daemon = gui_owned_daemon_state_for_setup.clone();
                        let app_handle_for_supervisor = app_handle_for_daemon.clone();
                        tauri::async_runtime::spawn(async move {
                            supervise_daemon(
                                &app_handle_for_supervisor,
                                &supervisor_state,
                                &supervisor_daemon,
                                supervisor_token,
                            )
                            .await;
                        });
                    }
                    Err(error) => {
                        error!(error = %error, "Daemon startup/probe failed during Tauri bootstrap");
                    }
                }
            });

            // Load startup settings for tray and silent start
            let (silent_start, initial_language) = {
                let settings_port = runtime.settings_port();
                match tauri::async_runtime::block_on(settings_port.load()) {
                    Ok(settings) => {
                        let silent = settings.general.silent_start;
                        let lang = settings.general.language.unwrap_or_default();
                        (silent, lang)
                    }
                    Err(e) => {
                        warn!("Failed to load settings for startup: {}, using defaults", e);
                        (false, "en-US".to_string())
                    }
                }
            };

            // Initialize system tray
            let tray_state = app.state::<TrayState>();
            if let Err(e) = tray_state.init(app.handle(), &initial_language) {
                error!("Failed to initialize system tray: {}", e);
                // Non-fatal: continue startup without tray
            }

            #[cfg(target_os = "macos")]
            if let Err(error) = app.handle().set_dock_visibility(false) {
                warn!(error = %error, "Failed to hide Dock icon during startup");
            }

            // Register global shortcut plugin (empty — shortcuts registered dynamically)
            #[cfg(desktop)]
            {
                app.handle()
                    .plugin(tauri_plugin_global_shortcut::Builder::new().build())?;

                // Read shortcut override from settings, or use default
                let shortcuts = {
                    let settings_port = runtime.settings_port();
                    match tauri::async_runtime::block_on(settings_port.load()) {
                        Ok(settings) => quick_panel::resolve_shortcut_from_settings(&settings),
                        Err(e) => {
                            warn!("Failed to load settings for shortcut: {}, using default", e);
                            vec![quick_panel::DEFAULT_SHORTCUT.to_string()]
                        }
                    }
                };

                for shortcut_str in &shortcuts {
                    if let Err(e) = quick_panel::register_global_shortcut(app.handle(), shortcut_str) {
                        tracing::error!(error = %e, shortcut = %shortcut_str, "Failed to register global shortcut during startup");
                    }
                }
            }

            // Pre-create quick panel (hidden) so the first
            // shortcut press doesn't activate the app via WebviewWindowBuilder::build()
            quick_panel::pre_create(app.handle());

            // Show window based on silent_start setting
            if !silent_start {
                uc_tauri::tray::show_main_window(app.handle());
                info!("Main window show requested (silent_start=false)");
            } else {
                info!("Silent start enabled, main window stays hidden");
            }

            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            app.handle()
                .plugin(tauri_plugin_updater::Builder::new().build())?;

            app.manage(PendingUpdate(Mutex::new(None)));

            // Start file cache cleanup task (runs once at startup)
            start_background_tasks(
                runtime.wiring_deps().settings.clone(),
                background.file_cache_dir.clone(),
                runtime.task_registry(),
            );

            // Clone handles for async blocks
            let app_handle_for_startup = app.handle().clone();
            let startup_barrier_for_backend = startup_barrier.clone();

            // Spawn the initialization task immediately (don't wait for frontend)
            let runtime = runtime.clone();
            let silent_start_for_barrier = silent_start;
            tauri::async_runtime::spawn(async move {
                info!("Starting backend initialization");

                // 0. Ensure device name is initialized (runs on every startup)
                if let Err(e) = ensure_default_device_name(runtime.settings_port()).await {
                    warn!("Failed to initialize default device name: {}", e);
                    // Non-fatal: continue startup even if device name initialization fails
                }

                // Mark backend-side startup tasks completed. We now finish startup based on backend readiness
                // to avoid deadlocks when the main window is hidden; frontend handles its own loading state.
                info!("[Startup] Backend startup tasks completed, marking backend_ready");
                startup_barrier_for_backend.mark_backend_ready();
                if !silent_start_for_barrier {
                    startup_barrier_for_backend.try_finish(&app_handle_for_startup);
                } else {
                    info!("[Startup] Silent start: skipping startup barrier window show");
                }

                // 1. Auto-unlock (non-blocking) via daemon API if enabled in settings
                let runtime_for_auto_unlock = runtime.clone();
                let daemon_conn_for_unlock = daemon_connection_state.clone();
                tauri::async_runtime::spawn(async move {
                    let auto_unlock_enabled =
                        match runtime_for_auto_unlock.settings_port().load().await {
                            Ok(settings) => settings.security.auto_unlock_enabled,
                            Err(e) => {
                                warn!("[Startup] Failed to load settings for auto unlock: {}", e);
                                false
                            }
                        };

                    if !auto_unlock_enabled {
                        info!("[Startup] Auto unlock disabled by settings");
                        return;
                    }

                    let client = uc_daemon_client::DaemonQueryClient::new(daemon_conn_for_unlock);
                    match client.unlock_encryption().await {
                        Ok(true) => {
                            info!("[Startup] Daemon encryption auto-unlocked");
                        }
                        Ok(false) => {
                            info!("[Startup] Encryption not initialized, skip");
                        }
                        Err(e) => {
                            warn!("[Startup] Daemon encryption unlock failed: {}", e);
                            return;
                        }
                    }

                    if let Err(e) = client.lifecycle_retry().await {
                        warn!("[Startup] Daemon lifecycle retry failed: {}", e);
                    } else {
                        info!("[Startup] Daemon lifecycle boot completed");
                    }
                });
            });

            info!("App runtime initialized, backend initialization started");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // Tray commands
            uc_tauri::commands::tray::set_tray_language,
            // Lifecycle commands
            uc_tauri::commands::get_tauri_pid,
            uc_tauri::commands::get_device_id,
            uc_tauri::commands::get_daemon_connection_info,
            // Autostart commands
            uc_tauri::commands::autostart::enable_autostart,
            uc_tauri::commands::autostart::disable_autostart,
            uc_tauri::commands::autostart::is_autostart_enabled,
            // Updater commands
            uc_tauri::commands::updater::check_for_update,
            uc_tauri::commands::updater::install_update,
            // Storage commands
            uc_tauri::commands::storage::open_data_directory,
            // macOS-specific commands (conditionally compiled)
            #[cfg(target_os = "macos")]
            plugins::mac_rounded_corners::enable_rounded_corners,
            #[cfg(target_os = "macos")]
            plugins::mac_rounded_corners::enable_modern_window_style,
            #[cfg(target_os = "macos")]
            plugins::mac_rounded_corners::reposition_traffic_lights,
            // Quick panel commands
            uc_tauri::commands::quick_panel::paste_to_previous_app,
            uc_tauri::commands::quick_panel::dismiss_quick_panel,
            uc_tauri::commands::quick_panel::set_quick_panel_layout,
            uc_tauri::commands::quick_panel::finalize_quick_panel_show,
        ])
        .build(tauri::generate_context!())
        .expect("error building tauri application")
        .run(move |app_handle, event| {
            match event {
                tauri::RunEvent::ExitRequested { api, .. } => {
                    info!("App exit requested, cancelling all tracked tasks");
                    task_registry_for_run.token().cancel();

                    if gui_owned_daemon_state_for_run.exit_cleanup_in_progress() {
                        api.prevent_exit();
                        info!("GUI-owned daemon exit cleanup already in progress");
                        return;
                    }

                    if gui_owned_daemon_state_for_run.snapshot_pid().is_none() {
                        return;
                    }

                    if !gui_owned_daemon_state_for_run.begin_exit_cleanup() {
                        api.prevent_exit();
                        info!("Skipping duplicate GUI-owned daemon exit cleanup request");
                        return;
                    }

                    api.prevent_exit();
                    let app_handle = app_handle.clone();
                    let gui_owned_daemon_state = gui_owned_daemon_state_for_run.clone();
                    tauri::async_runtime::spawn(async move {
                        match gui_owned_daemon_state
                            .shutdown_owned_daemon(
                                DAEMON_EXIT_CLEANUP_TIMEOUT,
                                DAEMON_EXIT_CLEANUP_POLL_INTERVAL,
                            )
                            .await
                        {
                            Ok(true) => {
                                info!("GUI-owned daemon cleaned up before application exit");
                            }
                            Ok(false) => {
                                info!("No GUI-owned daemon cleanup required on application exit");
                            }
                            Err(error) => {
                                error!(
                                    error = %error,
                                    "Failed to clean up GUI-owned daemon during application exit"
                                );
                            }
                        }

                        gui_owned_daemon_state.finish_exit_cleanup();
                        app_handle.exit(0);
                    });
                }
                tauri::RunEvent::Exit => {
                    info!("Application exiting");
                }
                _ => {}
            }
        });
}
