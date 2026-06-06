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
pub mod crash_marker;
pub mod health_wait;
pub mod instance_lock;

// ADR-008 P5-0: the process-management primitives (`process_metadata`,
// `socket`, `spawn`, `spawn_contract`) were extracted into the thin
// `uc-daemon-process` crate so the daemon-client stack can use them without
// pulling in uc-application → uc-infra → iroh/diesel. Re-export them here under
// their original module paths so every existing `uc_daemon_local::<module>::*`
// consumer (uc-daemon, uc-desktop, uc-webserver, …) keeps compiling unchanged.
pub use uc_daemon_process::{handover, process_metadata, socket, spawn, spawn_contract};
