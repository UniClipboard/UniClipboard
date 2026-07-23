//! Mobile LAN compatibility listener lifecycle.
//!
//! The daemon owns this controller beside the shared [`Engine`]. Startup,
//! settings changes, and shutdown reconcile one listener to a desired local
//! target. Listener restarts reuse the same engine and clipboard event stream;
//! they never assemble another business runtime.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::{broadcast, Mutex};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

use uc_engine::{
    ActiveClipboardChanged, Engine, MobileLanEndpointUpdate, MobileSyncSettingsSummary, Operation,
    OperationResult,
};
use uc_webserver::mobile_lan::{start_mobile_lan_server, MobileLanServerHandle};

struct RunningListener {
    port: u16,
    cancel: CancellationToken,
    join: JoinHandle<anyhow::Result<()>>,
}

#[async_trait]
trait LanListenerSpawner: Send + Sync {
    async fn spawn(
        &self,
        bind: SocketAddr,
        cancel: CancellationToken,
    ) -> anyhow::Result<MobileLanServerHandle>;
}

/// Production listener spawner backed by the daemon's mobile-sync capability.
///
/// `sse_source` is a process-level singleton (constructed once at wire time;
/// `BroadcastingAdvance` is the sole publisher). It is stored as a
/// construction-time field of the spawner rather than a per-spawn parameter:
/// the spawner is long-lived and every listener restart (port change /
/// toggle) reuses the same `Sender`, so there is no need to thread it
/// through the [`LanListenerSpawner`] trait signature.
struct MobileSyncListenerSpawner {
    engine: Arc<Engine>,
    sse_source: broadcast::Sender<ActiveClipboardChanged>,
}

impl MobileSyncListenerSpawner {
    fn new(engine: Arc<Engine>, sse_source: broadcast::Sender<ActiveClipboardChanged>) -> Self {
        Self { engine, sse_source }
    }
}

#[async_trait]
impl LanListenerSpawner for MobileSyncListenerSpawner {
    async fn spawn(
        &self,
        bind: SocketAddr,
        cancel: CancellationToken,
    ) -> anyhow::Result<MobileLanServerHandle> {
        start_mobile_lan_server(
            bind,
            cancel,
            Arc::clone(&self.engine),
            self.sse_source.clone(),
        )
        .await
    }
}

pub(crate) struct MobileLanLifecycleController {
    status_sink: Arc<dyn LanListenerStatusSink>,
    spawner: Arc<dyn LanListenerSpawner>,
    state: Mutex<Option<RunningListener>>,
}

#[async_trait]
trait LanListenerStatusSink: Send + Sync {
    async fn stopped(&self);
    async fn listening(&self, url: String);
    async fn bind_failed(&self, reason: String);
}

struct EngineLanListenerStatusSink {
    engine: Arc<Engine>,
}

impl EngineLanListenerStatusSink {
    async fn update(&self, update: MobileLanEndpointUpdate) {
        match self
            .engine
            .execute(Operation::UpdateMobileLanEndpoint(update))
            .await
        {
            Ok(OperationResult::MobileLanEndpointUpdated) => {}
            Ok(_) => tracing::warn!("engine returned an unexpected LAN endpoint result"),
            Err(error) => tracing::warn!(
                code = error.code(),
                category = %error.category(),
                "failed to update engine LAN endpoint status"
            ),
        }
    }
}

#[async_trait]
impl LanListenerStatusSink for EngineLanListenerStatusSink {
    async fn stopped(&self) {
        self.update(MobileLanEndpointUpdate::Stopped).await;
    }

    async fn listening(&self, url: String) {
        self.update(MobileLanEndpointUpdate::Listening { base_url: url })
            .await;
    }

