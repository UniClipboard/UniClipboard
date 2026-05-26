//! "用户点开主窗口时顺手补一次检查" 的触发器。Phase 5B / Q10。
//!
//! ## 为什么需要
//!
//! macOS App Nap / Windows Modern Standby / 笔记本休眠等场景会让 tokio sleep
//! 停摆 —— scheduler 在用户没操作时可能 6h 没醒一次。用户主动点开主窗口
//! 是"系统/用户都活跃"的信号，趁此机会补一次 check。
//!
//! 阈值 30min（Q10.1）：太短打扰太频繁，太长又跟不上 release CDN 更新。
//!
//! ## 范围
//!
//! 本 helper 做 **检查 + 弹窗 + telemetry**，但不自动下载：
//! - 检测到新版本会调 [`notify_if_new_version`] 走 Sparkle 风格更新窗口
//!   （与 scheduler 主循环共用同一段去重逻辑，`last_notified_update.json`
//!   保证同一 (channel, version) 只弹一次）
//! - 自动下载仍是 scheduler 的职责，下一次 scheduler tick（≤ 6h）会接管
//!
//! ## 调用约束
//!
//! - **必须 sync 入口**：`tray::show_main_window` 是 sync 函数，6 个 caller
//!   都在事件 handler / IPC 同步路径里
//! - 内部 `tauri::async_runtime::spawn` 跑实际 check —— fire-and-forget
//! - 阈值 / 开关任何一个未通过都直接 return；spawn 失败兜底 warn 不向上传
//!
//! ## 调用点
//!
//! `tray::show_main_window` 顶部一次性触发，覆盖全部 6 个 caller
//! （tray menu open/settings、tray icon click、startup silent_start=false、
//! startup barrier、macOS dock reopen）。

use std::sync::Arc;

use tauri::{AppHandle, Manager};
use tracing::{debug, info, warn};
use uc_observability::analytics::{Event, UpdateCheckOutcome, UpdateCheckSource};

use super::last_check_at::LastCheckAt;
use super::notify_context::NotifyContext;
use super::scheduler::resolve_channel;
use crate::bootstrap::TauriAppRuntime;
use crate::commands::updater::{
    classify_check_failure, detect_install_kind, do_check_for_update, install_kind_for_telemetry,
    PendingUpdate,
};

/// 距离上次任意 source 的 check 至少这么久才触发顺手检查（Q10.1）。
pub(crate) const WINDOW_SHOW_CHECK_THRESHOLD_SECS: i64 = 30 * 60;

/// 距离上次 check 是否已经过了阈值，且 `auto_check_update == true`。
///
/// 提成纯函数便于单测。settings 部分由调用方读，避免本函数 await
/// settings_port（保留同步入口可在不需要时立刻 return）。
pub(crate) fn should_trigger(seconds_since_last_check: i64, auto_check_update: bool) -> bool {
    auto_check_update && seconds_since_last_check >= WINDOW_SHOW_CHECK_THRESHOLD_SECS
}

/// 主入口：用户打开主窗口时调用一次。
///
/// Sync / fire-and-forget。
/// 1. 读 `LastCheckAt`，距离 < 30min → 立即 return
/// 2. spawn 异步任务读 settings；`auto_check_update == false` → return
/// 3. 真正调 `do_check_for_update` → 写 `LastCheckAt` → emit
///    `update_check_performed { source: window_show }`
///
/// 不发通知 / 不自动下载（理由见模块 docstring）。
pub fn maybe_trigger_window_show_check(app: &AppHandle) {
    let seconds_since = match app.try_state::<LastCheckAt>() {
        Some(state) => state.seconds_since(),
        None => {
            debug!(
                target: "update_scheduler",
                "LastCheckAt state not mounted; skipping window_show check"
            );
            return;
        }
    };

    // 早返：距离不够、不需要打扰 tokio runtime。
    if seconds_since < WINDOW_SHOW_CHECK_THRESHOLD_SECS {
        debug!(
            target: "update_scheduler",
            seconds_since,
            "skipping window_show check; below threshold"
        );
        return;
    }

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        run_window_show_check(app).await;
    });
}

