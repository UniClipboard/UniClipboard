//! daemon 启动恢复任务。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tokio::sync::Notify;
use tracing::{info_span, Instrument};
use uc_application::facade::{AppFacade, SpaceSetupFacade};
use uc_core::ports::SettingsPort;

use super::run_mode::DaemonRunMode;

/// 启动恢复任务所需输入。
pub struct StartupRecoveryInput {
    pub run_mode: DaemonRunMode,
    pub app_facade: Arc<AppFacade>,
    pub settings: Arc<dyn SettingsPort>,
    pub space_setup: Arc<SpaceSetupFacade>,
    pub deferred_ready_notify: Arc<Notify>,
    pub clipboard_capture_gate: Arc<AtomicBool>,
}

/// 在后台恢复加密会话、空间会话和 presence。
///
/// 恢复动作可能触发系统钥匙串，启动阶段不能同步等待它完成；daemon 先把
/// HTTP 监听拉起来，再让这个后台任务慢慢恢复。
pub fn spawn_startup_recovery(input: StartupRecoveryInput) {
    tokio::spawn(async move {
        // Standalone daemon 在这里强制走 `true` 分支——CLI 拉起的独立 daemon
        // 没有 GUI 通道接收手动解锁，启动期不解锁就等于让剪贴板/同步服务
        // 永久卡在 deferred 队列里。详见
        // [`DaemonRunMode::uses_auto_unlock_setting`] 的注释。
        let auto_unlock_enabled = if input.run_mode.uses_auto_unlock_setting() {
            let settings = input.settings.load().await.unwrap_or_default();
            settings.security.auto_unlock_enabled
        } else {
            true
        };

        let unlocked = match crate::daemon::app::recover_encryption_session(
            &input.app_facade,
            auto_unlock_enabled,
        )
        .instrument(info_span!("daemon.startup.recover_encryption_session"))
        .await
        {
            Ok(unlocked) => unlocked,
            Err(error) => {
                tracing::warn!(
                    error = %error,
                    "background unlock: recover_encryption_session failed"
                );
                false
            }
        };

        // Gate: 仅当 KEK 已经被 `recover_encryption_session` 装进内存
        // (即 auto_unlock_enabled=true 且解锁成功) 时，才进入 facade-level
        // resume。否则 `space_setup.try_resume_session` 会下沉到
        // `space_access.try_resume_session` → `load_kek` → 触发 macOS
        // keychain 访问弹窗——而用户场景是"没启用 auto unlock、没点
        // unlock 按钮"，按规则 #1/#2/#3 此刻不该接触 keychain。
        //
        // 副作用：auto_unlock_enabled=false 时，启动期不再自动推进
        // switch-space migration recovery；但没有 KEK 也推不动，等用户
        // 点 unlock / 显式 auto-unlock 后再做也不迟。
        if unlocked {
            match input.space_setup.try_resume_session().await {
                Ok(true) => {
                    if let Err(error) = input.space_setup.refresh_presence().await {
                        tracing::warn!(error = %error, "background unlock: presence probe failed");
                    }
                }
                Ok(false) => {
                    tracing::info!(
                        "background unlock: no space on this profile — skipping resume/probe"
                    );
                }
                Err(error) => {
                    tracing::warn!(error = ?error, "background unlock: try_resume_session failed");
                }
            }
        } else {
            tracing::info!(
                "background unlock: encryption session not unlocked — skipping space_setup resume to avoid keychain prompt"
            );
        }

        if input.run_mode.auto_triggers_deferred_services() && unlocked {
            input.clipboard_capture_gate.store(true, Ordering::SeqCst);
            input.deferred_ready_notify.notify_one();
            tracing::info!("background unlock: persistent mode auto-triggered deferred services");
        }
    });
}
