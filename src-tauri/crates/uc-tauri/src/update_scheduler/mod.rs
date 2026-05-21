//! 后台周期更新检查调度器。
//!
//! 模块结构：
//! - `last_notified`: 持久化已通知过的版本（按 channel 去重）
//! - 后续 Phase 将加入主循环、通知发送、点击 handler

pub mod last_notified;

pub use last_notified::LastNotifiedUpdateStore;
