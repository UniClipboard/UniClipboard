// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::http::header::{
    HeaderValue, ACCESS_CONTROL_ALLOW_METHODS, ACCESS_CONTROL_ALLOW_ORIGIN, CONTENT_TYPE,
};
use tauri::http::{Request, Response, StatusCode};
use tauri::webview::PageLoadEvent;
use tauri::Manager;
use tauri_plugin_autostart::MacosLauncher;
use tauri_plugin_global_shortcut;
use tauri_plugin_single_instance;
use tauri_plugin_stronghold;
use tracing::{error, info, warn};

use uc_bootstrap::GuiBootstrapContext;
use uc_core::ports::ClipboardChangeHandler;
use uc_daemon_client::{realtime::install_daemon_setup_pairing_facade, DaemonConnectionState};
use uc_platform::ipc::PlatformCommand;
use uc_platform::ports::PlatformCommandExecutorPort;
use uc_platform::runtime::runtime::PlatformRuntime;
use uc_tauri::bootstrap::{
    bootstrap_daemon_connection, emit_daemon_connection_info_if_ready, ensure_default_device_name,
    start_background_tasks, supervise_daemon, AppRuntime, GuiOwnedDaemonState,
};
use uc_tauri::commands::updater::PendingUpdate;
use uc_tauri::protocol::{parse_uc_request, UcRoute};
use uc_tauri::tray::TrayState;

// Platform-specific command modules
mod plugins;

use uc_tauri::preview_panel;
use uc_tauri::quick_panel;

const DAEMON_EXIT_CLEANUP_TIMEOUT: Duration = Duration::from_secs(3);
const DAEMON_EXIT_CLEANUP_POLL_INTERVAL: Duration = Duration::from_millis(100);

/// Simple executor for platform commands
///
/// This is a placeholder implementation that logs commands.
/// In a full implementation, this would execute the actual platform commands.
struct SimplePlatformCommandExecutor;

#[async_trait::async_trait]
impl PlatformCommandExecutorPort for SimplePlatformCommandExecutor {
    async fn execute(&self, command: PlatformCommand) -> anyhow::Result<()> {
        // For now, just acknowledge the command
        // TODO: Implement actual command execution in future tasks
        match command {
            PlatformCommand::StartClipboardWatcher => {
                info!("StartClipboardWatcher command received");
            }
            PlatformCommand::StopClipboardWatcher => {
                info!("StopClipboardWatcher command received");
            }
            PlatformCommand::ReadClipboard => {
                info!("ReadClipboard command received (not implemented)");
            }
            PlatformCommand::WriteClipboard { .. } => {
                info!("WriteClipboard command received (not implemented)");
            }
            PlatformCommand::Shutdown => {
                info!("Shutdown command received (not implemented)");
            }
        }
        Ok(())
    }
}

fn is_allowed_cors_origin(origin: &str) -> bool {
    origin == "tauri://localhost"
        || origin == "http://tauri.localhost"
        || origin == "https://tauri.localhost"
        || origin.starts_with("http://localhost:")
        || origin.starts_with("http://127.0.0.1:")
        || origin.starts_with("http://[::1]:")
}

fn set_cors_headers(response: &mut Response<Vec<u8>>, origin: Option<&str>) {
    let origin = match origin {
        Some(origin) if is_allowed_cors_origin(origin) => origin,
        _ => return,
    };

    match HeaderValue::from_str(origin) {
        Ok(value) => {
            response
                .headers_mut()
                .insert(ACCESS_CONTROL_ALLOW_ORIGIN, value);
        }
        Err(err) => {
            error!(error = %err, "Invalid origin for CORS response");
        }
    }

    if let Ok(value) = HeaderValue::from_str("GET") {
        response
            .headers_mut()
            .insert(ACCESS_CONTROL_ALLOW_METHODS, value);
    }
}

fn build_response(
    status: StatusCode,
    content_type: Option<&str>,
    body: Vec<u8>,
    origin: Option<&str>,
) -> Response<Vec<u8>> {
    let mut response = Response::new(body);
    *response.status_mut() = status;

    if let Some(content_type) = content_type {
        match HeaderValue::from_str(content_type) {
            Ok(value) => {
                response.headers_mut().insert(CONTENT_TYPE, value);
            }
            Err(err) => {
                error!(error = %err, "Invalid content type for response");
            }
        }
    }

    set_cors_headers(&mut response, origin);

    response
}

