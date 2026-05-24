//! HUD 渲染端按平台分发。
//!
//! 加新平台 (Windows / Linux 自绘) 时:
//! 1. 新增 `windows.rs` / `linux.rs`,实现 [`TransferHudListener`]
//! 2. 在 [`create_listener`] 里加 `#[cfg]` 分支选出对应实现
//!
//! 状态机 / emitter / actions 全部平台无关,不需要动。

use std::sync::Arc;

use tauri::AppHandle;

use super::actions::TransferHudActions;
use super::emitter::TransferHudListener;

#[cfg(target_os = "macos")]
pub mod macos;
pub mod tracing;

/// 按运行平台创建对应的 listener。
///
/// - macOS:返回 [`macos::MacosTransferHudListener`](self::macos::MacosTransferHudListener) —— 真实 AppKit HUD。
/// - 其它平台:返回 [`tracing::TracingTransferHudListener`] —— 仅日志,
///   无 UI;产品语义就是 HUD 暂仅 macOS 支持。
pub fn create_listener(
    #[allow(unused_variables)] app_handle: AppHandle,
    #[allow(unused_variables)] actions: Arc<dyn TransferHudActions>,
) -> Arc<dyn TransferHudListener> {
    #[cfg(target_os = "macos")]
    {
        Arc::new(macos::MacosTransferHudListener::new(app_handle, actions))
    }
    #[cfg(not(target_os = "macos"))]
    {
        Arc::new(tracing::TracingTransferHudListener)
    }
}
