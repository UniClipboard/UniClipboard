//! macOS 原生文件接收 HUD。
//!
//! ## 目的
//!
//! 当从其他设备 (Windows / Linux 对端) 接收文件到本机时,弹出一个独立
//! 的 macOS 原生窗口显示进度,类似 AirDrop 的接收浮窗。这一层独立于
//! 主 webview 中已有的 `TransferProgressBar`,两者并存:主窗口里仍有
//! 详细列表与历史,HUD 提供随手可见的进度 + 取消入口。
//!
//! ## 模块分层
//!
//! - [`clock`]:单调时钟抽象,生产代码用 `Instant::now()`,单测用手动
//!   推进时钟。
//! - [`state`]:纯逻辑状态机,接收 host event,输出行快照。无 AppKit、
//!   无 host event 类型依赖以外的副作用,可完整单元测试。
//! - [`emitter`]:`HostEventEmitterPort` 适配器,把 host event bus 上的
//!   事件喂给状态机,并通过 [`emitter::TransferHudListener`] 通知 UI。
//! - `macos`:AppKit panel 渲染 (Slice 2/3 加入)。
//!
//! ## 非 macOS 平台
//!
//! `clock` / `state` / `emitter` 子模块平台无关,仍编译并可单测。
//! AppKit listener 仅在 `target_os = "macos"` 下编译;其他平台用占位
//! [`emitter::TracingTransferHudListener`] 不会有功能性 HUD,但 emitter
//! 本身仍可挂在 bus 上不报错。

pub mod clock;
pub mod emitter;
pub mod state;

#[cfg(target_os = "macos")]
pub mod macos;

pub use clock::{Clock, SystemClock};
pub use emitter::{TracingTransferHudListener, TransferHudEmitter, TransferHudListener};
pub use state::{RowState, TransferHudRow, TransferHudState};

#[cfg(target_os = "macos")]
pub use macos::MacosTransferHudListener;
