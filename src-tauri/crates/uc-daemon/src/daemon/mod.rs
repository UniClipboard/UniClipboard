//! daemon runtime 模块树（从 uc-desktop 迁出，ADR-008 P1/P2）。
//!
//! 保留 `daemon/` 路径使迁入文件内的 `crate::daemon::X` / `super::X` 引用
//! 原样可解析。

pub mod app;
pub mod app_assembly;
pub mod app_facade_assembly;
pub mod bootstrap;
pub mod handle;
pub mod host;
pub mod mobile_lan_lifecycle;
pub mod peers;
pub mod process_bootstrap;
pub mod process_runtime;
pub mod run_loop;
pub mod run_mode;
pub mod runtime_assembly;
pub mod runtime_controls;
pub mod search;
pub mod search_assembly;
pub mod service;
pub mod service_assembly;
pub mod service_plan;
pub mod startup_recovery;
pub mod state;
pub mod tokio_runtime;
pub mod workers;

pub use handle::DaemonHandle;
