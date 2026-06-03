//! UniClipboard 桌面宿主层（desktop host layer）—— GUI-framework agnostic。
//!
//! 本 crate 负责把 UniClipboard 的 app runtime（`uc-application`）跑在
//! 桌面环境里：接入系统能力、后台任务、HTTP/IPC、daemon 进程协调。
//!
//! 它**不是业务层**，不承载核心业务规则；**也不绑定任何 GUI 框架**，
//! 不依赖 `tauri` / `iced` / `AppKit` 等。具体 GUI shell（Tauri、未来
//! 原生 macOS 等）由各自的 shell crate 提供，consume 本 crate。
//!
//! 业务动作必须通过 `uc-application` 的 facade 进入；setup / pairing /
//! sync / transfer 等决策一律留在应用层与 `uc-core`。

pub const DAEMON_VERSION: &str = env!("CARGO_PKG_VERSION");
pub use uc_daemon_contract::DAEMON_API_REVISION;

pub mod background;
pub mod daemon;
pub mod daemon_probe;
pub mod runtime;
pub mod shortcuts;

pub use daemon::{DaemonHandle, DaemonOwnership};
pub use runtime::DesktopRuntime;
