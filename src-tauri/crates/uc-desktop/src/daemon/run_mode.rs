//! daemon 运行模式。

use uc_daemon_local::process_metadata::DaemonProcessMode;

/// 桌面 daemon 的运行模式。
///
/// 历史上还有 `GuiSidecar`（GUI 子进程，绑 stdin EOF）与 `Hybrid`
/// （持久 daemon 但读 GUI auto-unlock 设置）两个模式——sidecar 模型
/// 在 in-process 化后被全量删除（GUI 不再 spawn 子 daemon），Hybrid 的
/// "尊重 auto-unlock 设置"语义合并进了 [`Self::Standalone`]。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DaemonRunMode {
    /// 独立 daemon binary——CLI `start` detached spawn 出来，或用户手跑。
    /// 自己监听 OS 信号、自己驱动剪贴板/同步服务的 deferred-trigger，
    /// 启动期会读取 `settings.security.auto_unlock_enabled`。
    Standalone,
    /// in-process daemon——GUI 进程内启动，由 caller 持有
    /// [`crate::daemon::DaemonHandle`] 显式 shutdown。
    /// daemon 自己不监听 SIGTERM/SIGINT/stdin EOF，避免抢占 GUI 自己的
    /// 信号 handler；clipboard / sync 服务延迟到 GUI POST `/lifecycle/ready`
    /// 后才启动。
    GuiInProcess,
}

impl DaemonRunMode {
    /// 是否等 GUI 发出 ready 后再启动剪贴板相关服务。
    pub fn waits_for_gui_ready(self) -> bool {
        matches!(self, Self::GuiInProcess)
    }

    /// 是否使用用户设置里的自动解锁开关。
    ///
    /// CLI `start` 拉起的 standalone daemon 与 GUI in-process daemon 都是
    /// "为终端用户跑"，应当尊重 `settings.security.auto_unlock_enabled`。
    pub fn uses_auto_unlock_setting(self) -> bool {
        matches!(self, Self::Standalone | Self::GuiInProcess)
    }

    /// 解锁成功后是否由 daemon 自己触发延迟服务。
    ///
    /// `GuiInProcess` 由 GUI 显式 POST `/lifecycle/ready` 触发；
    /// `Standalone` 没有 GUI 介入，自己解锁后直接放行。
    pub fn auto_triggers_deferred_services(self) -> bool {
        matches!(self, Self::Standalone)
    }

    /// daemon 是否在自己的 main loop 里监听 OS 信号（SIGTERM/SIGINT/Ctrl-C）。
    ///
    /// `GuiInProcess` 跑在 GUI 进程里——OS 信号属于 GUI 的责任范围，daemon
    /// 不能抢占 handler；shutdown 必须通过 [`crate::daemon::DaemonHandle`] 显式触发。
    pub fn listens_to_os_signals(self) -> bool {
        matches!(self, Self::Standalone)
    }

    /// 持久化进 PID 文件的进程模式标记——决定 `cli stop` 能不能 SIGTERM
    /// 这个 daemon。
    ///
    /// `GuiInProcess` → [`DaemonProcessMode::InProcess`]：跟 GUI 同进程，
    /// 不能被外部杀；`Standalone` → [`DaemonProcessMode::Standalone`]，
    /// 可以 SIGTERM。
    pub fn process_mode(self) -> DaemonProcessMode {
        match self {
            Self::GuiInProcess => DaemonProcessMode::InProcess,
            Self::Standalone => DaemonProcessMode::Standalone,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standalone_listens_to_signals_and_drives_itself() {
        let mode = DaemonRunMode::Standalone;
        assert!(mode.listens_to_os_signals());
        assert!(mode.auto_triggers_deferred_services());
        assert!(mode.uses_auto_unlock_setting());
        assert!(!mode.waits_for_gui_ready());
    }

    #[test]
    fn gui_in_process_skips_os_signal_handler() {
        let mode = DaemonRunMode::GuiInProcess;
        assert!(
            !mode.listens_to_os_signals(),
            "in-process daemon must NOT install its own OS signal handler — would race \
             with GUI's own SIGTERM/SIGINT handling"
        );
    }

    #[test]
    fn gui_in_process_defers_clipboard_until_ready() {
        let mode = DaemonRunMode::GuiInProcess;
        assert!(mode.waits_for_gui_ready());
        assert!(!mode.auto_triggers_deferred_services());
        assert!(mode.uses_auto_unlock_setting());
    }

    #[test]
    fn process_mode_only_gui_in_process_is_in_process() {
        // The PID-file mode tag drives `cli stop`'s SIGTERM gate. Only the
        // mode that *literally runs inside a GUI process* must be tagged
        // InProcess.
        assert_eq!(
            DaemonRunMode::GuiInProcess.process_mode(),
            DaemonProcessMode::InProcess
        );
        assert_eq!(
            DaemonRunMode::Standalone.process_mode(),
            DaemonProcessMode::Standalone
        );
    }
}
