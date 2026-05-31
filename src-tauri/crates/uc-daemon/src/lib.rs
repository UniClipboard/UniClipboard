//! `uc-daemon`：UniClipboard 的 GUI-agnostic daemon runtime 库。
//!
//! 承载从 `uc-desktop/src/daemon/` 迁出的 daemon runtime 构件（run_mode、
//! 后台服务 / worker、装配链、main loop、startup recovery 等）。
//!
//! **硬约束**：不依赖任何 GUI / UI 框架，也**不依赖 `uc-desktop`**——
//! `uc-desktop` 反过来依赖本 crate（forward dep）。GuiInProcess 专属装配
//! （`start_in_process` / `ProcessRuntimeHandles`）暂留 `uc-desktop`，由 host
//! 胶水前向调用本 crate 的装配函数。
//!
//! `uniclipd` 二进制 target 在后续阶段加入（ADR-008 P2）。
//!
//! 见 `docs/architecture/adr-008-uniclipd-split-gui-as-client.md`
//! 与 `docs/architecture/adr-008-p1-extraction-plan.md`。

pub mod daemon;

pub use daemon::run_mode;
pub use daemon::DaemonHandle;
pub use daemon::DaemonOwnership;