async fn run_window_show_check(app: AppHandle) {
    let runtime = match app.try_state::<std::sync::Arc<TauriAppRuntime>>() {
        Some(r) => r,
        None => {
            warn!(
                target: "update_scheduler",
                "TauriAppRuntime not mounted; aborting window_show check"
            );
            return;
        }
    };

    let settings = match runtime.settings_port().load().await {
        Ok(s) => s,
        Err(err) => {
            warn!(
                target: "update_scheduler",
                error = %err,
                "failed to load settings; skipping window_show check"
            );
            return;
        }
    };

    if !should_trigger(
        app.state::<LastCheckAt>().seconds_since(),
        settings.general.auto_check_update,
    ) {
        // 二次确认：spawn 落地这一刻别的 source 可能已经触发过 check 并刷新了
        // LastCheckAt（如刚好遇到 scheduler tick）。同样地，用户可能在 spawn
        // 后秒关 auto_check_update。这两种情况都直接 return，不 emit。
        debug!(
            target: "update_scheduler",
            "window_show check superseded by concurrent activity or disabled by settings"
        );
        return;
    }

    let analytics = runtime.analytics();
    let pending = app.state::<PendingUpdate>();
    // 与 scheduler 主循环一致：用户在 settings 里指定的 channel 优先，
    // 否则按 app_version 走 detect_channel 兜底。两条路径共用同一份
    // resolve_channel 实现，避免去重 key 不一致导致弹窗去重失效。
    let app_version = app.package_info().version.to_string();
    let resolved_channel = resolve_channel(settings.general.update_channel.clone(), &app_version);
    info!(target: "update_scheduler", "running window_show check");
    let result = do_check_for_update(&app, Some(resolved_channel.clone()), pending.inner()).await;

    app.state::<LastCheckAt>().record_now();

    let install_kind = install_kind_for_telemetry(detect_install_kind());

    // 找到新版本时也弹更新窗口；走与 scheduler 共享的 NotifyContext，
    // 同一个 (channel, version) 只弹一次，用户每 30min 重开主窗口
    // 不会被骚扰。NotifyContext 由 run.rs setup 阶段 mount 到 app state；
    // 未挂载视为前置 wiring 缺失，warn 后回退到"只发 telemetry"老行为。
    if let Ok(Some(metadata)) = &result {
        match app.try_state::<Arc<NotifyContext>>() {
            Some(ctx) => {
                ctx.notify_if_new_version(&resolved_channel, &metadata.version, install_kind)
                    .await;
            }
            None => warn!(
                target: "update_scheduler",
                "NotifyContext not mounted; window_show check found update but cannot dedup/notify"
            ),
        }
    }

    let (outcome, failure_kind) = match &result {
        Ok(Some(_)) => (UpdateCheckOutcome::Available, None),
        Ok(None) => (UpdateCheckOutcome::UpToDate, None),
        Err(err) => (
            UpdateCheckOutcome::Failed,
            Some(classify_check_failure(err)),
        ),
    };
    analytics.capture(Event::UpdateCheckPerformed {
        source: UpdateCheckSource::WindowShow,
        outcome,
        failure_kind,
        install_kind,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn below_threshold_does_not_trigger() {
        assert!(!should_trigger(0, true));
        assert!(!should_trigger(29 * 60, true));
        assert!(!should_trigger(WINDOW_SHOW_CHECK_THRESHOLD_SECS - 1, true));
    }

    #[test]
    fn at_or_above_threshold_with_setting_enabled_triggers() {
        assert!(should_trigger(WINDOW_SHOW_CHECK_THRESHOLD_SECS, true));
        assert!(should_trigger(WINDOW_SHOW_CHECK_THRESHOLD_SECS + 1, true));
        assert!(should_trigger(6 * 60 * 60, true));
    }

    #[test]
    fn setting_disabled_blocks_even_when_threshold_passed() {
        assert!(!should_trigger(WINDOW_SHOW_CHECK_THRESHOLD_SECS, false));
        assert!(!should_trigger(24 * 60 * 60, false));
    }

    #[test]
    fn threshold_is_30_minutes() {
        // Q10.1 锁死 30min；改阈值需同步更新 schema doc + grill 决策。
        assert_eq!(WINDOW_SHOW_CHECK_THRESHOLD_SECS, 30 * 60);
    }

    #[test]
    fn negative_seconds_since_treated_as_recent() {
        // LastCheckAt::seconds_since saturating_sub clamps to 0; this exercises
        // the calling-convention assumption that we never see negative values.
        // 这里仅做防御性断言：未来谁改 saturating 语义都会让阈值判断回归"刚
        // 检查过"路径。
        assert!(!should_trigger(0, true));
    }
}
