//! 后台周期更新检查调度器。
//!
//! 模块结构：
//! - `last_notified`: 持久化已通知过的版本（按 channel 去重）
//! - `scheduler`: 主循环 + setup-wait + backoff（Phase 3B）
//! - `window`: Sparkle 风格更新窗口（替代了 Phase 4A 的系统通知路径）
//! - `last_check_at`: 距离上次任意 source check 的时间戳（Phase 5B）
//! - `wake_source`: 平台原生唤醒源，让后台周期检查在 App Nap / Modern Standby
//!   下也能发车（macOS NSBackgroundActivityScheduler / Windows resume 通知）

pub mod last_check_at;
pub mod last_notified;
pub mod notify_context;
pub mod scheduler;
pub mod wake_source;
pub mod window;

// 平台唤醒源实现：私有，仅供 `wake_source` 分发调用（同父模块的兄弟可见）。
#[cfg(target_os = "macos")]
mod background_activity_macos;
#[cfg(target_os = "windows")]
mod resume_listener_windows;

pub use last_check_at::LastCheckAt;
pub use last_notified::LastNotifiedUpdateStore;
pub use notify_context::NotifyContext;
pub use scheduler::{run, SchedulerDeps};
pub use wake_source::start as start_wake_source;
pub use window::{open_or_focus_updater_window, UPDATER_WINDOW_LABEL};
