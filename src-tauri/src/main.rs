// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tauri::http::header::{
    HeaderValue, ACCESS_CONTROL_ALLOW_METHODS, ACCESS_CONTROL_ALLOW_ORIGIN, CONTENT_TYPE,
};
use tauri::http::{Request, Response, StatusCode};
use tauri::webview::PageLoadEvent;
use tauri::Manager;
use tauri_plugin_autostart::MacosLauncher;
use tauri_plugin_single_instance;
use tauri_plugin_stronghold;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use uc_app::usecases::{
    pairing::{PairingOrchestrator, StagedPairedDeviceStore},
    space_access::SpaceAccessOrchestrator,
};
use uc_core::config::AppConfig;
use uc_core::ports::ClipboardChangeHandler;
use uc_core::ports::PeerDirectoryPort;
use uc_infra::fs::key_slot_store::{JsonKeySlotStore, KeySlotStore};
use uc_platform::app_dirs::DirsAppDirsAdapter;
use uc_platform::ipc::PlatformCommand;
use uc_platform::ports::AppDirsPort;
use uc_platform::ports::PlatformCommandExecutorPort;
use uc_platform::runtime::event_bus::{
    PlatformCommandReceiver, PlatformEventReceiver, PlatformEventSender,
};
use uc_platform::runtime::runtime::PlatformRuntime;
use uc_tauri::bootstrap::tracing as bootstrap_tracing;
use uc_tauri::bootstrap::{
    ensure_default_device_name, load_config, resolve_pairing_config, resolve_pairing_device_name,
    start_background_tasks, wire_dependencies, AppRuntime, SetupRuntimePorts,
};
use uc_tauri::commands::updater::PendingUpdate;
use uc_tauri::protocol::{parse_uc_request, UcRoute};
use uc_tauri::tray::TrayState;

// Platform-specific command modules
mod plugins;

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
    use std::env;
    use std::fs;
    use std::sync::Mutex;
    use tempfile::TempDir;

    static CWD_TEST_LOCK: Mutex<()> = Mutex::new(());

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

    #[test]
    fn test_resolve_config_path_finds_parent_directory() {
        let _guard = CWD_TEST_LOCK.lock().unwrap();
        let temp_dir = TempDir::new().unwrap();
        let root_dir = temp_dir.path();
        let nested_dir = root_dir.join("src-tauri");
        fs::create_dir_all(&nested_dir).unwrap();
        fs::write(root_dir.join("config.toml"), "").unwrap();

        let original_dir = env::current_dir().unwrap();
        env::set_current_dir(&nested_dir).unwrap();

        let resolved = resolve_config_path().and_then(|path| fs::canonicalize(path).ok());

        env::set_current_dir(original_dir).unwrap();

        let expected = fs::canonicalize(root_dir.join("config.toml")).unwrap();
        assert_eq!(resolved, Some(expected));
    }

    #[test]
    fn test_resolve_config_path_finds_src_tauri_config_from_repo_root() {
        let _guard = CWD_TEST_LOCK.lock().unwrap();
        let temp_dir = TempDir::new().unwrap();
        let root_dir = temp_dir.path();
        let src_tauri_dir = root_dir.join("src-tauri");
        fs::create_dir_all(&src_tauri_dir).unwrap();
        fs::write(src_tauri_dir.join("config.toml"), "").unwrap();

        let original_dir = env::current_dir().unwrap();
        env::set_current_dir(root_dir).unwrap();

        let resolved = resolve_config_path().and_then(|path| fs::canonicalize(path).ok());

        env::set_current_dir(original_dir).unwrap();

        let expected = fs::canonicalize(src_tauri_dir.join("config.toml")).unwrap();
        assert_eq!(resolved, Some(expected));
    }
}

fn resolve_config_path() -> Option<PathBuf> {
    if let Ok(explicit) = std::env::var("UC_CONFIG_PATH") {
        let explicit_path = PathBuf::from(explicit);
        if explicit_path.is_file() {
            return Some(explicit_path);
        }
    }

    let current_dir = std::env::current_dir().ok()?;

    for ancestor in current_dir.ancestors() {
        let candidate = ancestor.join("config.toml");
        if candidate.is_file() {
            return Some(candidate);
        }

        let src_tauri_candidate = ancestor.join("src-tauri").join("config.toml");
        if src_tauri_candidate.is_file() {
            return Some(src_tauri_candidate);
        }
    }

    None
}

