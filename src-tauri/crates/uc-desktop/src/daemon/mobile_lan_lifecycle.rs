//! # 为什么需要这个模块
//!
//! 让用户切换"移动端同步"开关、改监听端口后**立即生效**,不再要求重启
//! daemon (历史上点一次 toggle = 弹"请重启"横幅 = 整个 GUI 进程重启,
//! 对首次添加移动设备的用户极不友好,详见 `.planning/quick/260511-mobile-sync-no-restart/findings.md`)。
//!
//! 关键约束:LAN 监听器是普通 axum HTTP server,与 iroh `BIND_LOCK`
//! (Pitfall 3 进程级单次 bind)毫无关系,本来就可以在同一个进程内反复
//! cancel + rebind。`uc-webserver/tests/graceful_shutdown_port_reuse.rs`
//! 早就为这一点钉死了契约。本模块就是把这个能力落地。
//!
//! # 对外能力
//!
//! [`MobileLanLifecycleController`] 实现 [`MobileLanLifecyclePort`] 的
//! `apply(target)` 状态对齐契约。调用方传"我希望监听器现在是什么状态"
//! (Disabled / Enabled{port}),controller 负责把当前实际状态推到那个值。
//!
//! daemon 启动期把这个 controller 同时装入两条链路:
//! 1. application 侧 [`MobileSyncFacade`] 持 `Arc<dyn MobileLanLifecyclePort>`,
//!    `update_settings` 写盘后立即调 `apply(target)`;
//! 2. daemon 自身在 `run()` 开始时调 `apply(initial_target)` 起初始监听器,
//!    退出时调 `apply(Disabled)` 兜底回收端口。
//!
//! # 内部实现要点
//!
//! - 状态机契约见 [`MobileLanLifecyclePort`] doc-comment。本 adapter 用
//!   `tokio::sync::Mutex<Option<RunningListener>>` 串行化所有 transition,
//!   保证并发 `apply` 不会出现"两个 listener 同时占同一端口"或"start
//!   半路被 stop 打断"。
//! - controller 不直接持有 [`MobileSyncFacade`] —— facade ↔ controller
//!   循环引用,装配期会陷入构造死锁。改通过 [`LanListenerSpawner`] 抽象,
//!   生产实现 [`AppFacadeListenerSpawner`] 从 [`AppFacade`] 的 mobile_sync
//!   OnceLock lazy 读取,装配期 facade 可以为空,首次 `apply` 时存在即可。
//! - bind 失败**不通过返回值上报**,而是写 [`InMemoryMobileSyncEndpointInfoAdapter`]
//!   的 `BindFailed{reason}` 三态。UI 通过同一 adapter 查询,避免"设置已落盘但
//!   返回值反悔"的语义割裂。

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

use uc_application::facade::{AppFacade, FileTransferFacade};
use uc_core::mobile_sync::LanEndpointInfo;
use uc_core::ports::{MobileLanLifecyclePort, MobileLanTarget};
use uc_infra::mobile_sync::InMemoryMobileSyncEndpointInfoAdapter;
use uc_webserver::mobile_lan::{start_mobile_lan_server, MobileLanServerHandle};

/// 当前运行中的 listener 引用 —— 用 cancel token 控制 graceful shutdown,
/// 用 join handle 等待 axum::serve 任务真正退出(否则下次 bind 同端口可能撞
/// TIME_WAIT 短暂占用)。
struct RunningListener {
    port: u16,
    cancel: CancellationToken,
    join: JoinHandle<anyhow::Result<()>>,
}

/// 如何起一个 LAN listener。抽出来一是为了单测 mock,二是为了把"读 facade"
/// 与"状态机推进"解耦 —— controller 不需要知道 facade 具体长什么样。
#[async_trait]
pub(crate) trait LanListenerSpawner: Send + Sync {
    async fn spawn(
        &self,
        bind: SocketAddr,
        cancel: CancellationToken,
    ) -> anyhow::Result<MobileLanServerHandle>;
}

/// 生产实现:从 [`AppFacade`] 的 mobile_sync OnceLock lazy 读取当前 facade,
/// 配合 file_transfer facade 调 [`start_mobile_lan_server`]。
pub(crate) struct AppFacadeListenerSpawner {
    app_facade: Arc<AppFacade>,
    file_transfer: Option<Arc<FileTransferFacade>>,
}

impl AppFacadeListenerSpawner {
    pub(crate) fn new(
        app_facade: Arc<AppFacade>,
        file_transfer: Option<Arc<FileTransferFacade>>,
    ) -> Self {
        Self {
            app_facade,
            file_transfer,
        }
    }
}

