//! 跨平台 fallback listener:把 snapshot 打到 tracing 日志。
//!
//! 非 macOS / Windows 平台(以及调试 / 自动化场景)用这个。可以通过
//! `RUST_LOG=uc_tauri::activity_hud=debug` 直接看到行状态机推进,
//! 用来在没有真实 UI 的环境里端到端验证事件管道。

use tracing::debug;

use super::super::emitter::ActivityHudListener;
use super::super::state::ActivityHudRow;

pub struct TracingActivityHudListener;

impl ActivityHudListener for TracingActivityHudListener {
    fn on_changed(&self, snapshot: Vec<ActivityHudRow>) {
        debug!(
            row_count = snapshot.len(),
            rows = ?snapshot,
            "activity_hud snapshot"
        );
    }
}
