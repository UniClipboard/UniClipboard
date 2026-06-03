//! daemon 启动恢复任务。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tokio::sync::Notify;
use tracing::{info_span, Instrument};
use uc_application::facade::{AppFacade, SpaceSetupFacade};
use uc_core::ports::SettingsPort;
use uc_daemon_local::process_metadata::DaemonSpawnOrigin;
use uc_daemon_local::spawn_contract::unattended_from_env;

use super::run_mode::DaemonRunMode;

/// ADR-008 D9: whether this daemon is *attended* — i.e. respects the user's
/// `auto_unlock_enabled` setting because a GUI will connect and drive the
/// unlock. Attended iff the daemon was GUI-spawned, is not a headless node, and
/// was not launched strict-unattended. Every other launch force-unlocks.
fn is_attended(
    run_mode: DaemonRunMode,
    spawn_origin: DaemonSpawnOrigin,
    strict_unattended: bool,
) -> bool {
    matches!(spawn_origin, DaemonSpawnOrigin::Gui)
        && !matches!(run_mode, DaemonRunMode::ServerHeadless)
        && !strict_unattended
}

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
        // ADR-008 D9 (P4-2): only an *attended* daemon respects the user's
        // `auto_unlock_enabled` setting. Attended = this daemon was spawned by
        // a GUI (which will connect and drive unlock + `POST /lifecycle/ready`),
        // is not a headless node, and was not launched strict-unattended. For
        // an attended daemon with `auto_unlock_enabled = false` we stay locked
        // and let the GUI unlock — the deferred services are released by the
        // GUI's lifecycle/ready once the session becomes ready.
        //
        // Every other launch — interactive `uniclip start`, strict-unattended
        // autostart, headless, or a manually-run `uniclipd` — has no GUI
        // fallback, so it force-unlocks via keyring (the historical behavior):
        // without an unlock the clipboard/sync workers stay stuck in the
        // deferred queue and the daemon looks alive while doing nothing.
        let attended = is_attended(
            input.run_mode,
            DaemonSpawnOrigin::from_env(),
            unattended_from_env(),
        );

        let auto_unlock_enabled = if attended {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_gui_spawned_standalone_is_attended() {
        // The one attended case: GUI-spawned, not headless, not strict.
        assert!(is_attended(
            DaemonRunMode::Standalone,
            DaemonSpawnOrigin::Gui,
            false
        ));
    }

    #[test]
    fn cli_and_manual_launches_force_unlock() {
        for origin in [DaemonSpawnOrigin::Cli, DaemonSpawnOrigin::Unknown] {
            assert!(
                !is_attended(DaemonRunMode::Standalone, origin, false),
                "{origin:?} has no GUI fallback — must force-unlock"
            );
        }
    }

    #[test]
    fn headless_and_strict_unattended_force_unlock_even_if_gui_spawned() {
        assert!(
            !is_attended(DaemonRunMode::ServerHeadless, DaemonSpawnOrigin::Gui, false),
            "headless has no display/GUI surface — force-unlock"
        );
        assert!(
            !is_attended(DaemonRunMode::Standalone, DaemonSpawnOrigin::Gui, true),
            "strict-unattended overrides the GUI origin — force-unlock"
        );
    }
}
