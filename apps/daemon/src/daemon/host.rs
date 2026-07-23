//! Standalone daemon host for the shared engine.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};
use uc_bootstrap::{
    init_tracing_subscriber, install_panic_logging_hook, prepare_desktop_engine_host,
    DesktopHostFileHandles, DesktopHostProcessPaths,
};
use uc_daemon_local::crash_marker::{DaemonExitReport, DaemonRunMarker};
use uc_daemon_local::process_metadata::{DaemonPidManager, DaemonProcessMode};
use uc_engine::{ActiveClipboardChanged, Engine, HostFileHandle, Operation, OperationResult};
use uc_webserver::api::auth::load_or_create_auth_token;
use uc_webserver::api::server::{run_http_server, DaemonApiState, DaemonFileHandles};
use uc_webserver::api::types::{DaemonResidency, DaemonWsEvent};
use uc_webserver::security::{cleanup_rate_limiter_task, SecurityState};

use super::engine_events::forward_engine_events;
use super::mobile_lan_lifecycle::{initial_lan_target, MobileLanLifecycleController};
use super::run_mode::DaemonRunMode;
use super::startup_recovery::{record_upgrade_status_at_startup, spawn_startup_recovery};
use super::tokio_runtime::build_daemon_tokio_runtime;

const ENGINE_SHUTDOWN_DEADLINE: Duration = Duration::from_secs(15);

struct DesktopDaemonFileHandles {
    handles: DesktopHostFileHandles,
}

impl DesktopDaemonFileHandles {
    fn new(handles: DesktopHostFileHandles) -> Self {
        Self { handles }
    }
}

impl DaemonFileHandles for DesktopDaemonFileHandles {
    fn register_input(&self, path: &Path) -> anyhow::Result<HostFileHandle> {
        self.handles
            .register_input(path.to_path_buf())
            .map_err(anyhow::Error::new)
    }

    fn register_output(&self, path: &Path) -> anyhow::Result<HostFileHandle> {
        self.handles
            .register_output(path.to_path_buf())
            .map_err(anyhow::Error::new)
    }

    fn register_diagnostic_output(&self) -> anyhow::Result<(HostFileHandle, String)> {
        let directory = dirs::download_dir()
            .ok_or_else(|| anyhow::anyhow!("Downloads directory is unavailable"))?;
        std::fs::create_dir_all(&directory)?;
        let filename = format!(
            "uniclipboard-diagnostics-{}.zip",
            chrono::Utc::now().format("%Y%m%d-%H%M%S")
        );
        let path = directory.join(filename);
        let handle = self
            .handles
            .register_output(path.clone())
            .map_err(anyhow::Error::new)?;
        Ok((handle, path.to_string_lossy().into_owned()))
    }
}

/// Standalone daemon binary entry: start one shared engine and block until exit.
pub fn run(run_mode: DaemonRunMode) -> anyhow::Result<()> {
    let runtime = build_daemon_tokio_runtime()?;
    runtime.block_on(run_async(run_mode))
}

async fn run_async(run_mode: DaemonRunMode) -> anyhow::Result<()> {
    init_tracing_subscriber()?;
    install_panic_logging_hook();

    let prepared = prepare_desktop_engine_host()?;
    let process_paths = prepared.process_paths().clone();
    let file_handles: Arc<dyn DaemonFileHandles> =
        Arc::new(DesktopDaemonFileHandles::new(prepared.file_handles()));
    let (engine_config, host_capabilities) = prepared.into_engine_start();

    let _instance_lock = uc_daemon_local::instance_lock::acquire_with_deadline(
        process_paths.app_data_root(),
        uc_daemon_local::timing::LOCK_ACQUIRE_DEADLINE,
    )
    .await
    .map_err(|error| anyhow::anyhow!("{error}"))?;
    uc_daemon_local::handover::clear(process_paths.app_data_root());

    let (engine, events) = Engine::start(engine_config, host_capabilities)
        .await
        .map_err(anyhow::Error::new)?;
    let engine = Arc::new(engine);

    record_upgrade_status_at_startup(&engine).await;
    spawn_startup_recovery(run_mode, Arc::clone(&engine));

    let result = run_daemon_surfaces(
        run_mode,
        Arc::clone(&engine),
        events,
        file_handles,
        process_paths,
    )
    .await;
    let shutdown = engine
        .shutdown(ENGINE_SHUTDOWN_DEADLINE)
        .await
        .map_err(anyhow::Error::new);

    result.and(shutdown)
}

