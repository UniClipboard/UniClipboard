//! daemon 运行模式。

use uc_daemon_contract::api::types::DaemonResidency;
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
    /// 自己监听 OS 信号、自己驱动剪贴板/同步服务的 deferred-trigger。
    /// 启动期是否走 keyring auto-unlock 不再由 run mode 决定，而由 **D9 启动
    /// 契约**（attended / unattended，见 `startup_recovery`）按 spawn 来源裁定：
    /// GUI-spawned 为 attended（尊重 `auto_unlock_enabled`、等 GUI 解锁），其余
    /// 一律 force-unlock。
    Standalone,
    /// 无头 server daemon——CLI `start --server` 拉起的独立进程，部署在
    /// VPS / 容器里当常驻成员节点。行为与 [`Self::Standalone`] 完全一致
    /// （自监听 OS 信号、强制 keyring 解锁、自驱 deferred services），唯一
    /// 区别是**不接系统剪贴板**：没有 X11/Wayland display，装配走 Noop
    /// 适配器且不 spawn `ClipboardWatcherWorker`——见
    /// [`Self::runs_system_clipboard`]。入站同步、mobile_lan 网关、iroh
    /// P2P 一切照常。
    ServerHeadless,
    /// ADR-008 P5-L L0：一次性（oneshot）daemon 的惰性骨架。
    ///
    /// 目前在**每一个 predicate 上都与 [`Self::Standalone`] 完全一致**——
    /// 接系统剪贴板、自监听 OS 信号、自驱 deferred services、进程模式为
    /// [`DaemonProcessMode::Standalone`]（仍可被 `uniclip stop` SIGTERM）。
    /// 它是后续 sub-step（lease / 自终止 / analytics 门控 / health 字段 /
    /// handover）挂载的预留变体，**当前在生产路径里不可达**：没有任何 spawner
    /// 会发出 [`crate::RUN_MODE_ONESHOT`]，仅 env 解码识别它，行为中立。
    Oneshot,
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

    /// 解锁成功后是否由 daemon 自己触发延迟服务。
    ///
    /// 两个剩余模式都没有同进程 GUI 介入，自己解锁后直接放行(恒 `true`)。
    pub fn auto_triggers_deferred_services(self) -> bool {
        matches!(
            self,
            Self::Standalone | Self::ServerHeadless | Self::Oneshot
        )
    }

    /// daemon 是否在自己的 main loop 里监听 OS 信号（SIGTERM/SIGINT/Ctrl-C）。
    ///
    /// 两个剩余模式都是独立进程,自己处理 OS 信号(恒 `true`)。
    pub fn listens_to_os_signals(self) -> bool {
        matches!(
            self,
            Self::Standalone | Self::ServerHeadless | Self::Oneshot
        )
    }

    /// 是否抑制设备级 presence analytics（`AppFirstOpen` / `AppOpened`）。
    ///
    /// ADR-008 D20: [`Self::Oneshot`] is a transient command-runner spun up for a
    /// single CLI command, so counting it as a device "app open" would inflate
    /// device-level DAU / MAU. We therefore suppress the two device-presence
    /// events for Oneshot ONLY. The process-level `EventContext` registration
    /// still happens unconditionally, so any action-level events the transient
    /// process emits keep flowing with full context.
    ///
    /// [`Self::Standalone`] / [`Self::ServerHeadless`] are persistent residencies
    /// whose process start IS a genuine app open — they keep emitting.
    pub fn suppresses_device_presence_analytics(self) -> bool {
        matches!(self, Self::Oneshot)
    }

    /// 持久化进 PID 文件的进程模式标记——决定 `cli stop` 能不能 SIGTERM
    /// 这个 daemon。
    ///
    /// 两个剩余模式都 → [`DaemonProcessMode::Standalone`],可以被 SIGTERM。
    /// [`DaemonProcessMode::InProcess`] 不再被任何 run-mode 产生,仅保留供读取
    /// 旧版 GUI 写下的 legacy PID 文件(`cli stop` 据此拒绝 SIGTERM)。
    pub fn process_mode(self) -> DaemonProcessMode {
        match self {
            // P5-L L0: Oneshot mirrors Standalone — it must stay SIGTERM-able so
            // `uniclip stop` keeps working until later sub-steps add self-terminate.
            Self::Standalone | Self::ServerHeadless | Self::Oneshot => {
                DaemonProcessMode::Standalone
            }
        }
    }
}

