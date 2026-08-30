//! Tauri shell 的 bootstrap 入口。
//!
//! GUI 所需的轻量组装（file-backed ports only）住在 `uc-desktop::gui_wiring`，
//! 桌面后台任务调度与 daemon 进程协调住在 `uc-desktop`，所以这里只保留
//! Tauri 特有的 runtime 包装。

pub mod runtime;

pub use runtime::TauriAppRuntime;
