//! Bootstrap module - Application initialization and wiring
//! Bootstrap 模块 - 应用初始化和连接

pub mod clipboard_integration_mode;
pub mod config;
pub mod init;
pub mod logging;
pub mod run;
pub mod runtime;
pub mod task_registry;
pub mod tracing;
pub mod wiring;

// Re-export commonly used bootstrap functions
pub use clipboard_integration_mode::resolve_clipboard_integration_mode;
pub use config::load_config;
pub use init::ensure_default_device_name;
pub use runtime::{create_app, create_runtime, AppRuntime, SetupRuntimePorts, UseCases};
pub use wiring::{
    get_storage_paths, resolve_pairing_config, resolve_pairing_device_name, start_background_tasks,
    wire_dependencies, BackgroundRuntimeDeps, WiredDependencies,
};
