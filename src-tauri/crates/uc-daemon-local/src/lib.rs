//! Local daemon runtime metadata and process coordination helpers.
//!
//! 全部模块都不依赖任何 GUI 框架——desktop / daemon / CLI 任意一方都可以
//! 直接消费：
//!
//! - [`auth`]：daemon bearer token 文件持久化
//! - [`contract`]：纯 enum / error 契约（`ProbeOutcome`、
//!   `DaemonBootstrapError`、`TerminateDaemonError`、
//!   `terminate_local_daemon_pid`）
//! - [`health_wait`]：probe-only 的健康轮询 helpers
//! - [`process_metadata`]：PID 文件读写
//! - [`socket`]：IPC / HTTP socket 路径解析
//! - [`spawn`]：`uniclipd` 二进制的 detached spawn（CLI 与 GUI shell 共用）

pub mod auth;
pub mod contract;
pub mod health_wait;
pub mod instance_lock;
pub mod process_metadata;
pub mod socket;
pub mod spawn;
pub mod spawn_contract;
