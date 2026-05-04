//! daemon 运行模式。

use std::fmt;

use uc_daemon_local::process_metadata::DaemonProcessMode;

/// 桌面 daemon 的运行模式。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DaemonRunMode {
    /// 独立 daemon，由 CLI 或用户直接启动。
    Standalone,
    /// GUI sidecar，由 Tauri GUI 启动，生命周期跟随 GUI。
    ///
    /// 旧 sidecar 模型遗留——daemon 是 GUI 的子进程，stdin EOF = 父进程死。
    /// in-process 化迁移完成后会随 sidecar 拉起代码一起删除。
    GuiSidecar,
    /// 常驻 daemon，GUI 只是连接它的客户端。
    Hybrid,
    /// in-process daemon——GUI 进程内启动，由 caller 持有 [`crate::daemon::DaemonHandle`]
    /// 显式 shutdown，daemon 自己不监听 SIGTERM/SIGINT/stdin EOF（避免抢占 GUI
    /// 自己的信号 handler）。
    GuiInProcess,
}

/// daemon 运行模式参数错误。
#[derive(Debug)]
pub struct DaemonRunModeParseError {
    message: &'static str,
}

impl fmt::Display for DaemonRunModeParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message)
    }
}

impl std::error::Error for DaemonRunModeParseError {}

impl DaemonRunMode {
    /// 从 daemon 命令行标志转换到明确的运行模式。
    pub fn from_flags(gui_managed: bool, hybrid: bool) -> Result<Self, DaemonRunModeParseError> {
        match (gui_managed, hybrid) {
            (true, true) => Err(DaemonRunModeParseError {
                message: "--hybrid cannot be combined with --gui-managed",
            }),
            (true, false) => Ok(Self::GuiSidecar),
            (false, true) => Ok(Self::Hybrid),
            (false, false) => Ok(Self::Standalone),
        }
    }

    /// 是否需要跟随 GUI 父进程退出。
    pub fn follows_gui_parent(self) -> bool {
        matches!(self, Self::GuiSidecar)
    }

    /// 是否等 GUI 发出 ready 后再启动剪贴板相关服务。
    pub fn waits_for_gui_ready(self) -> bool {
        matches!(self, Self::GuiSidecar | Self::GuiInProcess)
    }

    /// 是否使用用户设置里的自动解锁开关。
    pub fn uses_auto_unlock_setting(self) -> bool {
        matches!(self, Self::GuiSidecar | Self::Hybrid | Self::GuiInProcess)
    }

    /// 解锁成功后是否由 daemon 自己触发延迟服务。
    pub fn auto_triggers_deferred_services(self) -> bool {
        !matches!(self, Self::GuiSidecar | Self::GuiInProcess)
    }

    /// daemon 是否在自己的 main loop 里监听 OS 信号（SIGTERM/SIGINT/Ctrl-C）。
    ///
    /// `GuiInProcess` 跑在 GUI 进程里——OS 信号属于 GUI 的责任范围，daemon
    /// 不能抢占 handler；shutdown 必须通过 [`crate::daemon::DaemonHandle`] 显式触发。
    pub fn listens_to_os_signals(self) -> bool {
        !matches!(self, Self::GuiInProcess)
    }

    /// 持久化进 PID 文件的进程模式标记——决定 `cli stop` 能不能 SIGTERM
    /// 这个 daemon。
    ///
    /// `GuiInProcess` → [`DaemonProcessMode::InProcess`]：跟 GUI 同进程，
    /// 不能被外部杀。其他模式（Standalone / Hybrid / 旧 GuiSidecar）都是
    /// 独立进程 → [`DaemonProcessMode::Standalone`]，可以 SIGTERM。
    pub fn process_mode(self) -> DaemonProcessMode {
        match self {
            Self::GuiInProcess => DaemonProcessMode::InProcess,
            Self::Standalone | Self::Hybrid | Self::GuiSidecar => DaemonProcessMode::Standalone,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daemon_flags_map_to_run_mode() {
        assert_eq!(
            DaemonRunMode::from_flags(true, false).unwrap(),
            DaemonRunMode::GuiSidecar
        );
        assert_eq!(
            DaemonRunMode::from_flags(false, false).unwrap(),
            DaemonRunMode::Standalone
        );
        assert_eq!(
            DaemonRunMode::from_flags(false, true).unwrap(),
            DaemonRunMode::Hybrid
        );
        assert!(DaemonRunMode::from_flags(true, true).is_err());
    }

    #[test]
    fn hybrid_behaves_as_persistent_daemon() {
        let mode = DaemonRunMode::Hybrid;

        assert!(!mode.follows_gui_parent());
        assert!(!mode.waits_for_gui_ready());
        assert!(mode.uses_auto_unlock_setting());
        assert!(mode.auto_triggers_deferred_services());
    }

    #[test]
    fn gui_in_process_skips_os_signal_handler() {
        let mode = DaemonRunMode::GuiInProcess;

        assert!(
            !mode.listens_to_os_signals(),
            "in-process daemon must NOT install its own OS signal handler — would race \
             with GUI's own SIGTERM/SIGINT handling"
        );
        assert!(
            !mode.follows_gui_parent(),
            "in-process daemon shares GUI's process; stdin EOF (used by sidecar) \
             does not apply"
        );
    }

    #[test]
    fn gui_in_process_defers_clipboard_until_ready() {
        let mode = DaemonRunMode::GuiInProcess;

        assert!(
            mode.waits_for_gui_ready(),
            "GUI still drives lifecycle/ready — clipboard services must defer \
             until the frontend signals it's ready, just like GuiSidecar"
        );
        assert!(
            !mode.auto_triggers_deferred_services(),
            "GUI explicitly POSTs /lifecycle/ready; daemon should not auto-trigger"
        );
        assert!(
            mode.uses_auto_unlock_setting(),
            "GUI users have an auto_unlock_enabled setting; respect it like GuiSidecar"
        );
    }

    #[test]
    fn standalone_listens_to_signals() {
        let mode = DaemonRunMode::Standalone;
        assert!(
            mode.listens_to_os_signals(),
            "independent daemon binary needs SIGTERM/SIGINT to shut down cleanly"
        );
    }

    #[test]
    fn process_mode_only_gui_in_process_is_in_process() {
        // The PID-file mode tag drives `cli stop`'s SIGTERM gate. Only the
        // mode that *literally runs inside a GUI process* must be tagged
        // InProcess; everything else (including the legacy GuiSidecar
        // sub-process model) is a separate OS process and is fair game
        // for `cli stop`.
        assert_eq!(
            DaemonRunMode::GuiInProcess.process_mode(),
            DaemonProcessMode::InProcess
        );
        assert_eq!(
            DaemonRunMode::Standalone.process_mode(),
            DaemonProcessMode::Standalone
        );
        assert_eq!(
            DaemonRunMode::Hybrid.process_mode(),
            DaemonProcessMode::Standalone
        );
        assert_eq!(
            DaemonRunMode::GuiSidecar.process_mode(),
            DaemonProcessMode::Standalone
        );
    }
}