async fn run_daemon_surfaces(
    run_mode: DaemonRunMode,
    engine: Arc<Engine>,
    events: uc_engine::EventStream,
    file_handles: Arc<dyn DaemonFileHandles>,
    process_paths: DesktopHostProcessPaths,
) -> anyhow::Result<()> {
    let auth_token = load_or_create_auth_token(process_paths.daemon_token())?;
    let pid_manager = DaemonPidManager::new(process_paths.daemon_pid());
    let _pid_file_guard = DaemonPidFileGuard::activate(pid_manager, run_mode.process_mode())?;
    let pid = std::process::id();
    let run_marker = DaemonRunMarker::new(process_paths.app_data_root().to_path_buf());
    log_previous_crash(run_marker.begin_run(pid)?);

    let security = Arc::new(SecurityState::new());
    security.register_pid(pid).await;
    let mut api_state = DaemonApiState::new(
        Arc::clone(&engine),
        file_handles,
        auth_token,
        Arc::clone(&security),
    )
    .with_residency(run_mode.into());
    let (event_tx, _) = broadcast::channel::<DaemonWsEvent>(64);
    api_state.event_tx = event_tx.clone();

    let restart = api_state.restart.clone();
    let lease_registry = api_state.lease_registry.clone();
    let cancel = CancellationToken::new();
    let http_cancel = cancel.child_token();
    let cleanup_cancel = cancel.child_token();
    let event_cancel = cancel.child_token();
    let mut http_handle = tokio::spawn(run_http_server(api_state, http_cancel));
    let _cleanup_handle = cleanup_rate_limiter_task(security, cleanup_cancel);

    let (active_clipboard_tx, _) = broadcast::channel::<ActiveClipboardChanged>(64);
    let mobile_lan = Arc::new(MobileLanLifecycleController::for_engine(
        Arc::clone(&engine),
        active_clipboard_tx.clone(),
    ));
    let event_forwarder = tokio::spawn(forward_engine_events(
        events,
        Arc::clone(&engine),
        event_tx,
        active_clipboard_tx.clone(),
        Arc::clone(&mobile_lan),
        event_cancel,
    ));
    apply_initial_mobile_lan_target(&engine, mobile_lan.as_ref()).await;

    let residency: DaemonResidency = run_mode.into();
    let oneshot_terminate = (residency == DaemonResidency::Oneshot).then(CancellationToken::new);
    if let Some(token) = oneshot_terminate.clone() {
        tokio::spawn(super::oneshot::run_oneshot_self_terminate_supervisor(
            lease_registry,
            restart.clone(),
            token,
            cancel.child_token(),
            super::oneshot::SupervisorTimings::production(),
        ));
    }

    info!(?residency, "uniclipboard daemon running on shared engine");
    let mut http_completed = false;
    let mut controlled_oneshot_exit = false;
    tokio::select! {
        result = wait_for_shutdown_signal(), if run_mode.listens_to_os_signals() => {
            result?;
            info!("shutdown signal received");
        }
        result = &mut http_handle => {
            http_completed = true;
            warn!(?result, "HTTP server exited unexpectedly");
        }
        _ = async {
            match &oneshot_terminate {
                Some(token) => token.cancelled().await,
                None => std::future::pending::<()>().await,
            }
        } => {
            controlled_oneshot_exit = true;
            info!("oneshot control leases drained");
        }
    }

    if controlled_oneshot_exit {
        write_handover_if_requested(&restart, process_paths.app_data_root());
    }
    cancel.cancel();
    mobile_lan.disable().await;
    if !http_completed {
        let _ =
            tokio::time::timeout(uc_daemon_local::timing::SHUTDOWN_JOIN_TIMEOUT, http_handle).await;
    }
    let _ = tokio::time::timeout(
        uc_daemon_local::timing::SHUTDOWN_JOIN_TIMEOUT,
        event_forwarder,
    )
    .await;
    run_marker.mark_clean_exit()?;
    Ok(())
}

async fn apply_initial_mobile_lan_target(
    engine: &Engine,
    controller: &MobileLanLifecycleController,
) {
    let settings = match engine.execute(Operation::QueryMobileSyncSettings).await {
        Ok(OperationResult::MobileSyncSettings(settings)) => Some(settings),
        Ok(_) | Err(_) => None,
    };
    controller
        .reconcile(initial_lan_target(settings.as_deref()))
        .await;
}

fn log_previous_crash(previous: Option<DaemonExitReport>) {
    if let Some(previous) = previous {
        warn!(
            prev_pid = previous.pid,
            prev_started_at_ms = previous.started_at_ms,
            "previous daemon run exited abnormally"
        );
    }
}

fn write_handover_if_requested(
    restart: &uc_webserver::api::restart::RestartCoordinator,
    data_root: &Path,
) {
    let Some(request) = restart.pending() else {
        return;
    };
    let record = uc_daemon_local::handover::HandoverRecord {
        target_mode: super::run_mode::residency_to_run_mode_env(request.target),
        generation: request.generation,
    };
    if let Err(error) = uc_daemon_local::handover::write(data_root, &record) {
        warn!(error = %error, "failed to write controlled-restart handover record");
    }
}

struct DaemonPidFileGuard {
    manager: DaemonPidManager,
}

impl DaemonPidFileGuard {
    fn activate(manager: DaemonPidManager, mode: DaemonProcessMode) -> anyhow::Result<Self> {
        manager.write_current_pid_with_mode(mode)?;
        Ok(Self { manager })
    }
}

impl Drop for DaemonPidFileGuard {
    fn drop(&mut self) {
        if let Err(error) = self.manager.remove_pid_file() {
            warn!(error = %error, "failed to remove daemon PID metadata");
        }
    }
}

async fn wait_for_shutdown_signal() -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut sigterm = signal(SignalKind::terminate())?;
        tokio::select! {
            result = tokio::signal::ctrl_c() => result?,
            _ = sigterm.recv() => {}
        }
    }
    #[cfg(not(unix))]
    tokio::signal::ctrl_c().await?;
    Ok(())
}

pub use uc_daemon_local::spawn_contract::RUN_MODE_ENV;
pub use uc_daemon_local::spawn_contract::RUN_MODE_ONESHOT;
pub use uc_daemon_local::spawn_contract::RUN_MODE_SERVER;

pub fn run_standalone_from_env() -> anyhow::Result<()> {
    let run_mode = match std::env::var(RUN_MODE_ENV).as_deref() {
        Ok(RUN_MODE_SERVER) => {
            std::env::set_var("UC_DISABLE_SYSTEM_CLIPBOARD", "1");
            DaemonRunMode::ServerHeadless
        }
        Ok(RUN_MODE_ONESHOT) => DaemonRunMode::Oneshot,
        _ => DaemonRunMode::Standalone,
    };
    run(run_mode)
}
