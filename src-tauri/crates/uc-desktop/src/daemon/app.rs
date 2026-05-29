//! # DaemonApp
//!
//! Top-level daemon lifecycle: starts the HTTP API server and services,
//! waits for shutdown signal, and tears down in reverse order.

use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::broadcast;
use tokio::sync::RwLock;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};
use uc_application::facade::{AppFacade, AppPaths, HostEventBus, HostEventEmitterPort};
use uc_core::ports::MobileLanLifecyclePort;

use crate::daemon::peers::presence_monitor::PresenceMonitor;
use crate::daemon::service::DaemonService;
use crate::daemon::state::RuntimeState;
use uc_daemon_local::process_metadata::{DaemonPidManager, DaemonProcessMode};
use uc_webserver::api::auth::load_or_create_auth_token;
use uc_webserver::api::event_emitter::DaemonApiEventEmitter;
use uc_webserver::api::server::{run_http_server, DaemonApiState};
use uc_webserver::api::setup_events::spawn_pairing_completion_forwarder;
use uc_webserver::api::types::DaemonWsEvent;
use uc_webserver::security::{cleanup_rate_limiter_task, SecurityState};

/// Recover encryption session from disk/keyring if encryption has been initialized.
///
/// # Parameters
///
/// - `app_facade`: Application facade — entry point for all encryption ops
/// - `auto_unlock_enabled`: Whether to attempt automatic unlock via keyring
///
/// # Returns
///
/// - `Ok(true)`: Session was successfully unlocked
/// - `Ok(false)`: Session was NOT unlocked — either encryption is uninitialized,
///   or `auto_unlock_enabled` is false while encryption is initialized
/// - `Err`: Unlock failed (daemon must not start)
///
/// `pub` so the daemon background unlock task can call it BEFORE
/// constructing `DaemonApp`, using the result to decide whether to start
/// `PeerDiscoveryWorker` immediately or defer.
pub async fn recover_encryption_session(
    app_facade: &AppFacade,
    auto_unlock_enabled: bool,
) -> anyhow::Result<bool> {
    // Phase C: setup completion truth source is `EncryptionStateView.initialized`,
    // which is backed by `SetupStatus.has_completed`.
    let setup_completed = app_facade
        .encryption
        .state()
        .await
        .map(|state| state.initialized)
        .map_err(|e| anyhow::anyhow!("failed to load encryption state: {}", e))?;

    if !auto_unlock_enabled {
        if setup_completed {
            info!("Auto-unlock disabled via settings — skipping encryption session recovery");
        } else {
            info!("Setup not completed, skipping session recovery");
        }
        return Ok(false);
    }

    match app_facade.encryption.unlock().await {
        Ok(true) => {
            info!("Encryption session recovered from disk");
            Ok(true)
        }
        Ok(false) => {
            info!("Encryption not initialized, skipping session recovery");
            Ok(false)
        }
        Err(e) => {
            error!(error = %e, "Encryption session recovery failed");
            anyhow::bail!(
                "Cannot start daemon: encryption session recovery failed: {}",
                e
            )
        }
    }
}