fn text_response(status: StatusCode, message: &str, origin: Option<&str>) -> Response<Vec<u8>> {
    build_response(
        status,
        Some("text/plain"),
        message.as_bytes().to_vec(),
        origin,
    )
}

async fn resolve_uc_request(
    app_handle: tauri::AppHandle,
    request: Request<Vec<u8>>,
) -> Response<Vec<u8>> {
    let uri = request.uri();
    let host = uri.host().unwrap_or_default();
    let path = uri.path();
    let origin = request
        .headers()
        .get("Origin")
        .and_then(|value| value.to_str().ok());

    let route = match parse_uc_request(&request) {
        Ok(route) => route,
        Err(err) => {
            error!(
                error = %err,
                host = %host,
                path = %path,
                "Failed to parse uc URI request"
            );
            return text_response(err.status_code(), err.response_message(), origin);
        }
    };

    match route {
        UcRoute::Blob { blob_id } => resolve_uc_blob_request(app_handle, blob_id, origin).await,
        UcRoute::Thumbnail { representation_id } => {
            resolve_uc_thumbnail_request(app_handle, representation_id, origin).await
        }
    }
}

async fn resolve_uc_blob_request(
    app_handle: tauri::AppHandle,
    blob_id: uc_core::BlobId,
    origin: Option<&str>,
) -> Response<Vec<u8>> {
    let runtime = match app_handle.try_state::<Arc<AppRuntime>>() {
        Some(state) => state,
        None => {
            error!("AppRuntime state not managed for uc URI handling");
            return text_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Runtime not ready",
                origin,
            );
        }
    };

    let use_case = runtime.usecases().resolve_blob_resource();
    match use_case.execute(&blob_id).await {
        Ok(result) => build_response(
            StatusCode::OK,
            Some(
                result
                    .mime_type
                    .as_deref()
                    .unwrap_or("application/octet-stream"),
            ),
            result.bytes,
            origin,
        ),
        Err(err) => {
            let err_msg = err.to_string();
            error!(error = %err, blob_id = %blob_id, "Failed to resolve blob resource");
            let status = if err_msg.contains("not found") {
                StatusCode::NOT_FOUND
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            text_response(status, "Failed to resolve blob resource", origin)
        }
    }
}