fn apply_profile_suffix(path: PathBuf) -> PathBuf {
    let profile = match std::env::var("UC_PROFILE") {
        Ok(value) if !value.is_empty() => value,
        _ => return path,
    };

    let file_name = match path.file_name().and_then(|name| name.to_str()) {
        Some(name) => name.to_string(),
        None => return path,
    };

    let mut updated = path;
    updated.set_file_name(format!("{file_name}_{profile}"));
    updated
}

fn resolve_keyslot_store_vault_dir(config: &AppConfig, app_data_root: PathBuf) -> PathBuf {
    if config.vault_key_path.as_os_str().is_empty() {
        return app_data_root.join("vault");
    }

    let configured_vault_root = config
        .vault_key_path
        .parent()
        .unwrap_or(&config.vault_key_path)
        .to_path_buf();

    if config.database_path.as_os_str().is_empty() {
        return apply_profile_suffix(configured_vault_root);
    }

    let configured_db_root = config
        .database_path
        .parent()
        .unwrap_or(&config.database_path)
        .to_path_buf();

    if configured_vault_root.starts_with(&configured_db_root) {
        let relative = configured_vault_root
            .strip_prefix(&configured_db_root)
            .unwrap_or(Path::new(""));
        app_data_root.join(relative)
    } else {
        apply_profile_suffix(configured_vault_root)
    }
}

/// Starts the application.
///
/// Initializes tracing, attempts to load `config.toml` (development mode), falls back to system
/// defaults using the platform app-data directory when no config file is present, and then runs
/// the Tauri application. On fatal initialization failures (tracing or app-data resolution) the
/// process exits with code 1.
///
/// # Examples
///
/// ```no_run
/// // Running the application (example; do not run in doctests)
/// crate::main();
/// ```
fn main() {
    // Initialize tracing subscriber FIRST (before any logging)
    // This sets up the tracing infrastructure and enables log-tracing bridge
    if let Err(e) = bootstrap_tracing::init_tracing_subscriber() {
        eprintln!("Failed to initialize tracing: {}", e);
        std::process::exit(1);
    }

    // NOTE: config.toml is optional and intended for development use only
    // Production environment uses system-default paths automatically

    let config_path = resolve_config_path().unwrap_or_else(|| PathBuf::from("config.toml"));

    // Load configuration using the new bootstrap flow
    let config = match load_config(config_path.clone()) {
        Ok(config) => {
            info!(
                "Loaded config from {} (development mode)",
                config_path.display()
            );
            config
        }
        Err(e) => {
            debug!("No config.toml found, using system defaults: {}", e);

            let app_dirs = match uc_platform::app_dirs::DirsAppDirsAdapter::new().get_app_dirs() {
                Ok(dirs) => dirs,
                Err(err) => {
                    error!("Failed to determine system data directory: {}", err);
                    error!("Please ensure your platform's data directory is accessible");
                    error!("macOS: ~/Library/Application Support/");
                    error!("Linux: ~/.local/share/");
                    error!("Windows: %LOCALAPPDATA%");
                    std::process::exit(1);
                }
            };

            AppConfig::with_system_defaults(app_dirs.app_data_root)
        }
    };

    // Run the application with the loaded config
    run_app(config);
}