#[async_trait]
impl LanListenerSpawner for AppFacadeListenerSpawner {
    async fn spawn(
        &self,
        bind: SocketAddr,
        cancel: CancellationToken,
    ) -> anyhow::Result<MobileLanServerHandle> {
        let facade = self.app_facade.mobile_sync.get().cloned().ok_or_else(|| {
            anyhow::anyhow!("mobile_sync facade not installed; daemon lifecycle has not run yet")
        })?;
        start_mobile_lan_server(bind, cancel, facade, self.file_transfer.clone()).await
    }
}

pub struct MobileLanLifecycleController {
    endpoint_info: Arc<InMemoryMobileSyncEndpointInfoAdapter>,
    spawner: Arc<dyn LanListenerSpawner>,
    state: Mutex<Option<RunningListener>>,
}

impl MobileLanLifecycleController {
    pub(crate) fn new(
        endpoint_info: Arc<InMemoryMobileSyncEndpointInfoAdapter>,
        spawner: Arc<dyn LanListenerSpawner>,
    ) -> Self {
        Self {
            endpoint_info,
            spawner,
            state: Mutex::new(None),
        }
    }

    /// 内部:停止当前 listener(若有)。要在持锁状态下调用。
    async fn stop_locked(&self, guard: &mut tokio::sync::MutexGuard<'_, Option<RunningListener>>) {
        if let Some(running) = guard.take() {
            running.cancel.cancel();
            // 等 axum::serve 真正退出 —— 否则上层立刻调 start 同端口会撞瞬时占用。
            match running.join.await {
                Ok(Ok(())) => info!(port = running.port, "mobile LAN listener stopped"),
                Ok(Err(e)) => {
                    warn!(port = running.port, error = %e, "mobile LAN listener exited with error")
                }
                Err(join_err) => {
                    warn!(port = running.port, error = %join_err, "mobile LAN listener task join failed")
                }
            }
            self.endpoint_info.clear().await;
        }
    }

    /// 内部:在指定端口起 listener。要在持锁状态下调用。bind 失败把错误写
    /// endpoint_info 三态,state 保持 None,不返回错误(契约见 trait doc)。
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
                self.endpoint_info
                    .set(LanEndpointInfo { url: url.clone() })
                    .await;
                info!(url, "mobile LAN listener started");
                **guard = Some(RunningListener {
                    port,
                    cancel,
                    join: handle.join_handle,
                });
            }
            Err(e) => {
                let reason = format!("{}", e);
                self.endpoint_info.set_bind_failure(reason.clone()).await;
                error!(
                    bind = %bind,
                    error = %e,
                    "mobile LAN listener failed to bind"
                );
                // state 保持 None,下次 apply(Enabled) 可重试
            }
        }
    }
}