    async fn bind_failed(&self, reason: String) {
        self.update(MobileLanEndpointUpdate::BindFailed { reason })
            .await;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LanListenerTarget {
    Disabled,
    Enabled { port: u16 },
}

impl MobileLanLifecycleController {
    pub(crate) fn for_engine(
        engine: Arc<Engine>,
        sse_source: broadcast::Sender<ActiveClipboardChanged>,
    ) -> Self {
        let spawner = Arc::new(MobileSyncListenerSpawner::new(
            Arc::clone(&engine),
            sse_source,
        ));
        Self {
            status_sink: Arc::new(EngineLanListenerStatusSink { engine }),
            spawner,
            state: Mutex::new(None),
        }
    }

    #[cfg(test)]
    fn with_adapters(
        status_sink: Arc<dyn LanListenerStatusSink>,
        spawner: Arc<dyn LanListenerSpawner>,
    ) -> Self {
        Self {
            status_sink,
            spawner,
            state: Mutex::new(None),
        }
    }

    pub(crate) async fn disable(&self) {
        self.reconcile(LanListenerTarget::Disabled).await;
    }

    /// Stops the current listener while holding the transition lock.
    async fn stop_locked(&self, guard: &mut tokio::sync::MutexGuard<'_, Option<RunningListener>>) {
        if let Some(running) = guard.take() {
            running.cancel.cancel();
            match running.join.await {
                Ok(Ok(())) => info!(port = running.port, "mobile LAN listener stopped"),
                Ok(Err(e)) => {
                    warn!(port = running.port, error = %e, "mobile LAN listener exited with error")
                }
                Err(join_err) => {
                    warn!(port = running.port, error = %join_err, "mobile LAN listener task join failed")
                }
            }
            self.status_sink.stopped().await;
        }
    }

    /// Starts a listener while holding the transition lock.
    async fn start_locked(
        &self,
        guard: &mut tokio::sync::MutexGuard<'_, Option<RunningListener>>,
        port: u16,
    ) {
        let bind = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), port);
        let cancel = CancellationToken::new();
        match self.spawner.spawn(bind, cancel.clone()).await {
            Ok(handle) => {
                let url = format!("http://{}", handle.bound_addr);
                self.status_sink.listening(url.clone()).await;
                info!(url, "mobile LAN listener started");
                **guard = Some(RunningListener {
                    port,
                    cancel,
                    join: handle.join_handle,
                });
            }
            Err(e) => {
                let reason = format!("{}", e);
                self.status_sink.bind_failed(reason.clone()).await;
                error!(
                    bind = %bind,
                    error = %e,
                    "mobile LAN listener failed to bind"
                );
                // Keep the state empty so the next reconciliation can retry.
            }
        }
    }

    pub(crate) async fn reconcile(&self, target: LanListenerTarget) {
        let mut guard = self.state.lock().await;
        let current_port = guard.as_ref().map(|r| r.port);
        match (current_port, target) {
            (None, LanListenerTarget::Disabled) => {}
            (Some(_), LanListenerTarget::Disabled) => {
                self.stop_locked(&mut guard).await;
            }
            (None, LanListenerTarget::Enabled { port }) => {
                self.start_locked(&mut guard, port).await;
            }
            (Some(p), LanListenerTarget::Enabled { port }) if p == port => {}
            (Some(_), LanListenerTarget::Enabled { port }) => {
                self.stop_locked(&mut guard).await;
                self.start_locked(&mut guard, port).await;
            }
        }
    }
}

/// 移动端 LAN 监听器的默认端口 —— settings 未显式设 `lan_port` 时取此值
/// (SPEC §3.2)。SyncClipboard 客户端默认指向它,改动会破坏既有手机配置。
pub(crate) const DEFAULT_MOBILE_LAN_PORT: u16 = 42720;

/// 由持久化的移动端同步设置推导 daemon 启动期的 LAN 监听器目标状态。
///
/// 仅当总开关 (`enabled`) 与 LAN 子开关 (`lan_listen_enabled`) **同时**打开
/// 时才起监听器;端口缺省取 [`DEFAULT_MOBILE_LAN_PORT`]。`view` 为 `None`
/// (启动期 settings 读取失败) 时保守返回 [`LanListenerTarget::Disabled`] ——
/// 之后仍可经设置变更把监听器拉起。
///
/// **决策只依赖 settings,不接受任何 daemon 运行模式入参**:无头 server 节点
/// ([`DaemonRunMode::ServerHeadless`](crate::daemon::run_mode::DaemonRunMode::ServerHeadless))
/// 与普通 daemon 起的是同一个手机网关。保持本签名与 run mode 无关,正是这条
/// 不变量的结构性保证 (issue #899 / ADR-007 §2.3)。
pub(crate) fn initial_lan_target(view: Option<&MobileSyncSettingsSummary>) -> LanListenerTarget {
    match view {
        Some(view) => mobile_lan_target(view.enabled, view.lan_listen_enabled, view.lan_port),
        _ => LanListenerTarget::Disabled,
    }
}

