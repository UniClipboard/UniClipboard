//! daemon 运行循环。

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use tokio::sync::Notify;
use uc_application::facade::{AppFacade, UpgradeStatus};
use uc_bootstrap::SpaceSetupAssembly;
use uc_core::ports::SettingsPort;

use crate::daemon::app::DaemonApp;
use crate::daemon::run_mode::DaemonRunMode;
use crate::daemon::startup_recovery::{spawn_startup_recovery, StartupRecoveryInput};
use crate::DAEMON_VERSION;

/// daemon 运行循环输入。
pub struct DaemonRunLoopInput {
    pub run_mode: DaemonRunMode,
    pub daemon: DaemonApp,
    pub app_facade: Arc<AppFacade>,
    pub settings: Arc<dyn SettingsPort>,
    pub space_setup_assembly: SpaceSetupAssembly,
    pub deferred_ready_notify: Arc<Notify>,
    pub clipboard_capture_gate: Arc<AtomicBool>,
}

/// 运行 daemon main loop，退出后关闭 space setup 资源。
///
/// async 形态：caller 必须已经在 tokio runtime 上下文中。独立 daemon binary
/// 入口在自己的 `Runtime::block_on` 里调用；in-process 入口
/// （[`crate::daemon::start_in_process`]）通过 `tokio::spawn` 把它跑成 task，
/// 由 [`crate::daemon::DaemonHandle`] 持有 join handle。
pub async fn run_daemon_main(input: DaemonRunLoopInput) -> anyhow::Result<()> {
    let DaemonRunLoopInput {
        run_mode,
        daemon,
        app_facade,
        settings,
        space_setup_assembly,
        deferred_ready_notify,
        clipboard_capture_gate,
    } = input;

    let space_setup = space_setup_assembly.facade.clone();

    // P1 thin 升级检测：启动期一次性比较 last_seen_version 游标 vs
    // 当前构建版本。结果一方面进 tracing 日志；另一方面，对全新
    // 安装会就地把游标推进到当前版本，避免后续 UI 把"已完成 setup
    // 的全新安装"误判成"老用户跨配对协议升级"而弹出重新配对提示。
    record_upgrade_status_at_startup(&app_facade).await;

    spawn_startup_recovery(StartupRecoveryInput {
        run_mode,
        app_facade,
        settings,
        space_setup,
        deferred_ready_notify,
        clipboard_capture_gate,
    });

    let result = daemon.run().await;
    space_setup_assembly.shutdown().await;
    result
}

async fn record_upgrade_status_at_startup(app_facade: &AppFacade) {
    let status = match app_facade.upgrade.detect_on_startup(DAEMON_VERSION).await {
        Ok(status) => status,
        Err(error) => {
            tracing::warn!(
                target: "upgrade",
                error = %error,
                "detect_on_startup failed; skipping upgrade decision this boot"
            );
            return;
        }
    };

    match &status {
        UpgradeStatus::FreshInstall => {
            tracing::info!(
                target: "upgrade",
                current = DAEMON_VERSION,
                "fresh install detected"
            );
            // 全新安装时立即把游标推进到当前版本：否则等用户走完 setup，
            // `has_completed` 会翻成 true，但 cursor 仍是 None，下一次
            // detect 就会落入 `Upgraded { from: None }` 这条"没有游标但
            // setup 已完成 = 老用户跨边界升级"的兜底分支，导致前端在首次
            // 安装的设备上错误地弹出"请重新配对设备"提示。
            // 失败仅警告，不阻塞 daemon 启动；下次启动还会重试。
            if let Err(error) = app_facade.upgrade.acknowledge(DAEMON_VERSION).await {
                tracing::warn!(
                    target: "upgrade",
                    error = %error,
                    current = DAEMON_VERSION,
                    "fresh install detected but failed to seal version cursor; \
                     re-pair notice may surface on this device until next boot"
                );
            }
        }
        UpgradeStatus::NoChange => {
            tracing::info!(
                target: "upgrade",
                current = DAEMON_VERSION,
                "version cursor matches current build"
            );
        }
        UpgradeStatus::Upgraded { from, to } => {
            tracing::info!(
                target: "upgrade",
                from = from.as_ref().map(|v| v.to_string()).as_deref().unwrap_or("<unknown>"),
                to = %to,
                "upgrade detected"
            );
        }
        UpgradeStatus::Downgraded { from, to } => {
            tracing::warn!(
                target: "upgrade",
                from = %from,
                to = %to,
                "downgrade detected — rolled back to an older build"
            );
        }
    }
}
