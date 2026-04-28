//! daemon 运行循环。

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use tokio::runtime::Runtime;
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

/// 运行 daemon，退出后关闭 space setup 资源。
pub fn run_daemon_until_shutdown(
    runtime: &Runtime,
    input: DaemonRunLoopInput,
) -> anyhow::Result<()> {
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

    runtime.block_on(async move {
        // P1 thin 升级检测：启动期一次性比较 last_seen_version 游标 vs
        // 当前构建版本。结果只打 tracing 日志，不连 UI/CLI；后续 phase
        // 把它接到重新配对引导等具体动作。
        log_upgrade_status(&app_facade).await;

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
    })
}

async fn log_upgrade_status(app_facade: &AppFacade) {
    match app_facade.upgrade.detect_on_startup(DAEMON_VERSION).await {
        Ok(UpgradeStatus::FreshInstall) => {
            tracing::info!(
                target: "upgrade",
                current = DAEMON_VERSION,
                "fresh install detected"
            );
        }
        Ok(UpgradeStatus::NoChange) => {
            tracing::info!(
                target: "upgrade",
                current = DAEMON_VERSION,
                "version cursor matches current build"
            );
        }
        Ok(UpgradeStatus::Upgraded { from, to }) => {
            tracing::info!(
                target: "upgrade",
                from = from.as_ref().map(|v| v.to_string()).as_deref().unwrap_or("<unknown>"),
                to = %to,
                "upgrade detected"
            );
        }
        Ok(UpgradeStatus::Downgraded { from, to }) => {
            tracing::warn!(
                target: "upgrade",
                from = %from,
                to = %to,
                "downgrade detected — rolled back to an older build"
            );
        }
        Err(error) => {
            tracing::warn!(
                target: "upgrade",
                error = %error,
                "detect_on_startup failed; skipping upgrade decision this boot"
            );
        }
    }
}
