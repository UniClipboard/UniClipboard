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
    /// 启动期**强制走 keyring auto-unlock，无视 settings**——见
    /// [`Self::uses_auto_unlock_setting`] 的注释。
    Standalone,
    /// in-process daemon——GUI 进程内启动，由 caller 持有
    /// [`crate::daemon::DaemonHandle`] 显式 shutdown。
    /// daemon 自己不监听 SIGTERM/SIGINT/stdin EOF，避免抢占 GUI 自己的
    /// 信号 handler；clipboard / sync 服务延迟到 GUI POST `/lifecycle/ready`
    /// 后才启动。
    GuiInProcess,
    /// 无头 server daemon——CLI `start --server` 拉起的独立进程，部署在
    /// VPS / 容器里当常驻成员节点。行为与 [`Self::Standalone`] 完全一致
    /// （自监听 OS 信号、强制 keyring 解锁、自驱 deferred services），唯一
    /// 区别是**不接系统剪贴板**：没有 X11/Wayland display，装配走 Noop
    /// 适配器且不 spawn `ClipboardWatcherWorker`——见
    /// [`Self::runs_system_clipboard`]。入站同步、mobile_lan 网关、iroh
    /// P2P 一切照常。
    ServerHeadless,
}

impl DaemonRunMode {
    /// 是否等 GUI 发出 ready 后再启动剪贴板相关服务。
    pub fn waits_for_gui_ready(self) -> bool {
        matches!(self, Self::GuiInProcess)
    }

    /// daemon 是否接管系统剪贴板（读 OS 剪贴板做出站捕获 + 写 OS 剪贴板）。
    ///
    /// 只有 [`Self::ServerHeadless`] 返回 `false`：无头节点没有 X11/Wayland
    /// display，装配时用 Noop 适配器替代真实剪贴板，并跳过
    /// `ClipboardWatcherWorker`（没有 OS 剪贴板可监听）。入站内容仍会落库 +
    /// 经 mobile_lan / fan-out 流转，只是不写本机系统剪贴板。
    pub fn runs_system_clipboard(self) -> bool {
        !matches!(self, Self::ServerHeadless)
    }

    /// 是否让用户的 `settings.security.auto_unlock_enabled` 决定 daemon
    /// 启动期是否尝试 keyring 解锁。
    ///
    /// **只有 [`Self::GuiInProcess`] 返回 `true`**——[`Self::Standalone`] 必须
    /// 强制无脑 keyring 解锁，原因是这两种模式下"用户怎么解锁"的通道不一样：
    ///
    /// - **GuiInProcess**：daemon 跟 GUI 同进程，前端能弹解锁框、调
    ///   `POST /encryption/unlock`、调 `POST /lifecycle/ready` 触发
    ///   deferred services。所以 `auto_unlock=false` 还能 fallback 到
    ///   "用户手动在前端解锁"——daemon 启动期不解锁不影响最终能用。
    /// - **Standalone**：daemon 是 CLI `start` 拉起的独立进程，**没有 GUI
    ///   做兜底**。如果启动期不解锁，剪贴板 watcher / 同步 worker 全部
    ///   卡在 deferred 队列里（`should_defer_clipboard = !encryption_unlocked`），
    ///   而又没有任何外部入口来调 `/lifecycle/ready`——daemon 看似活着，
    ///   实际上什么也不做。所以 standalone daemon 必须无视用户的
    ///   `auto_unlock_enabled` 设置，强制 keyring 解锁；keyring 失败再走
    ///   错误路径退出。这是 Phase E2 collapse run modes 时遗漏的回归——
    ///   旧 Hybrid 的"读 setting"语义不能直接照搬到 Standalone，因为
    ///   Standalone 没有 GUI 这个 fallback。
    pub fn uses_auto_unlock_setting(self) -> bool {
        matches!(self, Self::GuiInProcess)
    }

    /// 解锁成功后是否由 daemon 自己触发延迟服务。
    ///
    /// `GuiInProcess` 由 GUI 显式 POST `/lifecycle/ready` 触发；
    /// `Standalone` / `ServerHeadless` 没有 GUI 介入，自己解锁后直接放行。
    pub fn auto_triggers_deferred_services(self) -> bool {
        matches!(self, Self::Standalone | Self::ServerHeadless)
    }

    /// daemon 是否在自己的 main loop 里监听 OS 信号（SIGTERM/SIGINT/Ctrl-C）。
    ///
    /// `GuiInProcess` 跑在 GUI 进程里——OS 信号属于 GUI 的责任范围，daemon
    /// 不能抢占 handler；shutdown 必须通过 [`crate::daemon::DaemonHandle`] 显式触发。
    pub fn listens_to_os_signals(self) -> bool {
        matches!(self, Self::Standalone | Self::ServerHeadless)
    }

    /// 持久化进 PID 文件的进程模式标记——决定 `cli stop` 能不能 SIGTERM
    /// 这个 daemon。
    ///
    /// `GuiInProcess` → [`DaemonProcessMode::InProcess`]：跟 GUI 同进程，
    /// 不能被外部杀；`Standalone` / `ServerHeadless` →
    /// [`DaemonProcessMode::Standalone`]，可以 SIGTERM。
    pub fn process_mode(self) -> DaemonProcessMode {
        match self {
            Self::GuiInProcess => DaemonProcessMode::InProcess,
            Self::Standalone | Self::ServerHeadless => DaemonProcessMode::Standalone,
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
        assert!(!mode.waits_for_gui_ready());
    }

    #[test]
    fn standalone_force_unlocks_ignoring_user_setting() {
        // Standalone daemon (CLI start) 必须无脑走 keyring 解锁——它没有
        // GUI 通道接收用户的手动解锁/lifecycle ready 信号，如果启动期
        // 不解锁，clipboard watcher 永远卡在 deferred 队列，daemon 看似
        // 活着实际什么也不做。Phase E2 把它和 GuiInProcess 一视同仁去
        // 读 setting 是回归——下面这个 false 是这个回归的反向回归测试。
        assert!(
            !DaemonRunMode::Standalone.uses_auto_unlock_setting(),
            "standalone daemon must force keyring unlock — no GUI fallback exists"
        );
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

    #[test]
    fn server_headless_matches_standalone_except_clipboard() {
        // 无头 server 与 standalone 在进程模型 / OS 信号 / 强制解锁 /
        // 自驱 deferred 上完全一致 —— 它也是 CLI 拉起的独立进程,没有 GUI
        // 通道兜底。唯一区别是不接系统剪贴板。
        let mode = DaemonRunMode::ServerHeadless;
        assert!(mode.listens_to_os_signals());
        assert!(mode.auto_triggers_deferred_services());
        assert!(!mode.waits_for_gui_ready());
        assert!(
            !mode.uses_auto_unlock_setting(),
            "server daemon has no GUI fallback — must force keyring unlock like standalone"
        );
        assert_eq!(mode.process_mode(), DaemonProcessMode::Standalone);
    }

    #[test]
    fn only_server_headless_skips_system_clipboard() {
        assert!(DaemonRunMode::Standalone.runs_system_clipboard());
        assert!(DaemonRunMode::GuiInProcess.runs_system_clipboard());
        assert!(
            !DaemonRunMode::ServerHeadless.runs_system_clipboard(),
            "headless server must not touch the OS clipboard (no display)"
        );
    }
}