/// Main daemon application.
///
/// Owns the service list and cancellation token.
/// Services use `Arc<dyn DaemonService>` (not `Box`) to allow cloning
/// for `tokio::spawn` `'static` requirement.
pub struct DaemonApp {
    services: Vec<Arc<dyn DaemonService>>,
    /// Application-layer entry point. All business calls go through here;
    /// daemon never reaches into uc-application internals.
    app_facade: Arc<AppFacade>,
    /// Process-level filesystem layout (token path, db path, cache dir, etc.)
    /// — NOT business state, lives on daemon.
    storage_paths: AppPaths,
    /// Shared host-event bus. Bootstrap creates the bus and shares it with
    /// downstream consumers; daemon registers its `DaemonApiEventEmitter`
    /// on the bus once `event_tx` is ready. Registration is additive —
    /// emitters already attached by other call sites (Tauri webview,
    /// logging) keep receiving events without coordination.
    host_event_bus: Arc<HostEventBus>,
    state: Arc<RwLock<RuntimeState>>,
    event_tx: broadcast::Sender<DaemonWsEvent>,
    cancel: CancellationToken,
    deferred_services: Vec<Arc<dyn DaemonService>>,
    deferred_ready_notify: Option<Arc<tokio::sync::Notify>>,
    external_shutdown: Option<CancellationToken>,
    clipboard_capture_gate: Option<Arc<AtomicBool>>,
    /// Local device id (sponsor view) baked in at construction so the
    /// pairing-completion forwarder doesn't need to pull
    /// `DeviceIdentityPort` at event time. Pre-resolved in `entrypoint`.
    local_device_id: Option<String>,
    /// 是否在 main loop 里监听 OS 信号（SIGTERM/SIGINT/Ctrl-C）。
    ///
    /// `Standalone` 置 true；`GuiInProcess` 置 false——OS 信号属于 GUI 的
    /// 责任，daemon 不能抢占 handler，shutdown 必须通过 caller 持有的
    /// cancel token 触发。
    listens_to_os_signals: bool,
    /// 写进 PID 文件的进程模式——决定 `cli stop` 能不能 SIGTERM 这个
    /// daemon。`GuiInProcess` → `InProcess`；`Standalone` → `Standalone`。
    process_mode: DaemonProcessMode,
    /// Mobile sync LAN endpoint adapter 的具体类型,daemon 启动时用它 spawn
    /// `mobile_lan` listener,起来后 `set` 当前 URL,关闭后 `clear`。
    /// `None` 表示该装配场景不接 mobile listener(测试或未来 GUI-only 模式)。
    mobile_lan_endpoint_info:
        Option<Arc<uc_infra::mobile_sync::InMemoryMobileSyncEndpointInfoAdapter>>,
    /// 移动同步 LAN listener 生命周期控制器。`Some(...)` 时 daemon `run()`
    /// 启动期把 listener 对齐到 settings, 退出期 `apply(Disabled)` 回收端口;
    /// `update_settings` 写盘后也通过这一份 controller 即时切换 —— 两条链路
    /// 共用单点状态机, 不再要求重启 daemon。`None` 表示该装配场景不需要
    /// listener (测试 / 未来 GUI-only 路径)。
    mobile_lan_lifecycle:
        Option<Arc<crate::daemon::mobile_lan_lifecycle::MobileLanLifecycleController>>,
}

impl DaemonApp {
    /// Create a new DaemonApp with the given services.
    #[allow(dead_code)]
    pub fn new(
        services: Vec<Arc<dyn DaemonService>>,
        app_facade: Arc<AppFacade>,
        storage_paths: AppPaths,
        host_event_bus: Arc<HostEventBus>,
        state: Arc<RwLock<RuntimeState>>,
        event_tx: broadcast::Sender<DaemonWsEvent>,
    ) -> Self {
        Self {
            services,
            app_facade,
            storage_paths,
            host_event_bus,
            state,
            event_tx,
            cancel: CancellationToken::new(),
            deferred_services: Vec::new(),
            deferred_ready_notify: None,
            external_shutdown: None,
            clipboard_capture_gate: None,
            local_device_id: None,
            listens_to_os_signals: true,
            process_mode: DaemonProcessMode::Standalone,
            mobile_lan_endpoint_info: None,
            mobile_lan_lifecycle: None,
        }
    }

    /// Construct a DaemonApp with deferred services support.
    ///
    /// Services in `deferred_services` will only start when `deferred_ready_notify` fires.
    #[allow(clippy::too_many_arguments)]
    pub fn new_with_deferred(
        services: Vec<Arc<dyn DaemonService>>,
        app_facade: Arc<AppFacade>,
        storage_paths: AppPaths,
        host_event_bus: Arc<HostEventBus>,
        state: Arc<RwLock<RuntimeState>>,
        event_tx: broadcast::Sender<DaemonWsEvent>,
        _encryption_unlocked: bool,
        deferred_services: Vec<Arc<dyn DaemonService>>,
        deferred_ready_notify: Option<Arc<tokio::sync::Notify>>,
        external_shutdown: Option<CancellationToken>,
        clipboard_capture_gate: Option<Arc<AtomicBool>>,
        local_device_id: Option<String>,
        listens_to_os_signals: bool,
        process_mode: DaemonProcessMode,
    ) -> Self {
        debug_assert!(
            deferred_services.is_empty() || deferred_ready_notify.is_some(),
            "deferred_services is non-empty but deferred_ready_notify is None — services would never start"
        );
        // Slice4 P3 T3.3 invariant: when SpaceSetupFacade is wired in,
        // sponsor device id must be too — the pairing-completion
        // forwarder needs both to emit `setup.pairingCompleted`.
        debug_assert!(
            app_facade.space_setup.get().is_some() == local_device_id.is_some(),
            "space_setup facade and local_device_id must be wired together"
        );
        Self {
            services,
            app_facade,
            storage_paths,
            host_event_bus,
            state,
            event_tx,
            cancel: CancellationToken::new(),
            deferred_services,
            deferred_ready_notify,
            external_shutdown,
            clipboard_capture_gate,
            local_device_id,
            listens_to_os_signals,
            process_mode,
            mobile_lan_endpoint_info: None,
            mobile_lan_lifecycle: None,
        }
    }

