//! 后台周期更新检查调度器。
//!
//! 模块结构：
//! - `last_notified`: 持久化已通知过的版本（按 channel 去重）
//! - `scheduler`: 主循环 + setup-wait + backoff（Phase 3B）
//! - `window`: Sparkle 风格更新窗口（替代了 Phase 4A 的系统通知路径）
//! - `last_check_at`: 距离上次任意 source check 的时间戳（Phase 5B）

pub mod last_check_at;
pub mod last_notified;
pub mod notify_context;
pub mod scheduler;
pub mod window;
pub mod window_show_check;

pub use last_check_at::LastCheckAt;
pub use last_notified::LastNotifiedUpdateStore;
pub use notify_context::NotifyContext;
pub use scheduler::{run, SchedulerDeps};
pub use window::{open_or_focus_updater_window, UPDATER_WINDOW_LABEL};
pub use window_show_check::maybe_trigger_window_show_check;
