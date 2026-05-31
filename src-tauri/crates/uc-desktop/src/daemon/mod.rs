//! daemon host 胶水。
//!
//! GUI-agnostic runtime 主体已迁出至 `uc-daemon`（ADR-008 P1）；本模块仅保留
//! host 胶水（`host`：GuiInProcess 装配 `start_in_process` / `ProcessRuntimeHandles`，
//! 以及独立入口 `run` / `run_standalone_from_env`），并 re-export `uc-daemon`
//! 的公共符号以保持 `uc_desktop::daemon::*` 接口面不变。
//!
//! `host` 内部已直接 `use uc_daemon::daemon::*` 调用 runtime 装配函数（前向依赖）。

pub(crate) mod host;

// ── re-exports from uc-daemon（保 uc_desktop::daemon::* 公共面）──
pub use uc_daemon::daemon::run_mode;
pub use uc_daemon::{DaemonHandle, DaemonOwnership};

pub(crate) use host::start_in_process;
pub use host::{
    run, run_standalone_from_env, ProcessRuntimeHandles, RUN_MODE_ENV, RUN_MODE_SERVER,
};
