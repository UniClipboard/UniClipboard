//! 跨平台 fallback listener:把 snapshot 打到 tracing 日志。
//!
//! 非 macOS / Windows 平台(以及调试 / 自动化场景)用这个。可以通过
//! `RUST_LOG=uc_tauri::transfer_hud=debug` 直接看到行状态机推进,
//! 用来在没有真实 UI 的环境里端到端验证事件管道。

use tracing::debug;

use super::super::emitter::TransferHudListener;
use super::super::state::TransferHudRow;

pub struct TracingTransferHudListener;

impl TransferHudListener for TracingTransferHudListener {
    fn on_changed(&self, snapshot: Vec<TransferHudRow>) {
        debug!(
            row_count = snapshot.len(),
            rows = ?snapshot,
            "transfer_hud snapshot"
        );
    }
}
