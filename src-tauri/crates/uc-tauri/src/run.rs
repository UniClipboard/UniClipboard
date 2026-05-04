//! Tauri shell 主入口。
//!
//! `main.rs` 在外面构造 `GuiBootstrapContext` 与 `tauri::Context`（后者由
//! `tauri::generate_context!()` 宏生成，必须在 bin crate 里），然后调用
//! [`run`] 把控制权交给 Tauri shell：装配 `TauriAppRuntime`、注册
//! plugins、启动 daemon 拉起/守护、初始化托盘、注册 commands、运行 Tauri
//! 事件循环。
//!
//! 这里是"Tauri shell 的最后一公里"——所有 GUI-framework agnostic 的
//! 桌面宿主能力（runtime 装配、后台任务调度、daemon ownership 协调状态）
//! 都已下沉到 [`uc_desktop`]，本文件只关心怎么把它们落到 Tauri 的
//! `Builder` / `setup` / `RunEvent` 上。

use std::sync::{Arc, Mutex};
use std::time::Duration;

use tauri::webview::PageLoadEvent;
use tauri::{Emitter, Manager};
use tauri_plugin_autostart::MacosLauncher;
use tracing::{error, info, warn};

use uc_daemon_client::DaemonConnectionState;
use uc_desktop::bootstrap::{build_gui_app, GuiBootstrapContext};
use uc_desktop::daemon_probe::{
    bootstrap_daemon_in_process, HEALTH_CHECK_TIMEOUT, HEALTH_POLL_INTERVAL,
    INCOMPATIBLE_DAEMON_EXIT_TIMEOUT,
};
use uc_desktop::DaemonOwnership;

use crate::bootstrap::{
    ensure_default_device_name, start_background_tasks, start_gui_pairing_lease_task,
    TauriAppRuntime,
};
use crate::commands::updater::PendingUpdate;
use crate::quick_panel;
use crate::tray::TrayState;

/// daemon shutdown 等待上限。
///
/// daemon 内部 `DaemonApp::run` 的 cleanup 序列自带兜底超时（5s
/// service_tasks join + 5s http_handle graceful join + services.stop()
/// 串行），最长 wallclock ~10s。前端会在 [`SHUTDOWN_FRONTEND_GRACE_MS`]
/// 内主动关掉 WebSocket，正常 case 整体 <1s；这里给 15s 兜底覆盖最坏路径。
const DAEMON_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(15);

/// 前端事件名——告诉 webview "马上关 daemon 了，请主动 close 你那条
/// WebSocket"。前端 `daemon-ws-bootstrap.ts` 的 listener 收到后调用
/// `daemonWs.disconnect()` 发送 close frame，让 daemon 端的 axum
/// `with_graceful_shutdown` 立即返回，不等 30s heartbeat 超时。
const FRONTEND_SHUTDOWN_EVENT: &str = "app://shutting-down";

/// 给前端响应 `app://shutting-down` 事件、发出 WebSocket close frame
/// 的时间。100ms 对单进程内 IPC + 浏览器 WebSocket close frame 飞过
/// loopback 来说极宽裕——用户感知不到这点延迟。
const SHUTDOWN_FRONTEND_GRACE_MS: u64 = 100;

/// 这个 GUI shell 期望 daemon 上报的 `packageVersion`——`probe_daemon_health`
/// 用它做版本兼容性判断。`env!` 拿的是 `uc-tauri` 自己的 cargo 版本，
/// workspace 共享版本号所以与 `uniclipboard` bin 一致。
const EXPECTED_PACKAGE_VERSION: &str = env!("CARGO_PKG_VERSION");

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

