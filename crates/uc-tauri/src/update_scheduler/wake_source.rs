//! 平台原生「唤醒更新调度器」的统一入口。
//!
//! `scheduler` 的 `tokio::sleep` 在 app 后台无可见窗口时，会被 macOS App Nap /
//! Windows Modern Standby 挂起，周期检查因此迟迟不发车（历史症状：「只有打开主
//! 窗口才检测/弹更新窗口」）。这里按平台挂上 OS 原生 hook，它们在那种状态下仍会
//! 触发，通过 `wake_tx` 把 `scheduler` 主循环从被挂起的 sleep 里叫醒。无对应 hook
//! 的平台（Linux 等）为 no-op，scheduler 退回纯 tokio cadence。
//!
//! - macOS：[`background_activity_macos`] —— `NSBackgroundActivityScheduler`
//! - Windows：[`resume_listener_windows`] —— 系统 resume 通知回调

use std::time::Duration;

use tauri::AppHandle;
use tokio::sync::mpsc::Sender;

#[cfg(target_os = "macos")]
use super::background_activity_macos;
#[cfg(target_os = "windows")]
use super::resume_listener_windows;

/// 启动平台唤醒源。`interval` 是期望的后台检查周期（macOS 后台活动调度器用它设
/// interval；其它平台忽略）。fire-and-forget，内部失败仅 warn。
pub fn start(app: &AppHandle, wake_tx: Sender<()>, interval: Duration) {
    #[cfg(target_os = "macos")]
    background_activity_macos::start(app, wake_tx, interval);

    #[cfg(target_os = "windows")]
    {
        let _ = (app, interval);
        resume_listener_windows::start(wake_tx);
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        // Linux 等：无后台节流 hook；tokio cadence 正常工作。丢弃 sender——
        // channel 的存活由 run.rs 移进 scheduler task 的 keepalive sender 保证，
        // 所以 `wake_rx.recv()` 不会因此返回 None。
        let _ = (app, interval, wake_tx);
    }
}
