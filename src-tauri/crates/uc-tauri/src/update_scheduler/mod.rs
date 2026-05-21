//! 后台周期更新检查调度器。
//!
//! 模块结构：
//! - `last_notified`: 持久化已通知过的版本（按 channel 去重）
//! - `scheduler`: 主循环 + setup-wait + backoff（Phase 3B）
//! - 后续 Phase 将加入通知发送（4A）、点击 handler（4D）

pub mod last_notified;
pub mod scheduler;

pub use last_notified::LastNotifiedUpdateStore;
pub use scheduler::{run, SchedulerDeps};