async fn resolve_uc_thumbnail_request(
    app_handle: tauri::AppHandle,
    representation_id: uc_core::ids::RepresentationId,
    origin: Option<&str>,
) -> Response<Vec<u8>> {
    let runtime = match app_handle.try_state::<Arc<AppRuntime>>() {
        Some(state) => state,
        None => {
            error!("AppRuntime state not managed for uc URI handling");
            return text_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Runtime not ready",
                origin,
            );
        }
    };

    let use_case = runtime.usecases().resolve_thumbnail_resource();
    match use_case.execute(&representation_id).await {
        Ok(result) => build_response(
            StatusCode::OK,
            Some(
                result
                    .mime_type
                    .as_deref()
                    .unwrap_or("application/octet-stream"),
            ),
            result.bytes,
            origin,
        ),
        Err(err) => {
            let err_msg = err.to_string();
            error!(
                error = %err,
                representation_id = %representation_id,
                "Failed to resolve thumbnail resource"
            );
            let status = if err_msg.contains("not found") {
                StatusCode::NOT_FOUND
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            text_response(status, "Failed to resolve thumbnail resource", origin)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cors_headers_are_set_for_dev_origin() {
        let origin = "http://localhost:1420";
        let response = build_response(StatusCode::OK, None, vec![], Some(origin));

        let headers = response.headers();
        assert_eq!(
            headers
                .get(ACCESS_CONTROL_ALLOW_ORIGIN)
                .and_then(|value| value.to_str().ok()),
            Some(origin)
        );
        assert_eq!(
            headers
                .get(ACCESS_CONTROL_ALLOW_METHODS)
                .and_then(|value| value.to_str().ok()),
            Some("GET")
        );
    }

    #[test]
    fn test_cors_headers_not_set_for_untrusted_origin() {
        let response = build_response(StatusCode::OK, None, vec![], Some("https://example.com"));

        let headers = response.headers();
        assert!(headers.get(ACCESS_CONTROL_ALLOW_ORIGIN).is_none());
    }
}

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

    // Destructure context -- channels, deps, orchestrators all come from build_gui_app()
    let GuiBootstrapContext {
        deps,
        background,
        watcher_control,
        setup_ports,
        storage_paths,
        platform_event_tx,
        platform_event_rx,
        platform_cmd_tx: _,
        platform_cmd_rx,
        pairing_orchestrator: _pairing_orchestrator,
        pairing_action_rx: _pairing_action_rx,
        staged_store: _staged_store,
        key_slot_store: _key_slot_store,
        config: _config,
    } = ctx;

    let daemon_connection_state = DaemonConnectionState::default();
    let gui_owned_daemon_state = GuiOwnedDaemonState::default();
    let mut setup_ports = setup_ports;
    let setup_pairing_event_hub =
        install_daemon_setup_pairing_facade(&mut setup_ports, daemon_connection_state.clone());

    let event_emitter: std::sync::Arc<dyn uc_core::ports::HostEventEmitterPort> =
        std::sync::Arc::new(uc_tauri::adapters::host_event_emitter::LoggingEventEmitter);
    let runtime = AppRuntime::with_setup(
        deps,
        setup_ports,
        watcher_control,
        storage_paths,
        event_emitter,
    );

    // Wrap runtime in Arc for clipboard handler (PlatformRuntime needs Arc<dyn ClipboardChangeHandler>)
    let runtime_for_handler = Arc::new(runtime);

    // Clone Arc for Tauri state management (will have app_handle injected in setup)
    let runtime_for_tauri = runtime_for_handler.clone();

    // Startup barrier used to coordinate backend readiness and main window show timing.
    let startup_barrier = Arc::new(uc_tauri::commands::startup::StartupBarrier::default());

    // Create clipboard handler from runtime (AppRuntime implements ClipboardChangeHandler)
    let clipboard_handler: Arc<dyn ClipboardChangeHandler> = runtime_for_handler.clone();

    info!("Creating platform runtime with clipboard callback");

    // Note: PlatformRuntime will be started in setup block
    // The actual startup will be completed in a follow-up task

    let disable_single_instance = std::env::var("UC_DISABLE_SINGLE_INSTANCE").as_deref() == Ok("1");

    // Store TaskRegistry reference for exit hook registration
    let task_registry = runtime_for_handler.task_registry().clone();
    let startup_barrier_for_page_load = startup_barrier.clone();
    let daemon_connection_state_for_page_load = daemon_connection_state.clone();

    let builder = Builder::default()
        // Register AppRuntime for Tauri commands
        .manage(runtime_for_tauri)
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

            if matches!(payload.event(), PageLoadEvent::Finished) {
                startup_barrier_for_page_load.mark_frontend_ready();
                if let Err(error) = emit_daemon_connection_info_if_ready(
                    &webview.app_handle(),
                    &daemon_connection_state_for_page_load,
                    &startup_barrier_for_page_load,
                ) {
                    error!(
                        error = %error,
                        "Failed to emit daemon connection info after main webview load"
                    );
                }
            }
        })
        .register_asynchronous_uri_scheme_protocol("uc", move |ctx, request, responder| {
            let app_handle = ctx.app_handle().clone();
            tauri::async_runtime::spawn(async move {
                let response = resolve_uc_request(app_handle, request).await;
                responder.respond(response);
            });
        })
        // Manual verification (dev):
        // 1) In frontend devtools: fetch("uc://blob/<blob_id>")
        // 2) In frontend devtools: fetch("uc://thumbnail/<representation_id>")
        // 3) Network should show 200 with Access-Control-Allow-Origin matching http://localhost:1420
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init());

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
            runtime_for_handler.set_app_handle(app.handle().clone());
            info!("AppHandle set on AppRuntime for event emission");

            // Swap event emitter from LoggingEventEmitter to TauriEventEmitter
            // now that AppHandle is available
            let tauri_emitter: std::sync::Arc<dyn uc_core::ports::HostEventEmitterPort> =
                std::sync::Arc::new(uc_tauri::adapters::host_event_emitter::TauriEventEmitter::new(
                    app.handle().clone(),
                ));
            runtime_for_handler.set_event_emitter(tauri_emitter);
            info!("Event emitter swapped to TauriEventEmitter");

            let daemon_connection_state_for_setup = daemon_connection_state.clone();
            let gui_owned_daemon_state_for_setup = gui_owned_daemon_state.clone();
            let startup_barrier_for_daemon = startup_barrier.clone();
            let app_handle_for_daemon = app.handle().clone();
            let supervisor_token = task_registry.token().clone();
            tauri::async_runtime::spawn(async move {
                match bootstrap_daemon_connection(
                    &daemon_connection_state_for_setup,
                    &gui_owned_daemon_state_for_setup,
                )
                .await
                {
                    Ok(_connection_info) => {
                        if let Err(error) = emit_daemon_connection_info_if_ready(
                            &app_handle_for_daemon,
                            &daemon_connection_state_for_setup,
                            &startup_barrier_for_daemon,
                        ) {
                            warn!(error = %error, "Failed to deliver daemon connection info to main webview");
                        }

                        // Start daemon supervisor to respawn if daemon dies unexpectedly.
                        let supervisor_state = daemon_connection_state_for_setup.clone();
                        let supervisor_daemon = gui_owned_daemon_state_for_setup.clone();
                        tauri::async_runtime::spawn(async move {
                            supervise_daemon(
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
                let settings_port = runtime_for_handler.settings_port();
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
                    let settings_port = runtime_for_handler.settings_port();
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

            // Pre-create quick panel and preview panel (hidden) so the first
            // shortcut press doesn't activate the app via WebviewWindowBuilder::build()
            quick_panel::pre_create(app.handle());
            preview_panel::pre_create(app.handle());

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

            // Start background spooler and blob worker tasks
            start_background_tasks(
                background,
                runtime_for_handler.wiring_deps(),
                runtime_for_handler.event_emitter(),
                daemon_connection_state.clone(),
                setup_pairing_event_hub.clone(),
                runtime_for_handler.task_registry(),
            );

            // Clone handles for async blocks
            let app_handle_for_startup = app.handle().clone();
            let startup_barrier_for_backend = startup_barrier.clone();

            // Spawn the initialization task immediately (don't wait for frontend)
            let runtime = runtime_for_handler.clone();
            let platform_event_tx_clone = platform_event_tx.clone();
            let silent_start_for_barrier = silent_start;
            tauri::async_runtime::spawn(async move {
                info!("Starting backend initialization");

                // 0. Ensure device name is initialized (runs on every startup)
                if let Err(e) = ensure_default_device_name(runtime.settings_port()).await {
                    warn!("Failed to initialize default device name: {}", e);
                    // Non-fatal: continue startup even if device name initialization fails
                }

                // 1. Create PlatformRuntime
                info!("Creating PlatformRuntime...");
                let executor = Arc::new(SimplePlatformCommandExecutor);
                let platform_runtime = match PlatformRuntime::new(
                    platform_event_tx_clone,
                    platform_event_rx,
                    platform_cmd_rx,
                    executor,
                    Some(clipboard_handler),
                ) {
                    Ok(rt) => {
                        info!("PlatformRuntime created successfully");
                        rt
                    }
                    Err(e) => {
                        error!("Failed to create platform runtime: {}", e);
                        return;
                    }
                };

                // Mark backend-side startup tasks completed. We now finish startup based on backend readiness
                // to avoid deadlocks when the main window is hidden; frontend handles its own loading state.
                info!("[Startup] Backend startup tasks completed, marking backend_ready");
                startup_barrier_for_backend.mark_backend_ready();
                if !silent_start_for_barrier {
                    startup_barrier_for_backend.try_finish(&app_handle_for_startup);
                } else {
                    info!("[Startup] Silent start: skipping startup barrier window show");
                }

                // 2. Auto-unlock (non-blocking) if enabled in settings
                let runtime_for_auto_unlock = runtime.clone();
                let app_handle_for_unlock = app_handle_for_startup.clone();
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

                    if let Err(e) =
                        uc_tauri::commands::encryption::unlock_encryption_session_with_runtime(
                            &runtime_for_auto_unlock,
                            &app_handle_for_unlock,
                            None,
                        )
                        .await
                    {
                        warn!("[Startup] Auto unlock failed: {}", e);
                    }
                });

                // 3. Start platform runtime (this is an infinite loop that runs until app exits)
                platform_runtime.start().await;

                info!("Platform runtime task ended");
            });

            info!("App runtime initialized, backend initialization started");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // Clipboard commands
            uc_tauri::commands::clipboard::get_clipboard_entries,
            uc_tauri::commands::clipboard::get_clipboard_entry,
            uc_tauri::commands::clipboard::get_clipboard_entry_detail,
            uc_tauri::commands::clipboard::get_clipboard_entry_resource,
            uc_tauri::commands::clipboard::delete_clipboard_entry,
            uc_tauri::commands::clipboard::restore_clipboard_entry,
            uc_tauri::commands::clipboard::sync_clipboard_items,
            uc_tauri::commands::clipboard::get_clipboard_stats,
            uc_tauri::commands::clipboard::toggle_favorite_clipboard_item,
            uc_tauri::commands::clipboard::get_clipboard_item,
            uc_tauri::commands::clipboard::copy_file_to_clipboard,
            // Encryption commands
            uc_tauri::commands::encryption::initialize_encryption,
            uc_tauri::commands::encryption::get_encryption_session_status,
            uc_tauri::commands::encryption::unlock_encryption_session,
            uc_tauri::commands::encryption::verify_keychain_access,
            // Settings commands
            uc_tauri::commands::settings::get_settings,
            uc_tauri::commands::settings::update_settings,
            // Setup commands
            uc_tauri::commands::setup::get_setup_state,
            uc_tauri::commands::setup::start_new_space,
            uc_tauri::commands::setup::start_join_space,
            uc_tauri::commands::setup::select_device,
            uc_tauri::commands::setup::submit_passphrase,
            uc_tauri::commands::setup::verify_passphrase,
            uc_tauri::commands::setup::confirm_peer_trust,
            uc_tauri::commands::setup::cancel_setup,
            uc_tauri::commands::setup::handle_space_access_completed,
            // Pairing commands
            uc_tauri::commands::pairing::get_local_peer_id,
            uc_tauri::commands::pairing::get_p2p_peers,
            uc_tauri::commands::pairing::get_local_device_info,
            uc_tauri::commands::pairing::get_paired_peers,
            uc_tauri::commands::pairing::get_paired_peers_with_status,
            uc_tauri::commands::pairing::initiate_p2p_pairing,
            uc_tauri::commands::pairing::verify_p2p_pairing_pin,
            uc_tauri::commands::pairing::reject_p2p_pairing,
            uc_tauri::commands::pairing::accept_p2p_pairing,
            uc_tauri::commands::pairing::unpair_p2p_device,
            uc_tauri::commands::pairing::list_paired_devices,
            uc_tauri::commands::pairing::set_pairing_state,
            uc_tauri::commands::pairing::get_device_sync_settings,
            uc_tauri::commands::pairing::update_device_sync_settings,
            // Tray commands
            uc_tauri::commands::tray::set_tray_language,
            // Lifecycle commands
            uc_tauri::commands::lifecycle::retry_lifecycle,
            uc_tauri::commands::lifecycle::get_lifecycle_status,
            // Autostart commands
            uc_tauri::commands::autostart::enable_autostart,
            uc_tauri::commands::autostart::disable_autostart,
            uc_tauri::commands::autostart::is_autostart_enabled,
            // Updater commands
            uc_tauri::commands::updater::check_for_update,
            uc_tauri::commands::updater::install_update,
            // Storage commands
            uc_tauri::commands::storage::get_storage_stats,
            uc_tauri::commands::storage::clear_cache,
            uc_tauri::commands::storage::clear_all_clipboard_history,
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
            // Preview panel commands
            uc_tauri::commands::preview_panel::show_preview_panel,
            uc_tauri::commands::preview_panel::dismiss_preview_panel,
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
