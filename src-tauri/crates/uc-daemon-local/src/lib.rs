//! Local daemon runtime metadata and process coordination helpers.
//!
//! ## 模块分层
//!
//! 默认编译路径（**无 GUI 框架依赖**）——任何 desktop / daemon / CLI
//! 进程都可以消费：
//!
//! - [`auth`]：daemon bearer token 文件持久化
//! - [`contract`]：纯 enum / error 契约（`DaemonBootstrapError`、
//!   `ProbeOutcome`、`SpawnReason`、`TerminateDaemonError`、
//!   `terminate_local_daemon_pid`）
//! - [`health_wait`]：probe-only 的健康轮询 helpers
//! - [`process_metadata`]：PID 文件读写
//! - [`socket`]：IPC / HTTP socket 路径解析
//!
//! 仅在 `sidecar-lifecycle` feature 启用时编译（**会拖入 `tauri-plugin-shell`**）：
//!
//! - [`daemon_bootstrap`]：拉起协调（含绑 `CommandChild` 的 spawn hook）
//! - [`daemon_lifecycle`]：`OwnedDaemonChild` / `GuiOwnedDaemonState` 持有
//!   `tauri-plugin-shell::CommandChild`

pub mod auth;
pub mod contract;
pub mod health_wait;
pub mod process_metadata;
pub mod socket;

#[cfg(feature = "sidecar-lifecycle")]
pub mod daemon_bootstrap;
#[cfg(feature = "sidecar-lifecycle")]
pub mod daemon_lifecycle;