/// Map the daemon's internal run mode onto the wire-stable residency enum
/// surfaced in the health/status handshake (ADR-008 P5-L L1).
///
/// The mapping lives HERE (uc-daemon side) rather than in the contract so
/// `uc-daemon-contract` stays free of any `DaemonRunMode` dependency — the
/// contract owns only the closed wire enum, the daemon owns the translation.
impl From<DaemonRunMode> for DaemonResidency {
    fn from(mode: DaemonRunMode) -> Self {
        match mode {
            DaemonRunMode::Standalone => DaemonResidency::Standalone,
            DaemonRunMode::ServerHeadless => DaemonResidency::ServerHeadless,
            DaemonRunMode::Oneshot => DaemonResidency::Oneshot,
        }
    }
}

/// Map a controlled-restart target residency to its [`RUN_MODE_ENV`] string for
/// the handover record (ADR-008 P5-L L8c).
///
/// Standalone is the absent-env default, so it maps to an empty string (the
/// spawner sets `RUN_MODE_ENV=""` which decodes back to Standalone). Oneshot is
/// never a valid promotion target — the `/lifecycle/restart` handler rejects it
/// with `InvalidTarget` — so it is unreachable here; defensively map it to `""`
/// too rather than panic.
///
/// [`RUN_MODE_ENV`]: uc_daemon_local::spawn_contract::RUN_MODE_ENV
pub fn residency_to_run_mode_env(target: DaemonResidency) -> String {
    use uc_daemon_local::spawn_contract::RUN_MODE_SERVER;
    match target {
        DaemonResidency::ServerHeadless => RUN_MODE_SERVER.to_string(),
        DaemonResidency::Standalone | DaemonResidency::Oneshot => String::new(),
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

    #[test]
    fn oneshot_mirrors_standalone_in_every_predicate() {
        // ADR-008 P5-L L0: Oneshot is an inert skeleton that, FOR NOW, must be
        // behavior-identical to Standalone in every predicate. Later sub-steps
        // (lease / self-terminate / analytics gating / health / handover) will
        // diverge it; until then it stays unreachable and behavior-neutral.
        let oneshot = DaemonRunMode::Oneshot;
        let standalone = DaemonRunMode::Standalone;

        assert_eq!(
            oneshot.runs_system_clipboard(),
            standalone.runs_system_clipboard()
        );
        assert!(
            oneshot.runs_system_clipboard(),
            "oneshot runs the OS clipboard exactly like standalone"
        );

        assert_eq!(
            oneshot.auto_triggers_deferred_services(),
            standalone.auto_triggers_deferred_services()
        );
        assert!(oneshot.auto_triggers_deferred_services());

        assert_eq!(
            oneshot.listens_to_os_signals(),
            standalone.listens_to_os_signals()
        );
        assert!(oneshot.listens_to_os_signals());

        assert_eq!(oneshot.process_mode(), standalone.process_mode());
        assert_eq!(
            oneshot.process_mode(),
            DaemonProcessMode::Standalone,
            "oneshot must stay SIGTERM-able for `uniclip stop`"
        );

        assert_eq!(
            oneshot.waits_for_gui_ready(),
            standalone.waits_for_gui_ready()
        );
        assert!(!oneshot.waits_for_gui_ready());
    }

    #[test]
    fn only_oneshot_suppresses_device_presence_analytics() {
        // ADR-008 D20: a transient Oneshot command-runner must not be counted as
        // a device "app open" (it would inflate DAU / MAU), so it suppresses the
        // device-level presence events. Persistent residencies keep emitting —
        // their process start IS a real app open.
        assert!(
            DaemonRunMode::Oneshot.suppresses_device_presence_analytics(),
            "oneshot must suppress device-presence analytics (D20)"
        );
        assert!(!DaemonRunMode::Standalone.suppresses_device_presence_analytics());
        assert!(!DaemonRunMode::ServerHeadless.suppresses_device_presence_analytics());
    }

    #[test]
    fn run_mode_maps_to_wire_residency() {
        // ADR-008 P5-L L1: the health/status handshake reports residency. Each
        // run mode must map to its own distinct wire variant so a persistent
        // client can later detect an Oneshot (R8-F2) and a CLI version-check
        // (L2) can read it.
        assert_eq!(
            DaemonResidency::from(DaemonRunMode::Standalone),
            DaemonResidency::Standalone
        );
        assert_eq!(
            DaemonResidency::from(DaemonRunMode::ServerHeadless),
            DaemonResidency::ServerHeadless
        );
        assert_eq!(
            DaemonResidency::from(DaemonRunMode::Oneshot),
            DaemonResidency::Oneshot
        );
    }

    #[test]
    fn each_run_mode_surfaces_matching_residency_in_health_and_status() {
        // ADR-008 P5-L L1 GATE: a `DaemonApiState` built from each
        // `DaemonRunMode` must surface that mode's residency in BOTH the
        // health and the status handshake bodies. Building a full
        // `DaemonApiState` here is infeasible (it needs a real `AppFacade`
        // composed from ~18 sub-facades + ports), so we drive the same
        // assembly seam the runtime uses — `DaemonApiState::with_residency`
        // is fed `run_mode.into()` at `build_daemon_app_instance` /
        // `DaemonApp::build_api_state`, and `health_response()` /
        // `status_response()` are verbatim copies of `self.residency`. We
        // therefore exercise that exact handler-emission logic per run mode.
        use uc_webserver::api::server::DaemonApiState;

        for (mode, expected) in [
            (DaemonRunMode::Standalone, DaemonResidency::Standalone),
            (
                DaemonRunMode::ServerHeadless,
                DaemonResidency::ServerHeadless,
            ),
            (DaemonRunMode::Oneshot, DaemonResidency::Oneshot),
        ] {
            // Same value the assembly boundary injects via `.with_residency`.
            let residency_for_state: DaemonResidency = mode.into();
            assert_eq!(
                residency_for_state, expected,
                "run mode {mode:?} must map to {expected:?} before it reaches DaemonApiState"
            );

            // `health_response()` / `status_response()` copy `self.residency`
            // verbatim, so assert the bodies a state with this residency emits.
            let health = DaemonApiState::health_response_for(residency_for_state);
            assert_eq!(
                health.residency, expected,
                "GET /health must report {expected:?} for run mode {mode:?}"
            );

            let status = DaemonApiState::status_response_for(residency_for_state);
            assert_eq!(
                status.residency, expected,
                "GET /status must report {expected:?} for run mode {mode:?}"
            );
        }
    }

    #[test]
    fn residency_maps_to_run_mode_env_for_handover() {
        // ADR-008 P5-L L8c: the handover record carries the RUN_MODE_ENV string
        // the spawner will set on the successor daemon. ServerHeadless → "server";
        // Standalone → "" (the absent-env default decodes back to Standalone).
        use uc_daemon_local::spawn_contract::RUN_MODE_SERVER;

        assert_eq!(
            residency_to_run_mode_env(DaemonResidency::ServerHeadless),
            RUN_MODE_SERVER
        );
        assert_eq!(
            residency_to_run_mode_env(DaemonResidency::Standalone),
            "",
            "Standalone is the absent-env default — empty string round-trips to Standalone"
        );
        // Oneshot is never a valid promotion target (the restart handler rejects
        // it); it is defensively mapped to "" rather than panicking.
        assert_eq!(residency_to_run_mode_env(DaemonResidency::Oneshot), "");
    }
}