    /// 注入 mobile sync LAN endpoint adapter。daemon `run()` 看到 `Some(...)`
    /// 且 `MobileSyncSettings.enabled && lan_listen_enabled` 时会 spawn
    /// `mobile_lan` listener, 始终绑 `0.0.0.0:lan_port`(默认 42720),
    /// 起来后写 `endpoint_info.set(...)`, 关闭后 `clear()`。`None`(默认)
    /// 表示当前装配场景不接 listener (测试 / 未来 GUI-only 路径)。
    /// `lan_advertise_ip` 由 application 层 register_device 直接读 settings
    /// 决定二维码 URL, 不在本侧使用。
    pub fn with_mobile_lan_endpoint_info(
        mut self,
        endpoint_info: Arc<uc_infra::mobile_sync::InMemoryMobileSyncEndpointInfoAdapter>,
    ) -> Self {
        self.mobile_lan_endpoint_info = Some(endpoint_info);
        self
    }

    /// 注入 mobile sync LAN lifecycle controller。daemon `run()` 启动期调
    /// `apply(initial_target)` 起 listener,退出期 `apply(Disabled)` 回收端口。
    /// 与 [`Self::with_mobile_lan_endpoint_info`] 共享同一份 endpoint_info ——
    /// controller 内部写 endpoint_info, 进程内其它读者(GUI command 等)从同一
    /// Arc 看到。`None`(默认)表示该装配场景不接 listener。
    pub fn with_mobile_lan_lifecycle(
        mut self,
        controller: Arc<crate::daemon::mobile_lan_lifecycle::MobileLanLifecycleController>,
    ) -> Self {
        self.mobile_lan_lifecycle = Some(controller);
        self
    }

