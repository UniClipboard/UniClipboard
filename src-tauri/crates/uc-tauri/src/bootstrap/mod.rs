//! Tauri shell 的 bootstrap 入口。
//!
//! 历史上这里再分一个 `wiring` 子模块，但 GUI-framework agnostic 的装配
//! 工具早已下沉到 `uc-bootstrap`，桌面后台任务调度已下沉到 `uc-desktop`，
//! 留在这里的只剩 Tauri 特有的 daemon sidecar 拉起 / 监督 / runtime 包装。

pub mod logging;
pub mod run;
pub mod runtime;

// Re-export Tauri shell-side daemon sidecar orchestration.
pub use run::{bootstrap_daemon_connection, supervise_daemon};
pub use runtime::TauriAppRuntime;
pub use runtime::{DaemonBootstrapOwnershipSnapshot, DaemonBootstrapOwnershipState};

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
pub use uc_desktop::background::start_gui_pairing_lease as start_gui_pairing_lease_task;
