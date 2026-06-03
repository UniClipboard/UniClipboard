//! daemon 运行模式。

use uc_daemon_local::process_metadata::DaemonProcessMode;

/// 桌面 daemon 的运行模式。
///
/// 历史上还有 `GuiSidecar`（GUI 子进程，绑 stdin EOF）、`Hybrid`（持久
/// daemon 但读 GUI auto-unlock 设置）与 `GuiInProcess`（GUI 进程内 daemon）
/// 三个模式。sidecar 在 in-process 化后删除；Hybrid 的"尊重 auto-unlock"
/// 语义合并进了 [`Self::Standalone`]；`GuiInProcess` 在 ADR-008 P3-3 (B2'-3)
/// GUI 转纯客户端后删除——daemon 永远是独立进程,GUI 通过 detached spawn
/// 拉起它。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DaemonRunMode {
    /// 独立 daemon binary——CLI `start` / GUI detached spawn 出来，或用户手跑。
    /// 自己监听 OS 信号、自己驱动剪贴板/同步服务的 deferred-trigger，
    /// 启动期**强制走 keyring auto-unlock，无视 settings**——见
    /// [`Self::uses_auto_unlock_setting`] 的注释。
    Standalone,
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
    ///
    /// ADR-008 P3-3 (B2'-3): 唯一会等待的 `GuiInProcess` 模式已删除——daemon
    /// 永远是独立进程,没有同进程 GUI 来发 `/lifecycle/ready`,所以恒为
    /// `false`。clipboard 服务的启动改由 daemon 自己的解锁路径放行。
    pub fn waits_for_gui_ready(self) -> bool {
        false
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
    /// ADR-008 P3-3 (B2'-3): 唯一返回 `true` 的 `GuiInProcess` 已删除,所以
    /// 恒为 `false`——daemon 永远是独立进程,**没有 GUI 做兜底**:如果启动期
    /// 不解锁,剪贴板 watcher / 同步 worker 全部卡在 deferred 队列里
    /// (`should_defer_clipboard = !encryption_unlocked`),而又没有同进程 GUI
    /// 来调 `/lifecycle/ready`——daemon 看似活着实际什么也不做。所以独立
    /// daemon 必须无视用户的 `auto_unlock_enabled` 设置强制 keyring 解锁;
    /// keyring 失败再走错误路径退出。GUI 端的 auto-unlock 偏好改由 GUI 自己
    /// 在连上 daemon 后 `POST /encryption/unlock` 落实(见 run.rs 启动任务)。
    pub fn uses_auto_unlock_setting(self) -> bool {
        false
    }

    /// 解锁成功后是否由 daemon 自己触发延迟服务。
    ///
    /// 两个剩余模式都没有同进程 GUI 介入，自己解锁后直接放行(恒 `true`)。
    pub fn auto_triggers_deferred_services(self) -> bool {
        matches!(self, Self::Standalone | Self::ServerHeadless)
    }

    /// daemon 是否在自己的 main loop 里监听 OS 信号（SIGTERM/SIGINT/Ctrl-C）。
    ///
    /// 两个剩余模式都是独立进程,自己处理 OS 信号(恒 `true`)。
    pub fn listens_to_os_signals(self) -> bool {
        matches!(self, Self::Standalone | Self::ServerHeadless)
    }

    /// 持久化进 PID 文件的进程模式标记——决定 `cli stop` 能不能 SIGTERM
    /// 这个 daemon。
    ///
    /// 两个剩余模式都 → [`DaemonProcessMode::Standalone`],可以被 SIGTERM。
    /// [`DaemonProcessMode::InProcess`] 不再被任何 run-mode 产生,仅保留供读取
    /// 旧版 GUI 写下的 legacy PID 文件(`cli stop` 据此拒绝 SIGTERM)。
    pub fn process_mode(self) -> DaemonProcessMode {
        match self {
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
    fn process_mode_is_always_standalone() {
        // ADR-008 P3-3 (B2'-3): no run mode produces `InProcess` anymore (the
        // GuiInProcess variant is gone). The PID-file mode tag drives `cli
        // stop`'s SIGTERM gate; every spawned daemon is now Standalone (kill-able).
        assert_eq!(
            DaemonRunMode::Standalone.process_mode(),
            DaemonProcessMode::Standalone
        );
        assert_eq!(
            DaemonRunMode::ServerHeadless.process_mode(),
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
        assert!(
            !DaemonRunMode::ServerHeadless.runs_system_clipboard(),
            "headless server must not touch the OS clipboard (no display)"
        );
    }
}
