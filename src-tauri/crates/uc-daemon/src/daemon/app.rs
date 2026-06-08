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
use uc_application::facade::{
    AppFacade, AppPaths, ClipboardHistoryFacade, HostEventBus, HostEventEmitterPort,
};
use uc_core::ports::MobileLanLifecyclePort;

use crate::daemon::peers::presence_monitor::PresenceMonitor;
use crate::daemon::service::DaemonService;
use crate::daemon::state::RuntimeState;
use uc_daemon_local::crash_marker::DaemonRunMarker;
use uc_daemon_local::process_metadata::{DaemonPidManager, DaemonProcessMode};
use uc_webserver::api::auth::load_or_create_auth_token;
use uc_webserver::api::event_emitter::DaemonApiEventEmitter;
use uc_webserver::api::server::{run_http_server, DaemonApiState};
use uc_webserver::api::setup_events::spawn_pairing_completion_forwarder;
use uc_webserver::api::types::{DaemonResidency, DaemonWsEvent};
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

/// Startup file-cache hygiene: reconcile stale DB entries against disk, then
/// TTL-clean expired cache files. Two passes back to back.
///
/// ADR-008 P3-3 B2': moved off the GUI (`uc_desktop::background::
/// start_file_cache_cleanup`) into the daemon, which owns the sqlite pool and
/// iroh-blobs actor. Both passes route through the entry-aware delete path
/// (untag iroh-blobs reference + remove cache file + drop sqlite rows).
///
/// 1. **Reconcile** (`reconcile_missing_files`): drop any DB entry whose
///    cache-managed `file://` path no longer exists on disk. Must run before
///    any service observes a hash whose `External` path may have vanished;
///    otherwise the iroh-blobs actor panics with "poisoned storage"
///    (bao_file.rs:410).
/// 2. **Cleanup** (`cleanup_expired_files`): walk the cache for files past
///    `file_sync.file_retention_hours` and remove them.
async fn run_startup_file_cache_hygiene(history_facade: Arc<ClipboardHistoryFacade>) {
    match history_facade.reconcile_missing_files().await {
        Ok(result) => {
            if result.entries_deleted > 0 || result.errors > 0 {
                info!(
                    entries_scanned = result.entries_scanned,
                    entries_deleted = result.entries_deleted,
                    errors = result.errors,
                    "Startup reconcile dropped stale entries with missing cache files"
                );
            }
        }
        Err(e) => {
            warn!(error = %e, "Startup reconcile failed (non-fatal)");
        }
    }

    match history_facade.cleanup_expired_files().await {
        Ok(result) => {
            if result.files_removed > 0 {
                info!(
                    files_removed = result.files_removed,
                    entries_deleted = result.entries_deleted,
                    orphans_removed = result.orphans_removed,
                    bytes_reclaimed = result.bytes_reclaimed,
                    errors = result.errors,
                    "Startup file cache cleanup completed"
                );
            }
        }
        Err(e) => {
            warn!(error = %e, "Startup file cache cleanup failed (non-fatal)");
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
    /// 现存所有 run-mode（`Standalone` / `ServerHeadless`）都是独立进程,
    /// 恒置 true——daemon 自己处理 OS 信号(ADR-008 P3-3 后不再有同进程 GUI)。
    listens_to_os_signals: bool,
    /// 写进 PID 文件的进程模式——决定 `cli stop` 能不能 SIGTERM 这个
    /// daemon。现存 run-mode 恒为 `Standalone`(可 SIGTERM);`InProcess` 仅作
    /// legacy PID 文件读取保留(ADR-008 P3-3)。
    process_mode: DaemonProcessMode,
    /// 在 health/status 握手里上报的 daemon 驻留模式（ADR-008 P5-L L1）。
    /// 从 `DaemonRunMode` 映射而来,透传进 `DaemonApiState`。默认
    /// `Standalone`,持久客户端后续据此识别 `Oneshot` 并接管(R8-F2)。
    residency: DaemonResidency,
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
    /// Analytics sink injected into `DaemonApiState` so `POST /analytics/capture`
    /// reports through the daemon — the single authoritative analytics sender
    /// (ADR-008 D20). `None` (test / GUI-only assembly) leaves the API state's
    /// no-op default in place.
    analytics: Option<Arc<dyn uc_observability::analytics::AnalyticsPort>>,
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
            residency: DaemonResidency::Standalone,
            mobile_lan_endpoint_info: None,
            mobile_lan_lifecycle: None,
            analytics: None,
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
            residency: DaemonResidency::Standalone,
            mobile_lan_endpoint_info: None,
            mobile_lan_lifecycle: None,
            analytics: None,
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

    /// Inject the analytics sink so the daemon's `POST /analytics/capture`
    /// endpoint reports through the single authoritative sender (ADR-008 D20).
    pub fn with_analytics(
        mut self,
        analytics: Arc<dyn uc_observability::analytics::AnalyticsPort>,
    ) -> Self {
        self.analytics = Some(analytics);
        self
    }

    /// Set the daemon residency mode reported in the health/status handshake
    /// (ADR-008 P5-L L1). Mapped from `DaemonRunMode` at the assembly boundary
    /// and forwarded into `DaemonApiState` so `GET /health` / `GET /status`
    /// surface it. Defaults to [`DaemonResidency::Standalone`] when not set.
    pub fn with_residency(mut self, residency: DaemonResidency) -> Self {
        self.residency = residency;
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
        // ADR-008 P5-0: DaemonPidManager now stores the resolved PID-file path
        // directly (the thin uc-daemon-process crate owns no app-stack deps).
        // `AppPaths::daemon_pid_path()` is `<app_data_root>/.daemon-pid`, the
        // exact path the manager used to compute internally from `AppPaths` —
        // byte-identical behavior.
        let pid_manager = DaemonPidManager::new(self.storage_paths.daemon_pid_path());
        let _pid_file_guard = DaemonPidFileGuard::activate(pid_manager.clone(), self.process_mode)?;
        let pid = std::process::id();

        // ADR-008 D17 / P4-5: reverse crash marker. Detect whether the previous
        // run died without a clean shutdown (its start marker survived), record
        // it for the GUI banner, then write this run's marker. Crash visibility
        // is best-effort — never fail boot over it.
        let run_marker = DaemonRunMarker::new(self.storage_paths.app_data_root_dir.clone());
        match run_marker.begin_run(pid) {
            Ok(Some(prev)) => warn!(
                prev_pid = prev.pid,
                prev_started_at_ms = prev.started_at_ms,
                "previous daemon run exited abnormally (no clean shutdown) — recorded for GUI"
            ),
            Ok(None) => {}
            Err(error) => warn!(error = %error, "failed to record daemon start marker"),
        }

        let presence_monitor = Arc::new(PresenceMonitor::new(
            Arc::clone(&self.app_facade),
            self.event_tx.clone(),
        ));

        // 2. Build security state and register daemon's own PID
        let security = Arc::new(SecurityState::new());
        security.register_pid(pid).await;

        // 3. Build API state using the shared event_tx (same channel used by all services)
        let mut api_state = DaemonApiState::new(Arc::clone(&self.app_facade), auth_token, security)
            .with_residency(self.residency);
        api_state.event_tx = self.event_tx.clone();
        let api_state = match &self.clipboard_capture_gate {
            Some(gate) => api_state.with_clipboard_gate(Arc::clone(gate)),
            None => api_state,
        };
        let api_state = match &self.deferred_ready_notify {
            Some(notify) => api_state.with_deferred_ready_notify(Arc::clone(notify)),
            None => api_state,
        };
        let api_state = match &self.analytics {
            Some(analytics) => api_state.with_analytics(Arc::clone(analytics)),
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

        // ADR-008 P5-L L4: Oneshot self-termination. For the Oneshot residency
        // ONLY, spawn the lease-draining supervisor and arm a terminate token;
        // for Standalone / ServerHeadless `oneshot_terminate` stays `None`, so
        // no supervisor is spawned and the run-loop arm below is wired to
        // `pending` — byte-for-byte unchanged behaviour. The registry clone is
        // captured AFTER `api_state` is fully built so it shares the SAME atomic
        // counter the WS handler increments, and BEFORE `api_state` is moved into
        // the HTTP server task below.
        let oneshot_terminate =
            (self.residency == DaemonResidency::Oneshot).then(CancellationToken::new);
        if let Some(token) = oneshot_terminate.clone() {
            let registry = api_state.lease_registry.clone();
            // ADR-008 P5-L L8c: hand the supervisor the restart coordinator (which
            // owns the L8b quiescing flag) so it reads quiescing through it and
            // aborts a timed-out drain via `coordinator.abort()` — clearing the
            // in-flight restart state + lowering quiescing atomically.
            let restart = api_state.restart.clone();
            let supervisor_shutdown = self.cancel.child_token();
            tokio::spawn(
                crate::daemon::oneshot::run_oneshot_self_terminate_supervisor(
                    registry,
                    restart,
                    token,
                    supervisor_shutdown,
                    crate::daemon::oneshot::SupervisorTimings::production(),
                ),
            );
        }

        // 6. Spawn HTTP server and rate limiter cleanup task
        let security_for_cleanup = api_state.security.clone();
        // ADR-008 P5-L L8c: capture the restart coordinator (Arc-backed) BEFORE
        // `api_state` is moved into the HTTP server task — it is read at
        // terminate time to decide whether to persist a handover record. Captured
        // UNCONDITIONALLY (not only for Oneshot): in every other residency
        // `pending()` is always `None`, so the handover-write block is a no-op.
        let restart_coordinator = api_state.restart.clone();
        let cleanup_cancel = self.cancel.child_token();
        let http_cancel = self.cancel.child_token();
        let mut http_handle = tokio::spawn(run_http_server(api_state, http_cancel));
        let mut http_handle_consumed = false;

        let _cleanup_handle = cleanup_rate_limiter_task(security_for_cleanup, cleanup_cancel);

        // ADR-008 P3-3 B2': startup file-cache hygiene runs in the daemon — the
        // process that owns the sqlite pool and the iroh-blobs actor — instead
        // of the GUI (which is becoming a pure client). Phase 1 reconcile must
        // flush `Complete{External(missing_path)}` entries before any service
        // observes their hash, else the iroh-blobs actor panics with "poisoned
        // storage" (bao_file.rs:410). Detached `tokio::spawn` (NOT a
        // `service_tasks` member) so its completion does not trip the
        // "service task exited unexpectedly" shutdown arm; this matches the
        // prior fire-and-forget GUI startup behavior.
        let history_facade_for_hygiene = Arc::clone(&self.app_facade.clipboard_history);
        let _hygiene_handle =
            tokio::spawn(run_startup_file_cache_hygiene(history_facade_for_hygiene));

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
                    info!("mobile_sync settings unreadable at daemon boot; LAN listener stays Disabled until next update_settings");
                }
            }
            controller.apply(target).await;
        }

        // Prepare deferred services start
        let mut deferred = std::mem::take(&mut self.deferred_services);
        let ready_notify = self.deferred_ready_notify.take();

        // 7. Wait for shutdown signal, infrastructure crash, service crash, or deferred start
        let listens_to_os_signals = self.listens_to_os_signals;
        // ADR-008 P5-L L8c: set only when the loop breaks via the Oneshot
        // self-terminate arm — the sole path on which a controlled-restart
        // handover record may be persisted (a signal/crash break leaves it false).
        let mut oneshot_self_terminated = false;
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
                // ADR-008 P5-L L4: Oneshot self-termination. `oneshot_terminate`
                // is `Some` ONLY in the Oneshot residency; for every other
                // residency it is `None`, so this arm awaits `pending` forever
                // and can never fire — preserving Standalone / ServerHeadless
                // behaviour byte-for-byte. When it does fire, the supervisor has
                // observed the control leases drain; break into the EXISTING
                // shutdown sequence below unchanged.
                _ = async {
                    match &oneshot_terminate {
                        Some(token) => token.cancelled().await,
                        None => std::future::pending::<()>().await,
                    }
                } => {
                    info!("oneshot residency: control leases drained — self-terminating");
                    // ADR-008 P5-L L8c: mark the supervisor-driven terminate so the
                    // post-loop handover-write block runs (only this path persists a
                    // record, and only when a restart is actually pending).
                    oneshot_self_terminated = true;
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

        // ADR-008 P5-L L8c: a controlled restart drains via the Oneshot supervisor
        // and then self-terminates. Only on that supervisor-driven terminate AND
        // with a pending restart do we persist the handover so the requester's
        // spawn launches the target mode. Written here — inside the instance-lock
        // window (host.rs holds it until after iroh unbinds) and before cancel — so
        // a successor never acquires the lock before the record exists. A normal
        // Oneshot self-terminate (no restart) has pending()==None -> no record; a
        // signal/crash break leaves the flag false.
        if oneshot_self_terminated {
            if let Some(req) = restart_coordinator.pending() {
                let record = uc_daemon_local::handover::HandoverRecord {
                    target_mode: crate::daemon::run_mode::residency_to_run_mode_env(req.target),
                    generation: req.generation,
                };
                if let Err(error) =
                    uc_daemon_local::handover::write(&self.storage_paths.app_data_root_dir, &record)
                {
                    warn!(error = %error, "failed to write controlled-restart handover record");
                } else {
                    info!(
                        target_mode = %record.target_mode,
                        generation = record.generation,
                        "wrote controlled-restart handover"
                    );
                }
            }
        }

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

        // ADR-008 D17 / P4-5: reached only on the graceful path (SIGKILL / OOM /
        // panic=abort never get here), so clearing the start marker here is what
        // distinguishes a clean shutdown from an abnormal exit on the next boot.
        if let Err(error) = run_marker.mark_clean_exit() {
            warn!(error = %error, "failed to clear daemon start marker on graceful shutdown");
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