/// Run the Tauri application
fn run_app(config: AppConfig) {
    use tauri::Builder;

    // Create event channels for PlatformRuntime
    let (platform_event_tx, platform_event_rx): (PlatformEventSender, PlatformEventReceiver) =
        mpsc::channel(100);
    let (platform_cmd_tx, platform_cmd_rx): (
        tokio::sync::mpsc::Sender<uc_platform::ipc::PlatformCommand>,
        PlatformCommandReceiver,
    ) = mpsc::channel(100);

    // Wire all dependencies using the new bootstrap flow
    let wired = match wire_dependencies(&config, platform_cmd_tx.clone()) {
        Ok(wired) => wired,
        Err(e) => {
            error!("Failed to wire dependencies: {}", e);
            panic!("Dependency wiring failed: {}", e);
        }
    };

    let deps = wired.deps;
    let background = wired.background;
    let watcher_control = wired.watcher_control;

    let pairing_device_repo = deps.device.paired_device_repo.clone();
    let pairing_device_identity = deps.device.device_identity.clone();
    let pairing_settings = deps.settings.clone();
    let discovery_network = deps.network_ports.peers.clone();
    let pairing_peer_id = background.libp2p_network.local_peer_id();
    let pairing_identity_pubkey = background.libp2p_network.local_identity_pubkey();
    let (pairing_device_name, pairing_config) = tauri::async_runtime::block_on(async move {
        let device_name = resolve_pairing_device_name(pairing_settings.clone()).await;
        let config = resolve_pairing_config(pairing_settings).await;
        (device_name, config)
    });
    let pairing_device_id = pairing_device_identity.current_device_id().to_string();
    let staged_store = Arc::new(StagedPairedDeviceStore::new());
    let (pairing_orchestrator, pairing_action_rx) = PairingOrchestrator::new(
        pairing_config,
        pairing_device_repo,
        pairing_device_name,
        pairing_device_id,
        pairing_peer_id,
        pairing_identity_pubkey,
        staged_store.clone(),
    );
    let pairing_orchestrator = Arc::new(pairing_orchestrator);
    let space_access_orchestrator = Arc::new(SpaceAccessOrchestrator::new());
    let key_slot_store: Arc<dyn KeySlotStore> = {
        let app_dirs = match DirsAppDirsAdapter::new().get_app_dirs() {
            Ok(dirs) => dirs,
            Err(err) => {
                error!(error = %err, "Failed to determine app directories for keyslot store");
                panic!(
                    "Failed to determine app directories for keyslot store: {}",
                    err
                );
            }
        };
        let app_data_root = if config.database_path.as_os_str().is_empty() {
            app_dirs.app_data_root.clone()
        } else {
            let configured_db_root = config
                .database_path
                .parent()
                .unwrap_or(&config.database_path)
                .to_path_buf();
            apply_profile_suffix(configured_db_root)
        };

        let vault_dir = resolve_keyslot_store_vault_dir(&config, app_data_root);
        Arc::new(JsonKeySlotStore::new(vault_dir))
    };

    let runtime = AppRuntime::with_setup(
        deps,
        SetupRuntimePorts::from_network(
            pairing_orchestrator.clone(),
            space_access_orchestrator.clone(),
            discovery_network,
        ),
        watcher_control,
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

    let builder = Builder::default()
        // Register AppRuntime for Tauri commands
        .manage(runtime_for_tauri)
        .manage(pairing_orchestrator.clone())
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
        .on_page_load(|webview, payload| {
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
        .plugin(tauri_plugin_opener::init());

    let builder = if disable_single_instance {
        info!("UC_DISABLE_SINGLE_INSTANCE=1 set; skipping single-instance plugin registration");
        builder
    } else {
        builder.plugin(tauri_plugin_single_instance::init(|_app, _args, _cwd| {}))
    };

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
                Some(app.handle().clone()),
                pairing_orchestrator.clone(),
                pairing_action_rx,
                staged_store,
                space_access_orchestrator.clone(),
                key_slot_store.clone(),
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
            // Encryption commands
            uc_tauri::commands::encryption::initialize_encryption,
            uc_tauri::commands::encryption::get_encryption_session_status,
            uc_tauri::commands::encryption::unlock_encryption_session,
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
        ])
        .build(tauri::generate_context!())
        .expect("error building tauri application")
        .run(move |_app_handle, event| {
            if let tauri::RunEvent::ExitRequested { .. } = event {
                info!("App exit requested, cancelling all tracked tasks");
                task_registry.token().cancel();
            }
            if let tauri::RunEvent::Exit = event {
                info!("Application exiting");
            }
        });
}