/// Run the Tauri application.
///
/// `tauri_ctx` 必须由 bin crate（`src-tauri/src/main.rs`）通过
/// `tauri::generate_context!()` 生成后传入——该宏依赖 bin 的
/// `Cargo.toml` 同目录的 `tauri.conf.json`，无法在 lib crate 里调用。
///
/// 启动期上下文（`GuiBootstrapContext`）由本函数内部通过
/// [`uc_desktop::bootstrap::build_gui_app`] 装配，bin 不需要关心装配细节。
/// 装配失败时返回 `Err`；装配成功后函数进入 Tauri 事件循环并不再返回，
/// 直到应用退出。
pub fn run(tauri_ctx: tauri::Context<tauri::Wry>) -> anyhow::Result<()> {
    let GuiBootstrapContext {
        deps,
        background,
        storage_paths,
        config: _config,
    } = build_gui_app()?;

    let daemon_connection_state = DaemonConnectionState::default();
    let daemon_ownership = DaemonOwnership::default();

    let event_emitter: std::sync::Arc<dyn uc_application::facade::HostEventEmitterPort> =
        std::sync::Arc::new(uc_bootstrap::LoggingHostEventEmitter);
    let runtime = TauriAppRuntime::with_setup(
        deps,
        storage_paths,
        event_emitter,
        background.clipboard_write_coordinator.clone(),
    );
    let runtime = Arc::new(runtime);

    // Startup barrier used to coordinate backend readiness and main window show timing.
    let startup_barrier = Arc::new(crate::commands::startup::StartupBarrier::default());

    let disable_single_instance = std::env::var("UC_DISABLE_SINGLE_INSTANCE").as_deref() == Ok("1");

    // Store TaskRegistry reference for exit hook registration
    let task_registry = runtime.task_registry().clone();
    let builder = tauri::Builder::default()
        // Register TauriAppRuntime for Tauri commands
        .manage(runtime.clone())
        .manage(DaemonConnectionState::clone(&daemon_connection_state))
        .manage(DaemonOwnership::clone(&daemon_ownership))
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
    let daemon_ownership_for_run = daemon_ownership.clone();

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
            info!("AppHandle set on TauriAppRuntime for event emission");
            configure_main_window_for_platform(app.handle());

            let daemon_connection_state_for_setup = daemon_connection_state.clone();
            let daemon_ownership_for_setup = daemon_ownership.clone();
            let runtime_for_daemon = runtime.clone();
            tauri::async_runtime::spawn(async move {
                match bootstrap_daemon_in_process(
                    &daemon_ownership_for_setup,
                    EXPECTED_PACKAGE_VERSION,
                    INCOMPATIBLE_DAEMON_EXIT_TIMEOUT,
                    HEALTH_CHECK_TIMEOUT,
                    HEALTH_POLL_INTERVAL,
                )
                .await
                {
                    Ok(connection_info) => {
                        daemon_connection_state_for_setup.set(connection_info);
                        start_gui_pairing_lease_task(
                            daemon_connection_state_for_setup.clone(),
                            runtime_for_daemon.task_registry(),
                        )
                        .await;
                        // 不再需要 daemon supervisor。in-process daemon 与
                        // GUI 进程同生死；外部 daemon 不归我们管，崩了
                        // 也由 CLI 负责重新拉起。
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

            // Register global shortcut plugin (empty — shortcuts registered dynamically).
            // `#[cfg(desktop)]` is normally injected by `tauri-build` in the bin crate;
            // here we spell it out explicitly so it compiles in this lib crate too.
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
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
                crate::tray::show_main_window(app.handle());
                info!("Main window show requested (silent_start=false)");
            } else {
                info!("Silent start enabled, main window stays hidden");
            }

            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            app.handle()
                .plugin(tauri_plugin_updater::Builder::new().build())?;

            app.manage(PendingUpdate(Mutex::new(None)));

            // Start file cache cleanup task (runs once at startup).
            // The starter is `async fn`; drive it on Tauri's managed tokio
            // runtime — `setup` itself runs on the main thread without a
            // tokio runtime context, so plain `tokio::spawn` here would
            // panic with "no reactor running".
            let history_facade_for_cleanup = runtime.app_facade().clipboard_history.clone();
            let task_registry_for_cleanup = runtime.task_registry().clone();
            tauri::async_runtime::spawn(async move {
                start_background_tasks(history_facade_for_cleanup, &task_registry_for_cleanup)
                    .await;
            });

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
            crate::commands::tray::set_tray_language,
            // Lifecycle commands
            crate::commands::get_tauri_pid,
            crate::commands::get_device_id,
            crate::commands::get_daemon_connection_info,
            // Autostart commands
            crate::commands::autostart::enable_autostart,
            crate::commands::autostart::disable_autostart,
            crate::commands::autostart::is_autostart_enabled,
            // Updater commands
            crate::commands::updater::check_for_update,
            crate::commands::updater::install_update,
            // Storage commands
            crate::commands::storage::open_data_directory,
            // macOS-specific commands (conditionally compiled)
            #[cfg(target_os = "macos")]
            crate::plugins::mac_rounded_corners::enable_rounded_corners,
            #[cfg(target_os = "macos")]
            crate::plugins::mac_rounded_corners::enable_modern_window_style,
            #[cfg(target_os = "macos")]
            crate::plugins::mac_rounded_corners::reposition_traffic_lights,
            // Quick panel commands
            crate::commands::quick_panel::paste_to_previous_app,
            crate::commands::quick_panel::dismiss_quick_panel,
            crate::commands::quick_panel::set_quick_panel_layout,
            crate::commands::quick_panel::finalize_quick_panel_show,
        ])
        .build(tauri_ctx)
        .expect("error building tauri application")
        .run(move |app_handle, event| {
            match event {
                tauri::RunEvent::ExitRequested { api, .. } => {
                    info!("App exit requested, cancelling all tracked tasks");
                    task_registry_for_run.token().cancel();

                    let Some(handle) = daemon_ownership_for_run.take_owned() else {
                        // External daemon (CLI start) 或还没拉起；GUI 直接退出，不动 daemon。
                        return;
                    };

                    api.prevent_exit();
                    let app_handle = app_handle.clone();

                    // Tell the webview to close its WebSocket *before* we ask
                    // the daemon to shut down. axum's `with_graceful_shutdown`
                    // waits for in-flight handlers — including the long-lived
                    // `/ws` upgrade — to finish. Browser WebSocket clients
                    // don't send close frames automatically when the webview
                    // is destroyed, so without this hint the daemon would
                    // wait for the 30s heartbeat timeout.
                    if let Err(error) = app_handle.emit(FRONTEND_SHUTDOWN_EVENT, ()) {
                        warn!(
                            error = %error,
                            event = FRONTEND_SHUTDOWN_EVENT,
                            "Failed to emit shutdown hint to frontend; daemon shutdown \
                             will fall back to heartbeat-driven WS disconnect"
                        );
                    }

                    tauri::async_runtime::spawn(async move {
                        // Give the webview a moment to actually send the WS
                        // close frame before we cancel the daemon.
                        tokio::time::sleep(Duration::from_millis(SHUTDOWN_FRONTEND_GRACE_MS))
                            .await;

                        match handle.shutdown(DAEMON_SHUTDOWN_TIMEOUT).await {
                            Ok(()) => {
                                info!("In-process daemon stopped before application exit");
                            }
                            Err(error) => {
                                error!(
                                    error = %error,
                                    "In-process daemon shutdown failed during application exit"
                                );
                            }
                        }
                        app_handle.exit(0);
                    });
                }
                tauri::RunEvent::Exit => {
                    info!("Application exiting");
                }
                #[cfg(target_os = "macos")]
                tauri::RunEvent::Reopen {
                    has_visible_windows,
                    ..
                } => {
                    // macOS: 点击 Dock 图标时，若没有可见窗口则恢复主窗口
                    if !has_visible_windows {
                        info!("Dock reopen with no visible windows, showing main window");
                        crate::tray::show_main_window(app_handle);
                    }
                }
                _ => {}
            }
        });

    Ok(())
}
