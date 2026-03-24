//! Bootstrap module - Application initialization and wiring
//! Bootstrap 模块 - 应用初始化和连接

pub mod assembly;
pub mod clipboard_integration_mode;
pub mod config;
pub mod config_resolution;
pub mod daemon_lifecycle;
pub mod file_transfer_wiring;
pub mod init;
pub mod logging;
pub mod run;
pub mod runtime;
pub mod setup_pairing_bridge;
pub mod task_registry;
pub mod tracing;
pub mod wiring;

// Re-export commonly used bootstrap functions
pub use assembly::SetupAssemblyPorts;
pub use clipboard_integration_mode::resolve_clipboard_integration_mode;
pub use config::load_config;
pub use config_resolution::{resolve_app_config, resolve_config_path, ConfigResolutionError};
pub use daemon_lifecycle::{GuiOwnedDaemonState, OwnedDaemonChild, SpawnReason};
pub use init::ensure_default_device_name;
pub use run::{
    bootstrap_daemon_connection, emit_daemon_connection_info_if_ready, supervise_daemon,
};
pub use runtime::{create_app, create_runtime, AppRuntime, AppUseCases};
pub use setup_pairing_bridge::{build_setup_pairing_facade, DaemonBackedSetupPairingFacade};
pub use uc_app::usecases::setup::SetupPairingFacadePort;
// assembly.rs re-exports (pure dependency construction — zero tauri imports)
pub use assembly::{
    build_setup_orchestrator, get_storage_paths, resolve_pairing_config,
    resolve_pairing_device_name, wire_dependencies, wire_dependencies_with_identity_store,
    WiredDependencies,
};
// wiring.rs re-exports (Tauri event loops and background task management)
pub use wiring::{start_background_tasks, BackgroundRuntimeDeps};