#[async_trait]
impl MobileLanLifecyclePort for MobileLanLifecycleController {
    async fn apply(&self, target: MobileLanTarget) {
        let mut guard = self.state.lock().await;
        let current_port = guard.as_ref().map(|r| r.port);
        match (current_port, target) {
            (None, MobileLanTarget::Disabled) => {
                // 已经没监听器,no-op
            }
            (Some(_), MobileLanTarget::Disabled) => {
                self.stop_locked(&mut guard).await;
            }
            (None, MobileLanTarget::Enabled { port }) => {
                self.start_locked(&mut guard, port).await;
            }
            (Some(p), MobileLanTarget::Enabled { port }) if p == port => {
                // 同端口,no-op
            }
            (Some(_), MobileLanTarget::Enabled { port }) => {
                self.stop_locked(&mut guard).await;
                self.start_locked(&mut guard, port).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use uc_core::mobile_sync::LanListenerStatus;
    use uc_core::ports::MobileSyncEndpointInfoPort;

    /// 测试用 spawner:起一个空 axum router 服务,可被 cancel。
    ///
    /// 测试既不依赖 uc-application MobileSyncFacade,也不引入跨 crate 依赖,
    /// 只验证 controller 的状态机 + endpoint_info 写入。
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
    ) -> (
        Arc<MobileLanLifecycleController>,
        Arc<InMemoryMobileSyncEndpointInfoAdapter>,
    ) {
        let endpoint_info = Arc::new(InMemoryMobileSyncEndpointInfoAdapter::new());
        let controller = Arc::new(MobileLanLifecycleController::new(
            endpoint_info.clone(),
            spawner,
        ));
        (controller, endpoint_info)
    }

    /// 测试用固定端口。FakeSpawner 已经不真实 bind, 这里随便选两个不冲突
    /// 的就行 —— 不会撞本机环境。选 42720 是因为它就是生产默认值,确保
    /// "同端口 no-op" 这条测试断言的语义就是生产意义上的"用户两次保存同
    /// 一个端口"。
    const FIXED_PORT_A: u16 = 42720;
    const FIXED_PORT_B: u16 = 51234;

    #[tokio::test]
    async fn apply_disabled_from_none_is_noop() {
        let spawner = Arc::new(FakeSpawner::new());
        let (c, ei) = build(spawner.clone());

        c.apply(MobileLanTarget::Disabled).await;

        assert_eq!(spawner.starts.load(Ordering::SeqCst), 0);
        assert_eq!(
            ei.current_status().await.unwrap(),
            LanListenerStatus::Stopped
        );
    }

    #[tokio::test]
    async fn apply_enabled_from_none_starts_listener() {
        let spawner = Arc::new(FakeSpawner::new());
        let (c, ei) = build(spawner.clone());

        c.apply(MobileLanTarget::Enabled { port: FIXED_PORT_A })
            .await;

        assert_eq!(spawner.starts.load(Ordering::SeqCst), 1);
        match ei.current_status().await.unwrap() {
            LanListenerStatus::Listening(ep) => {
                assert!(ep.url.starts_with("http://"), "got {}", ep.url);
            }
            other => panic!("expected Listening, got {:?}", other),
        }

        // cleanup —— 让 axum task 退出, 否则 test runtime 警告
        c.apply(MobileLanTarget::Disabled).await;
    }

    #[tokio::test]
    async fn apply_disabled_from_some_stops_listener() {
        let spawner = Arc::new(FakeSpawner::new());
        let (c, ei) = build(spawner.clone());

        c.apply(MobileLanTarget::Enabled { port: FIXED_PORT_A })
            .await;
        c.apply(MobileLanTarget::Disabled).await;

        assert_eq!(spawner.starts.load(Ordering::SeqCst), 1);
        assert_eq!(
            ei.current_status().await.unwrap(),
            LanListenerStatus::Stopped
        );
    }

    #[tokio::test]
    async fn apply_same_port_is_noop() {
        let spawner = Arc::new(FakeSpawner::new());
        let (c, _ei) = build(spawner.clone());

        // 起一次固定端口 → 再 apply 同一个固定端口。controller 的 state
        // 字段存的是"请求的 port",所以 (Some(FIXED_PORT_A), Enabled{FIXED_PORT_A})
        // 必须命中 no-op 分支,FakeSpawner 不会被第二次调用。
        c.apply(MobileLanTarget::Enabled { port: FIXED_PORT_A })
            .await;
        c.apply(MobileLanTarget::Enabled { port: FIXED_PORT_A })
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

        c.apply(MobileLanTarget::Disabled).await;
    }

    #[tokio::test]
    async fn apply_port_change_stops_then_starts() {
        let spawner = Arc::new(FakeSpawner::new());
        let (c, ei) = build(spawner.clone());

        // 起一次 FIXED_PORT_A
        c.apply(MobileLanTarget::Enabled { port: FIXED_PORT_A })
            .await;
        assert_eq!(spawner.starts.load(Ordering::SeqCst), 1);

        // 切换到 FIXED_PORT_B → controller 视角是不同 port,触发 stop + start
        c.apply(MobileLanTarget::Enabled { port: FIXED_PORT_B })
            .await;
        assert_eq!(
            spawner.starts.load(Ordering::SeqCst),
            2,
            "different-port apply must re-spawn once"
        );
        // 起来后 endpoint_info 还是 Listening(端口取决于 OS,只断言状态种类)
        assert!(matches!(
            ei.current_status().await.unwrap(),
            LanListenerStatus::Listening(_)
        ));

        c.apply(MobileLanTarget::Disabled).await;
    }

    #[tokio::test]
    async fn apply_bind_failure_keeps_state_none_and_sets_endpoint_error() {
        let spawner = Arc::new(FakeSpawner::new());
        spawner.arm_bind_failure(1); // 第一次 spawn 失败
        let (c, ei) = build(spawner.clone());

        c.apply(MobileLanTarget::Enabled { port: FIXED_PORT_A })
            .await;

        assert_eq!(spawner.starts.load(Ordering::SeqCst), 1);
        // state 保持 None(没 listener 跑着)
        {
            let guard = c.state.lock().await;
            assert!(guard.is_none(), "state must stay None after bind failure");
        }
        // endpoint_info 写了 BindFailed
        match ei.current_status().await.unwrap() {
            LanListenerStatus::BindFailed { reason } => {
                assert!(reason.contains("simulated bind failure"));
            }
            other => panic!("expected BindFailed, got {:?}", other),
        }

        // 下一次 apply(Enabled) 不应被失败状态阻塞 —— controller 视角仍然
        // 是 (None, Enabled) 应当 spawn,本次 spawner 不再失败 → 成功
        c.apply(MobileLanTarget::Enabled { port: FIXED_PORT_A })
            .await;
        assert_eq!(spawner.starts.load(Ordering::SeqCst), 2);
        assert!(matches!(
            ei.current_status().await.unwrap(),
            LanListenerStatus::Listening(_)
        ));

        c.apply(MobileLanTarget::Disabled).await;
    }
}
