//! daemon runtime 模块树（从 uc-desktop 迁出，ADR-008 P1/P2）。
//!
//! 保留 `daemon/` 路径使迁入文件内的 `crate::daemon::X` / `super::X` 引用
//! 原样可解析。

pub mod clipboard_router;
pub mod engine_events;
pub mod handle;
pub mod host;
pub mod mobile_lan_lifecycle;
pub mod oneshot;
pub mod run_mode;
pub mod space_catalog;
#[cfg(windows)]
pub mod spaces_axum;
#[cfg(target_os = "windows")]
pub mod spaces_http;
pub mod startup_recovery;
pub mod tokio_runtime;

pub use handle::DaemonHandle;
