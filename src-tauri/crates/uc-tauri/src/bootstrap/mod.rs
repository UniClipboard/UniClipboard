//! Tauri shell 的 bootstrap 入口。
//!
//! GUI-framework agnostic 的装配工具住在 `uc-bootstrap`，桌面后台任务调度
//! 与 daemon 进程协调住在 `uc-desktop`，所以这里只剩 Tauri 特有的 logging
//! 与 runtime 包装两件事。

pub mod logging;
pub mod runtime;

pub use runtime::TauriAppRuntime;

// Re-export composition-root assembly types from uc-bootstrap.
pub use uc_bootstrap::assembly::{
    get_storage_paths, resolve_pairing_device_name, wire_dependencies, WiredDependencies,
    WiringError, WiringResult,
};
pub use uc_bootstrap::ensure_default_device_name;
pub use uc_bootstrap::load_config;
pub use uc_bootstrap::BackgroundRuntimeDeps;

// Re-export desktop background task starters under their historical
// `uc_tauri::bootstrap::start_*` import paths. New code should target
// `uc_desktop::background::*` directly.
pub use uc_desktop::background::start_file_cache_cleanup as start_background_tasks;