pub(crate) fn mobile_lan_target(
    enabled: bool,
    lan_listen_enabled: bool,
    lan_port: Option<u16>,
) -> LanListenerTarget {
    if enabled && lan_listen_enabled {
        LanListenerTarget::Enabled {
            port: lan_port.unwrap_or(DEFAULT_MOBILE_LAN_PORT),
        }
    } else {
        LanListenerTarget::Disabled
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum RecordedStatus {
        Stopped,
        Listening(String),
        BindFailed(String),
    }

    struct RecordingStatusSink {
        status: Mutex<RecordedStatus>,
    }

    impl RecordingStatusSink {
        fn new() -> Self {
            Self {
                status: Mutex::new(RecordedStatus::Stopped),
            }
        }

        async fn current(&self) -> RecordedStatus {
            self.status.lock().await.clone()
        }
    }

    #[async_trait]
    impl LanListenerStatusSink for RecordingStatusSink {
        async fn stopped(&self) {
            *self.status.lock().await = RecordedStatus::Stopped;
        }

        async fn listening(&self, url: String) {
            *self.status.lock().await = RecordedStatus::Listening(url);
        }

        async fn bind_failed(&self, reason: String) {
            *self.status.lock().await = RecordedStatus::BindFailed(reason);
        }
    }

    /// 测试用 spawner:起一个空 axum router 服务,可被 cancel。
    ///
    /// This test spawner only verifies controller transitions and endpoint
    /// status updates; it does not construct an application runtime.
    struct FakeSpawner {
        starts: AtomicU32,
        fail_next_starts: AtomicU32,
    }

    impl FakeSpawner {
        fn new() -> Self {
            Self {
                starts: AtomicU32::new(0),
                fail_next_starts: AtomicU32::new(0),
            }
        }

        fn arm_bind_failure(&self, n: u32) {
            self.fail_next_starts.store(n, Ordering::SeqCst);
        }
    }

    #[async_trait]
    impl LanListenerSpawner for FakeSpawner {
        async fn spawn(
            &self,
            bind: SocketAddr,
            cancel: CancellationToken,
        ) -> anyhow::Result<MobileLanServerHandle> {
            self.starts.fetch_add(1, Ordering::SeqCst);

            // 注:fail_next_starts 不影响 starts 计数(增 1 后才判定),
            // 单测断言"spawn 被调几次"包含失败调用,这与生产语义一致(controller
            // 视角"我请求 spawn N 次")。
            let remaining = self.fail_next_starts.load(Ordering::SeqCst);
            if remaining > 0 {
                self.fail_next_starts.store(remaining - 1, Ordering::SeqCst);
                anyhow::bail!("simulated bind failure on {}", bind);
            }

            // 纯假 spawner:不真实 bind,bound_addr 直接回 bind 入参,join
            // handle 等 cancel。这样测试可以用任何固定端口(包括 12345 这种
            // 常用占用风险端口)而不撞本机环境真实端口冲突,也避免连续
            // bind/drop 同端口在某些平台触发的瞬时 TIME_WAIT。
            let bound_addr = bind;
            let join_handle = tokio::spawn(async move {
                cancel.cancelled().await;
                Ok::<(), anyhow::Error>(())
            });
            Ok(MobileLanServerHandle {
                bound_addr,
                join_handle,
            })
        }
    }

    fn build(
        spawner: Arc<FakeSpawner>,
    ) -> (Arc<MobileLanLifecycleController>, Arc<RecordingStatusSink>) {
        let status = Arc::new(RecordingStatusSink::new());
        let controller = Arc::new(MobileLanLifecycleController::with_adapters(
            status.clone(),
            spawner,
        ));
        (controller, status)
    }

    /// 测试用固定端口。FakeSpawner 已经不真实 bind, 这里随便选两个不冲突
    /// 的就行 —— 不会撞本机环境。选生产默认值 [`DEFAULT_MOBILE_LAN_PORT`] 确保
    /// "同端口 no-op" 这条测试断言的语义就是生产意义上的"用户两次保存同
    /// 一个端口"。
    const FIXED_PORT_A: u16 = DEFAULT_MOBILE_LAN_PORT;
    const FIXED_PORT_B: u16 = 51234;

    #[tokio::test]
    async fn apply_disabled_from_none_is_noop() {
        let spawner = Arc::new(FakeSpawner::new());
        let (c, ei) = build(spawner.clone());

        c.reconcile(LanListenerTarget::Disabled).await;

        assert_eq!(spawner.starts.load(Ordering::SeqCst), 0);
        assert_eq!(ei.current().await, RecordedStatus::Stopped);
    }

    #[tokio::test]
    async fn apply_enabled_from_none_starts_listener() {
        let spawner = Arc::new(FakeSpawner::new());
        let (c, ei) = build(spawner.clone());

        c.reconcile(LanListenerTarget::Enabled { port: FIXED_PORT_A })
            .await;

        assert_eq!(spawner.starts.load(Ordering::SeqCst), 1);
        match ei.current().await {
            RecordedStatus::Listening(url) => {
                assert!(url.starts_with("http://"), "got {url}");
            }
            other => panic!("expected Listening, got {:?}", other),
        }

        // cleanup —— 让 axum task 退出, 否则 test runtime 警告
        c.reconcile(LanListenerTarget::Disabled).await;
    }

    #[tokio::test]
    async fn apply_disabled_from_some_stops_listener() {
        let spawner = Arc::new(FakeSpawner::new());
        let (c, ei) = build(spawner.clone());

        c.reconcile(LanListenerTarget::Enabled { port: FIXED_PORT_A })
            .await;
        c.reconcile(LanListenerTarget::Disabled).await;

        assert_eq!(spawner.starts.load(Ordering::SeqCst), 1);
        assert_eq!(ei.current().await, RecordedStatus::Stopped);
    }

    #[tokio::test]
    async fn apply_same_port_is_noop() {
        let spawner = Arc::new(FakeSpawner::new());
        let (c, _ei) = build(spawner.clone());

        // 起一次固定端口 → 再 apply 同一个固定端口。controller 的 state
        // 字段存的是"请求的 port",所以 (Some(FIXED_PORT_A), Enabled{FIXED_PORT_A})
        // 必须命中 no-op 分支,FakeSpawner 不会被第二次调用。
        c.reconcile(LanListenerTarget::Enabled { port: FIXED_PORT_A })
            .await;
        c.reconcile(LanListenerTarget::Enabled { port: FIXED_PORT_A })
            .await;

        assert_eq!(
            spawner.starts.load(Ordering::SeqCst),
            1,
            "same-port apply must not re-spawn"
        );
        // 记下来的端口正是 FIXED_PORT_A,而不是 OS 分配的 ephemeral —— 这条
        // 断言钉死了 controller 内部 state 用"请求 port"做比对的契约。
        {
            let guard = c.state.lock().await;
            assert_eq!(guard.as_ref().expect("running").port, FIXED_PORT_A);
        }

        c.reconcile(LanListenerTarget::Disabled).await;
    }

    #[tokio::test]
    async fn apply_port_change_stops_then_starts() {
        let spawner = Arc::new(FakeSpawner::new());
        let (c, ei) = build(spawner.clone());

        // 起一次 FIXED_PORT_A
        c.reconcile(LanListenerTarget::Enabled { port: FIXED_PORT_A })
            .await;
        assert_eq!(spawner.starts.load(Ordering::SeqCst), 1);

        // 切换到 FIXED_PORT_B → controller 视角是不同 port,触发 stop + start
        c.reconcile(LanListenerTarget::Enabled { port: FIXED_PORT_B })
            .await;
        assert_eq!(
            spawner.starts.load(Ordering::SeqCst),
            2,
            "different-port apply must re-spawn once"
        );
        // 起来后 endpoint_info 还是 Listening(端口取决于 OS,只断言状态种类)
        assert!(matches!(ei.current().await, RecordedStatus::Listening(_)));

        c.reconcile(LanListenerTarget::Disabled).await;
    }

    #[tokio::test]
    async fn apply_bind_failure_keeps_state_none_and_sets_endpoint_error() {
        let spawner = Arc::new(FakeSpawner::new());
        spawner.arm_bind_failure(1); // 第一次 spawn 失败
        let (c, ei) = build(spawner.clone());

        c.reconcile(LanListenerTarget::Enabled { port: FIXED_PORT_A })
            .await;

        assert_eq!(spawner.starts.load(Ordering::SeqCst), 1);
        // state 保持 None(没 listener 跑着)
        {
            let guard = c.state.lock().await;
            assert!(guard.is_none(), "state must stay None after bind failure");
        }
        // endpoint_info 写了 BindFailed
        match ei.current().await {
            RecordedStatus::BindFailed(reason) => {
                assert!(reason.contains("simulated bind failure"));
            }
            other => panic!("expected BindFailed, got {:?}", other),
        }

        // 下一次 apply(Enabled) 不应被失败状态阻塞 —— controller 视角仍然
        // 是 (None, Enabled) 应当 spawn,本次 spawner 不再失败 → 成功
        c.reconcile(LanListenerTarget::Enabled { port: FIXED_PORT_A })
            .await;
        assert_eq!(spawner.starts.load(Ordering::SeqCst), 2);
        assert!(matches!(ei.current().await, RecordedStatus::Listening(_)));

        c.reconcile(LanListenerTarget::Disabled).await;
    }

    // ── initial_lan_target: 启动期 LAN 目标决策 (纯函数, run-mode 无关) ──
    //
    // 这组用例钉死 issue #899 的核心不变量:mobile_lan 手机网关在 daemon 启动期
    // 的开/关 **只看 settings**,跟运行模式 (含 ServerHeadless) 无关。函数签名
    // 不收 run_mode 入参,从结构上保证无头 server 与普通 daemon 起同一个网关。

    fn settings_view(
        enabled: bool,
        lan_listen_enabled: bool,
        lan_port: Option<u16>,
    ) -> MobileSyncSettingsSummary {
        MobileSyncSettingsSummary {
            enabled,
            lan_listen_enabled,
            lan_advertise_ip: None,
            lan_advertise_base_url: None,
            lan_port,
            lan_listener_error: None,
            shortcut_install_methods: Vec::new(),
        }
    }

    #[test]
    fn initial_lan_target_enabled_uses_configured_port() {
        let v = settings_view(true, true, Some(FIXED_PORT_B));
        assert_eq!(
            initial_lan_target(Some(&v)),
            LanListenerTarget::Enabled { port: FIXED_PORT_B }
        );
    }

    #[test]
    fn initial_lan_target_enabled_defaults_port_when_unset() {
        // lan_port 未设 → 取生产默认端口,而不是把网关留在 Disabled。
        let v = settings_view(true, true, None);
        assert_eq!(
            initial_lan_target(Some(&v)),
            LanListenerTarget::Enabled {
                port: DEFAULT_MOBILE_LAN_PORT
            }
        );
    }

    #[test]
    fn initial_lan_target_master_switch_off_is_disabled() {
        // 总开关关 —— 即便 LAN 子开关开着、端口也设了,也不起监听器。
        let v = settings_view(false, true, Some(FIXED_PORT_B));
        assert_eq!(initial_lan_target(Some(&v)), LanListenerTarget::Disabled);
    }

    #[test]
    fn initial_lan_target_lan_subswitch_off_is_disabled() {
        // 总开关开但 LAN 子开关关 → 不对外暴露 LAN 网关。
        let v = settings_view(true, false, Some(FIXED_PORT_B));
        assert_eq!(initial_lan_target(Some(&v)), LanListenerTarget::Disabled);
    }

    #[test]
    fn initial_lan_target_unreadable_settings_is_disabled() {
        // 启动期 settings 读取失败 (None) → 保守 Disabled,之后可经设置变更拉起。
        assert_eq!(initial_lan_target(None), LanListenerTarget::Disabled);
    }

    #[test]
    fn default_mobile_lan_port_matches_syncclipboard_convention() {
        // 既定默认端口,SyncClipboard 客户端默认指向它;改动会破坏既有手机配置。
        assert_eq!(DEFAULT_MOBILE_LAN_PORT, 42720);
    }
}
