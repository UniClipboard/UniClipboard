//! Bootstrap module - Application initialization and wiring
//! Bootstrap 模块 - 应用初始化和连接

pub mod logging;
pub mod run;
pub mod runtime;
pub mod wiring;

// Re-export commonly used bootstrap functions
pub use run::{bootstrap_daemon_connection, supervise_daemon};
pub use runtime::TauriAppRuntime;
pub use runtime::{DaemonBootstrapOwnershipSnapshot, DaemonBootstrapOwnershipState};
pub use uc_bootstrap::ensure_default_device_name;
pub use uc_bootstrap::load_config;
// uc_bootstrap re-exports (pure dependency construction — zero tauri imports)
pub use uc_bootstrap::assembly::{
    get_storage_paths, resolve_pairing_device_name, wire_dependencies, WiredDependencies,
};
// wiring.rs re-exports (Tauri event loops and background task management)
pub use wiring::{start_background_tasks, start_gui_pairing_lease_task, BackgroundRuntimeDeps};
