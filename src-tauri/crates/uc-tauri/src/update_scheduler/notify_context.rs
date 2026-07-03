//! 共享的"找到新版本 → 去重 → 弹更新窗口 → 持久化已通知版本"上下文。
//!
//! Scheduler 主循环和托盘手动检查都会走这条路径，所以把所需
//! 依赖打包成一个 struct 挂到 Tauri app state；多条路径都通过
//! `app.state::<Arc<NotifyContext>>()` 拿同一份，确保去重 store 的
//! `Mutex` 是同一把、落盘路径是同一个、analytics 出口也一致。

use std::{path::PathBuf, sync::Arc};

use tauri::AppHandle;
use tokio::sync::Mutex;
use tracing::{debug, warn};
use uc_core::settings::model::UpdateChannel;
use uc_observability::analytics::{
    AnalyticsPort, Event, InstallKind as AnalyticsInstallKind, NotificationDeliveryStatus,
};

use super::last_notified::LastNotifiedUpdateStore;
use super::prompt_throttle::PromptThrottleStore;
use super::skipped_version::SkippedVersionStore;
use super::window::open_or_focus_updater_window;

/// Who asked for the notification. Scheduled checks respect the prompt
/// cooldown; manual checks (tray menu) bypass it because the user explicitly
/// asked to check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotifyTrigger {
    Scheduled,
    Manual,
}

/// 所有走"通知 + 弹窗 + 持久化"路径的调用方共享的依赖集合。
pub struct NotifyContext {
    pub app_handle: AppHandle,
    pub analytics: Arc<dyn AnalyticsPort>,
    /// 已通知版本 store。两个调用方共享同一个 `Arc<Mutex<_>>`，避免
    /// 双源去重相互覆盖。
    pub last_notified: Arc<Mutex<LastNotifiedUpdateStore>>,
    /// `last_notified` 持久化目标路径。
    pub last_notified_path: PathBuf,
    /// 用户主动跳过的版本 store。
    pub skipped_version: Arc<Mutex<SkippedVersionStore>>,
    /// `skipped_version` 持久化目标路径。
    pub skipped_version_path: PathBuf,
    /// Cooldown store: when was the updater window last opened. Gates
    /// scheduler-triggered prompts so a fast release cadence doesn't turn
    /// into a popup per release.
    pub prompt_throttle: Arc<Mutex<PromptThrottleStore>>,
    /// `prompt_throttle` 持久化目标路径。
    pub prompt_throttle_path: PathBuf,
}

impl NotifyContext {
    /// Available 分支：若 (channel, version) 未通知过且未被用户跳过，弹出
    /// Sparkle 风格更新窗口，emit `update_notification_shown`，仅在窗口
    /// 成功创建后 `record` 持久化。
    ///
    /// 返回 `true` 表示这次确实打开（或聚焦了）窗口，`false` 表示被去重
    /// store / skipped store / prompt cooldown（仅 `Scheduled` 触发，emit
    /// `update_prompt_suppressed`）short-circuit 或 builder 失败。Scheduler
    /// 用这个布尔值判断是否需要在 auto-download Ready 阶段兜底再开一次窗口。
    ///
    /// `delivery_status` 字段语义：`Sent` 表示窗口已打开，`SendFailed`
    /// 表示 `WebviewWindowBuilder::build` 失败（OS 资源耗尽 / 平台异常）。
    pub async fn notify_if_new_version(
        &self,
        channel: &UpdateChannel,
        version: &str,
        install_kind: AnalyticsInstallKind,
        trigger: NotifyTrigger,
    ) -> bool {
        let is_skipped = {
            let store = self.skipped_version.lock().await;
            store.is_skipped(channel, version)
        };
        if is_skipped {
            debug!(
                target: "update_scheduler",
                channel = ?channel,
                version,
                "version skipped by user; not showing updater window"
            );
            return false;
        }

        let already_notified = {
            let store = self.last_notified.lock().await;
            store.contains(channel, version)
        };
        if already_notified {
            debug!(
                target: "update_scheduler",
                channel = ?channel,
                version,
                "version already notified; skipping updater window open"
            );
            return false;
        }

        // Cooldown gate (scheduled only). Deliberately checked AFTER the
        // per-version dedup and WITHOUT recording into `last_notified`:
        // versions found during the cooldown stay unrecorded, so the first
        // prompt after expiry shows the newest version.
        if trigger == NotifyTrigger::Scheduled {
            let throttled = !self.prompt_throttle.lock().await.allows_prompt(channel);
            if throttled {
                debug!(
                    target: "update_scheduler",
                    channel = ?channel,
                    version,
                    "prompt cooldown active; not showing updater window"
                );
                // Sole emit site for this event: the ready-fallback path can
                // only be reached in the same iteration after this branch
                // already returned false, so it stays silent to keep at most
                // one suppression event per check.
                self.analytics.capture(Event::UpdatePromptSuppressed {
                    version: version.to_string(),
                    install_kind,
                });
                return false;
            }
        }

        let delivery = match open_or_focus_updater_window(&self.app_handle, false) {
            Ok(()) => NotificationDeliveryStatus::Sent,
            Err(err) => {
                warn!(
                    target: "update_scheduler",
                    error = %err,
                    "failed to open updater window"
                );
                NotificationDeliveryStatus::SendFailed
            }
        };
        self.analytics.capture(Event::UpdateNotificationShown {
            version: version.to_string(),
            delivery_status: delivery,
            install_kind,
        });

        let opened = matches!(delivery, NotificationDeliveryStatus::Sent);
        if opened {
            let mut store = self.last_notified.lock().await;
            if let Err(err) = store
                .record(
                    channel.clone(),
                    version.to_string(),
                    &self.last_notified_path,
                )
                .await
            {
                warn!(
                    target: "update_scheduler",
                    error = %err,
                    "failed to persist last_notified_update.json"
                );
            }
            drop(store);
            self.record_prompt_shown().await;
        }
        opened
    }

    /// Cooldown-gated fallback used by the scheduler when auto-download
    /// reached `Ready` but the per-version dedup kept the window closed for
    /// this process (a previous process already notified this version).
    ///
    /// Returns `true` when the window was actually opened.
    pub async fn open_ready_fallback_window(&self, channel: &UpdateChannel) -> bool {
        {
            let throttle = self.prompt_throttle.lock().await;
            if !throttle.allows_prompt(channel) {
                debug!(
                    target: "update_scheduler",
                    channel = ?channel,
                    "prompt cooldown active; skipping ready-fallback window"
                );
                return false;
            }
        }
        match open_or_focus_updater_window(&self.app_handle, false) {
            Ok(()) => {
                self.record_prompt_shown().await;
                true
            }
            Err(err) => {
                warn!(
                    target: "update_scheduler",
                    error = %err,
                    "failed to open updater window after auto-download (ready fallback)"
                );
                false
            }
        }
    }

    /// Refresh the cooldown timestamp after any successful window open.
    async fn record_prompt_shown(&self) {
        let mut throttle = self.prompt_throttle.lock().await;
        if let Err(err) = throttle.record_prompt_now(&self.prompt_throttle_path).await {
            warn!(
                target: "update_scheduler",
                error = %err,
                "failed to persist update_prompt_throttle.json"
            );
        }
    }
}