    /// Run the daemon: start the HTTP API server and services, wait for shutdown, cleanup.
    ///
    /// NOTE: `recover_encryption_session` is called in the entrypoint background
    /// task BEFORE constructing `DaemonApp`.
    pub async fn run(mut self) -> anyhow::Result<()> {
        info!("uniclipboard-daemon starting");

        // 1. Load or create auth token (stored alongside PID metadata)
        let token_path = self.storage_paths.daemon_token_path();
        debug!(
            token_path = %token_path.display(),
            "loading daemon auth token"
        );
        let auth_token = load_or_create_auth_token(&token_path)?;
        let pid_manager = DaemonPidManager::new(self.storage_paths.clone());
        let _pid_file_guard = DaemonPidFileGuard::activate(pid_manager.clone(), self.process_mode)?;
        let pid = std::process::id();

        let presence_monitor = Arc::new(PresenceMonitor::new(
            Arc::clone(&self.app_facade),
            self.event_tx.clone(),
        ));

        // 2. Build security state and register daemon's own PID
        let security = Arc::new(SecurityState::new());
        security.register_pid(pid).await;

        // 3. Build API state using the shared event_tx (same channel used by all services)
        let mut api_state = DaemonApiState::new(Arc::clone(&self.app_facade), auth_token, security);
        api_state.event_tx = self.event_tx.clone();
        let api_state = match &self.clipboard_capture_gate {
            Some(gate) => api_state.with_clipboard_gate(Arc::clone(gate)),
            None => api_state,
        };
        let api_state = match &self.deferred_ready_notify {
            Some(notify) => api_state.with_deferred_ready_notify(Arc::clone(notify)),
            None => api_state,
        };
        // 4. Register the daemon's WS emitter on the shared host-event bus
        // so application use cases (which fan out through the bus) push WS
        // events to LAN clients.
        //
        // `register` is additive — emitters registered earlier (logging,
        // Tauri webview if running in-process) keep receiving events. The
        // `"daemon_ws"` name is the unregistration handle: a future daemon
        // reload can pull this exact emitter off the bus without disturbing
        // the GUI side.
        self.host_event_bus.register(
            "daemon_ws",
            Arc::new(DaemonApiEventEmitter::new(self.event_tx.clone()))
                as Arc<dyn HostEventEmitterPort>,
        );

        info!("uniclipboard-daemon running");

        // 5. Start ALL services uniformly via JoinSet
        let mut service_tasks = JoinSet::new();
        self.services
            .push(Arc::clone(&presence_monitor) as Arc<dyn DaemonService>);
        for service in &self.services {
            let svc = Arc::clone(service);
            let token = self.cancel.child_token();
            service_tasks.spawn(async move { svc.start(token).await });
        }

        // Slice4 P3 T3.3: spawn the sponsor-side pairing-completion forwarder
        // before the HTTP server. Subscribes to the facade's broadcast stream
        // and translates each `PairingOutcome` into a `setup.pairingCompleted`
        // ws frame on the shared event bus.
        if let (Some(facade), Some(sponsor_id)) = (
            self.app_facade.space_setup.get().cloned(),
            self.local_device_id.as_ref(),
        ) {
            spawn_pairing_completion_forwarder(
                facade.subscribe_pairing_completion(),
                self.event_tx.clone(),
                sponsor_id.clone(),
                self.cancel.child_token(),
            );
        }

        // 6. Spawn HTTP server and rate limiter cleanup task
        let security_for_cleanup = api_state.security.clone();
        let cleanup_cancel = self.cancel.child_token();
        let http_cancel = self.cancel.child_token();
        let mut http_handle = tokio::spawn(run_http_server(api_state, http_cancel));
        let mut http_handle_consumed = false;

        let _cleanup_handle = cleanup_rate_limiter_task(security_for_cleanup, cleanup_cancel);

        // mobile_sync LAN listener:经 controller 走"对齐到期望状态"的路径,
        // 不再一次性 tokio::spawn。同一个 controller 也注入了 MobileSyncFacade,
        // update_settings 写盘后通过 apply(target) 立即切换 listener,无需重启。
        //
        // 装配条件:必须同时有 mobile_lan_lifecycle controller 与 mobile_sync
        // facade。任一缺失就当未装配场景(测试 / GUI-only 路径),跳过 listener。
        if let (Some(controller), Some(mobile_sync_facade)) = (
            self.mobile_lan_lifecycle.clone(),
            self.app_facade.mobile_sync.get().cloned(),
        ) {
            // 读一次 settings 决定**初始**目标状态。此后任何变更都走
            // MobileSyncFacade::update_settings → controller.apply(target)
            // 即时路径,不再依赖 daemon 启动。
            let settings_view = mobile_sync_facade.get_settings().await.ok();
            // 启动期 LAN 目标只由 settings 决定,与 run mode 无关 —— 无头 server
            // 节点 (ServerHeadless) 跟普通 daemon 起同一个手机网关。决策抽到纯函数
            // initial_lan_target (无 run_mode 入参) 钉死这条不变量 (issue #899)。
            let target =
                crate::daemon::mobile_lan_lifecycle::initial_lan_target(settings_view.as_ref());
            // Log the boot-time decision exhaustively. The Enabled arm is the
            // operator's only confirmation that the phone gateway came up —
            // a ServerHeadless node has no GUI surfacing the listener state.
            match (&settings_view, &target) {
                (_, uc_core::ports::MobileLanTarget::Enabled { port }) => {
                    info!(
                        port = *port,
                        "mobile_sync LAN listener enabled by settings; starting at daemon boot"
                    );
                }
                (Some(v), uc_core::ports::MobileLanTarget::Disabled) => {
                    info!(
                        enabled = v.enabled,
                        lan_listen_enabled = v.lan_listen_enabled,
                        "mobile_sync LAN listener disabled by settings; not starting at daemon boot"
                    );
                }
                (None, uc_core::ports::MobileLanTarget::Disabled) => {
                    warn!("mobile_sync settings unreadable at daemon boot; LAN listener stays Disabled until next update_settings");
                }
            }
            controller.apply(target).await;
        }

        // Prepare deferred services start
        let mut deferred = std::mem::take(&mut self.deferred_services);
        let ready_notify = self.deferred_ready_notify.take();

        // 7. Wait for shutdown signal, infrastructure crash, service crash, or deferred start
        let listens_to_os_signals = self.listens_to_os_signals;
        loop {
            tokio::select! {
                _ = async {
                    if listens_to_os_signals {
                        if let Err(error) = wait_for_shutdown_signal().await {
                            warn!(error = %error, "shutdown signal handler error");
                        }
                    } else {
                        std::future::pending::<()>().await;
                    }
                } => {
                    info!("shutdown signal received");
                    break;
                }
                _ = async {
                    match &self.external_shutdown {
                        Some(token) => token.cancelled().await,
                        None => std::future::pending::<()>().await,
                    }
                } => {
                    info!("external shutdown signal received (parent process gone)");
                    break;
                }
                result = &mut http_handle => {
                    warn!("HTTP server exited unexpectedly: {:?}", result);
                    // JoinHandle 在 select! 这一支已经被 poll 到 Ready 并通过
                    // take_output 消费掉结果。后续 shutdown 阶段不能再 await
                    // 同一个 handle，否则触发
                    // panic!("JoinHandle polled after completion")。
                    http_handle_consumed = true;
                    break;
                }
                Some(result) = service_tasks.join_next() => {
                    warn!("service task exited unexpectedly: {:?}", result);
                    break;
                }
                _ = async {
                    match &ready_notify {
                        Some(n) => n.notified().await,
                        None => std::future::pending::<()>().await,
                    }
                }, if !deferred.is_empty() => {
                    info!(
                        count = deferred.len(),
                        "ready signal received — starting deferred services"
                    );
                    for worker in deferred.drain(..) {
                        let name = worker.name().to_string();
                        info!(service = %name, "starting deferred service");
                        let worker_for_shutdown: Arc<dyn DaemonService> = Arc::clone(&worker);
                        let token = self.cancel.child_token();
                        service_tasks.spawn(async move { worker.start(token).await });
                        self.services.push(worker_for_shutdown);
                        {
                            let mut state = self.state.write().await;
                            state.update_service_health(&name, crate::daemon::service::ServiceHealth::Healthy);
                        }
                    }
                }
            }
        }

        // 8. Shutdown sequence
        info!("shutting down...");
        self.cancel.cancel();

        // mobile_sync LAN listener 不挂在主 cancel token 上(它有自己的子
        // token 由 controller 管),必须**显式**调 apply(Disabled) 才能 stop
        // listener + drop bound listener + 写 endpoint_info.clear()。否则
        // 主进程退出 / daemon binary 启动新进程时端口仍被占。
        if let Some(controller) = self.mobile_lan_lifecycle.as_ref() {
            controller
                .apply(uc_core::ports::MobileLanTarget::Disabled)
                .await;
        }

        tokio::time::timeout(Duration::from_secs(5), async {
            while service_tasks.join_next().await.is_some() {}
        })
        .await
        .ok();

        if !http_handle_consumed {
            tokio::time::timeout(Duration::from_secs(5), http_handle)
                .await
                .ok();
        }

        for service in self.services.iter().rev() {
            info!(service = service.name(), "stopping service");
            if let Err(e) = service.stop().await {
                warn!(service = service.name(), "error stopping service: {}", e);
            }
        }

        info!("uniclipboard-daemon stopped");
        Ok(())
    }
}

struct DaemonPidFileGuard {
    manager: DaemonPidManager,
}

impl DaemonPidFileGuard {
    fn activate(manager: DaemonPidManager, mode: DaemonProcessMode) -> anyhow::Result<Self> {
        let pid = manager.write_current_pid_with_mode(mode)?;
        info!(pid, ?mode, "wrote daemon pid metadata");
        Ok(Self { manager })
    }
}

impl Drop for DaemonPidFileGuard {
    fn drop(&mut self) {
        if let Err(error) = self.manager.remove_pid_file() {
            warn!(error = %error, "failed to remove daemon pid metadata");
        }
    }
}

/// Wait for either Ctrl-C or SIGTERM (Unix).
async fn wait_for_shutdown_signal() -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut sigterm = signal(SignalKind::terminate())
            .map_err(|e| anyhow::anyhow!("failed to register SIGTERM handler: {}", e))?;
        tokio::select! {
            result = tokio::signal::ctrl_c() => {
                result.map_err(|e| anyhow::anyhow!("ctrl_c handler error: {}", e))?;
            }
            _ = sigterm.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c()
            .await
            .map_err(|e| anyhow::anyhow!("ctrl_c handler error: {}", e))?;
    }
    Ok(())
}
