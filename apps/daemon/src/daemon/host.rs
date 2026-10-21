//! Standalone daemon host for the shared engine.

use std::path::Path;
use std::sync::Arc;

use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};
use uc_bootstrap::{
    init_daemon_tracing_subscriber, initialize_analytics_context, install_panic_logging_hook,
    prepare_desktop_engine_host, resolve_desktop_process_paths, DesktopHostFileHandles,
    DesktopHostProcessPaths,
};
use uc_daemon_local::crash_marker::{DaemonExitReport, DaemonRunMarker};
use uc_daemon_local::process_metadata::{DaemonPidManager, DaemonProcessMode};
use uc_desktop_settings::DesktopSettingsStore;
use uc_engine::{
    ActiveClipboardChanged, Engine, HostFileHandle, Operation, OperationResult, StartupProgress,
};
use uc_observability::analytics::AnalyticsPort;
use uc_observability::startup_failure::{StartupFailureClass, StartupStep};
use uc_webserver::api::auth::load_or_create_auth_token_from_conn;
use uc_webserver::api::server::{run_http_server, DaemonApiState, DaemonFileHandles};
use uc_webserver::api::startup::StartupServer;
use uc_webserver::api::tag_layout::TagLayoutStore;
use uc_webserver::api::types::{DaemonResidency, DaemonWsEvent};
use uc_webserver::security::{cleanup_rate_limiter_task, SecurityState};

use super::diagnostics::{DesktopDiagnosticArchive, EngineDaemonDiagnostics};
use super::engine_events::forward_engine_events;
use super::mobile_lan_lifecycle::{initial_lan_target, MobileLanLifecycleController};
use super::profile_claim::{claim_instance_lock, record_startup_failure};
use super::run_mode::DaemonRunMode;
use super::startup_recovery::{record_upgrade_status_at_startup, spawn_startup_recovery};
use super::tokio_runtime::build_daemon_tokio_runtime;

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
}

/// Standalone daemon binary entry: start one shared engine and block until exit.
pub fn run(run_mode: DaemonRunMode) -> anyhow::Result<()> {
    let runtime = build_daemon_tokio_runtime()?;
    runtime.block_on(run_async(run_mode))
}

async fn run_async(run_mode: DaemonRunMode) -> anyhow::Result<()> {
    let claim_started = std::time::Instant::now();
    let process_paths = resolve_process_paths()?;
    // Claim the profile before anything else is opened: tracing installs the
    // Engine's shared log files, which a daemon that is still waiting for the
    // running one must not touch.
    let _instance_lock = claim_instance_lock(
        process_paths.app_data_root(),
        process_paths.logs_dir(),
        uc_daemon_local::timing::LOCK_ACQUIRE_DEADLINE,
    )
    .await?;
    let lock_wait = claim_started.elapsed();
    uc_daemon_local::handover::clear(process_paths.app_data_root());

    let observability = init_daemon_tracing_subscriber().inspect_err(|_| {
        record_startup_failure(
            process_paths.logs_dir(),
            StartupStep::Tracing,
            StartupFailureClass::Internal,
        );
    })?;
    install_panic_logging_hook();
    info!(
        lock_wait_ms = lock_wait.as_millis() as u64,
        engine_run_id = %observability.handle.query_local_diagnostic_status().run_id,
        "daemon claimed its profile"
    );
    let diagnostics = Arc::new(EngineDaemonDiagnostics::new(
        observability.handle,
        observability.network_log_budget,
    ));
    let result =
        run_async_with_diagnostics(run_mode, Arc::clone(&diagnostics), process_paths.logs_dir())
            .await;
    if let Err(error) = &result {
        tracing::error!(error = %format!("{error:#}"), "daemon run failed");
    }
    diagnostics.finish_process(&result);
    result
}

/// Resolve the profile's directories, recording the failure when it cannot be
/// done. The log directory is looked up independently so the record still has a
/// place to go.
fn resolve_process_paths() -> anyhow::Result<DesktopHostProcessPaths> {
    resolve_desktop_process_paths().map_err(|error| {
        if let Some(logs_dir) = uc_app_paths::app_log_dir() {
            record_startup_failure(
                &logs_dir,
                StartupStep::PathResolution,
                StartupFailureClass::Internal,
            );
        }
        anyhow::Error::new(error)
    })
}

async fn run_async_with_diagnostics(
    run_mode: DaemonRunMode,
    diagnostics: Arc<EngineDaemonDiagnostics>,
    logs_dir: &Path,
) -> anyhow::Result<()> {
    let prepared = prepare_desktop_engine_host().inspect_err(|_| {
        record_startup_failure(
            logs_dir,
            StartupStep::HostPreparation,
            StartupFailureClass::Internal,
        );
    })?;
    let process_paths = prepared.process_paths().clone();
    let analytics = prepared.analytics();
    let secure_storage_source = prepared.secure_storage_source();
    let file_handles: Arc<dyn DaemonFileHandles> =
        Arc::new(DesktopDaemonFileHandles::new(prepared.file_handles()));
    let (engine_config, host_capabilities) = prepared.into_engine_start();
    // Product settings load before the Engine starts and never depend on it.
    // The profile instance lock held by `run_async` makes this the only writer
    // of the file (ADR-013).
    let desktop_settings = Arc::new(DesktopSettingsStore::load(
        process_paths
            .app_data_root()
            .join(uc_desktop_settings::FILE_NAME),
    ));

    let pid_manager = DaemonPidManager::new(process_paths.daemon_pid());
    let _pid_file_guard = DaemonPidFileGuard::activate(pid_manager, run_mode.process_mode())?;
    let token =
        load_or_create_auth_token_from_conn(&uc_daemon_local::socket::resolve_daemon_conn_path()?)?;
    let (input, progress) = StartupProgress::channel();
    let startup_server = StartupServer::bind(
        progress,
        token,
        uc_daemon_local::socket::resolve_startup_conn_path()?,
    )
    .await?;
    let mut starting = tokio::spawn(Engine::start_with_progress(
        engine_config,
        host_capabilities,
        input,
    ));
    let started = tokio::select! {
        result = &mut starting => result.map_err(anyhow::Error::new).and_then(|result| result.map_err(anyhow::Error::new)),
        signal = wait_for_shutdown_signal() => {
            signal?;
            let shutdown = match starting.await.map_err(anyhow::Error::new) {
                Ok(Ok((engine, events))) => {
                    let result = engine
                    .shutdown_until_complete()
                    .await
                    .map_err(anyhow::Error::new);
                    drop(events);
                    result
                }
                Ok(Err(error)) => Err(anyhow::Error::new(error)),
                Err(error) => Err(error),
            };
            startup_server.shutdown().await?;
            shutdown?;
            return Ok(());
        }
    };
    let (engine, events) = match started {
        Ok(result) => result,
        Err(error) => {
            // Engine errors carry only a stable code and category, never secrets.
            tracing::error!(
                error = %format!("{error:#}"),
                "engine startup failed; startup status remains available"
            );
            record_startup_failure(
                logs_dir,
                StartupStep::EngineStart,
                StartupFailureClass::Internal,
            );
            // Keep the terminal snapshot readable until an explicit retry or full quit.
            wait_for_shutdown_signal().await?;
            startup_server.shutdown().await?;
            return Err(error);
        }
    };
    let engine = Arc::new(engine);
    diagnostics.mark_runtime_started();

    initialize_analytics_context(
        &analytics,
        &engine,
        run_mode.suppresses_device_presence_analytics(),
    )
    .await;

    record_upgrade_status_at_startup(&engine).await;
    spawn_startup_recovery(run_mode, Arc::clone(&engine), secure_storage_source);

    let wake_monitor = match super::system_wake::start(Arc::clone(&engine)).await {
        Ok(monitor) => Some(monitor),
        Err(_) => {
            tracing::warn!(
                "system wake notifications unavailable; Engine periodic recovery remains active"
            );
            None
        }
    };
    let result = run_daemon_surfaces(
        run_mode,
        Arc::clone(&engine),
        events,
        file_handles,
        process_paths,
        desktop_settings,
        analytics.sink(),
        startup_server.ready_flag(),
        diagnostics,
    )
    .await;
    if let Some((cancel, task)) = wake_monitor {
        cancel.cancel();
        if task.await.is_err() {
            tracing::warn!("system wake forwarding task failed");
        }
    }
    let shutdown = engine
        .shutdown_until_complete()
        .await
        .map_err(anyhow::Error::new);

    if let Err(error) = &result {
        startup_server.mark_service_failed();
        tracing::error!(
            error = %format!("{error:#}"),
            "daemon service startup failed; startup status remains available"
        );
        record_startup_failure(
            logs_dir,
            StartupStep::ServiceStart,
            StartupFailureClass::Internal,
        );
        wait_for_shutdown_signal().await?;
    }
    startup_server.shutdown().await?;
    result.and(shutdown)
}

async fn run_daemon_surfaces(
    run_mode: DaemonRunMode,
    engine: Arc<Engine>,
    events: uc_engine::EventStream,
    file_handles: Arc<dyn DaemonFileHandles>,
    process_paths: DesktopHostProcessPaths,
    desktop_settings: Arc<DesktopSettingsStore>,
    analytics_sink: Arc<dyn AnalyticsPort>,
    startup_ready: Arc<std::sync::atomic::AtomicBool>,
    diagnostics: Arc<EngineDaemonDiagnostics>,
) -> anyhow::Result<()> {
    // ADR-011: the bearer token lives inside `daemon.conn` (load-or-create);
    // the HTTP server publishes the full connection file once it has bound.
    let conn_path = uc_daemon_local::socket::resolve_daemon_conn_path()?;
    let auth_token = load_or_create_auth_token_from_conn(&conn_path)?;
    let pid = std::process::id();
    let run_marker = DaemonRunMarker::new(process_paths.app_data_root().to_path_buf());
    begin_marked_run(&run_marker, pid);

    let security = Arc::new(SecurityState::new());
    security.register_pid(pid).await;
    let mut api_state = DaemonApiState::new(
        Arc::clone(&engine),
        file_handles,
        auth_token,
        Arc::clone(&security),
    )
    .with_residency(run_mode.into())
    .with_analytics(analytics_sink)
    .with_tag_layout(Arc::new(TagLayoutStore::load(
        process_paths
            .app_data_root()
            .join("history-tags")
            .join("layout.v1.json"),
    )))
    .with_desktop_settings(Arc::clone(&desktop_settings))
    .with_diagnostics(
        diagnostics.clone(),
        Arc::new(DesktopDiagnosticArchive::new(
            process_paths.logs_dir().to_path_buf(),
        )),
    )
    .with_run_marker(run_marker.clone());
    api_state.startup_ready = Some(startup_ready);
    let (event_tx, _) = broadcast::channel::<DaemonWsEvent>(64);
    api_state.event_tx = event_tx.clone();

    let restart = api_state.restart.clone();
    let lease_registry = api_state.lease_registry.clone();
    let graceful_stop_requested = api_state.graceful_stop_requested.clone();
    let cancel = CancellationToken::new();
    let http_cancel = cancel.child_token();
    let cleanup_cancel = cancel.child_token();
    let event_cancel = cancel.child_token();
    let suspension_cancel = cancel.child_token();
    // Subscribed before the HTTP server serves, so no product settings change is missed.
    let settings_forwarder = tokio::spawn(
        uc_webserver::api::settings_events::forward_desktop_settings(
            desktop_settings,
            event_tx.clone(),
            cancel.child_token(),
        ),
    );
    let engine_settings_revision = api_state.engine_settings_revision.clone();
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
        engine_settings_revision,
        active_clipboard_tx.clone(),
        Arc::clone(&mobile_lan),
        event_cancel,
    ));
    let suspension_observer =
        tokio::spawn(Arc::clone(&diagnostics).watch_process_suspension(suspension_cancel));
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
        _ = graceful_stop_requested.notified() => {
            // POST /lifecycle/graceful-stop already marked this run's exit
            // clean (synchronously, before notifying) — this just drives the
            // same orderly shutdown sequence an OS signal would.
            info!("graceful stop requested via HTTP control plane");
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
    // ADR-011: a graceful exit must not leave a stale connection file behind —
    // clients probe it and would otherwise see a dead PID until their identity
    // check kicks in. Crash leftovers are still handled by that PID check.
    if let Err(error) = uc_daemon_local::socket::remove_daemon_conn_file() {
        warn!(error = %error, "failed to remove daemon connection file on shutdown");
    }
    let _ = tokio::time::timeout(
        uc_daemon_local::timing::SHUTDOWN_JOIN_TIMEOUT,
        event_forwarder,
    )
    .await;
    let _ = tokio::time::timeout(
        uc_daemon_local::timing::SHUTDOWN_JOIN_TIMEOUT,
        suspension_observer,
    )
    .await;
    let _ = tokio::time::timeout(
        uc_daemon_local::timing::SHUTDOWN_JOIN_TIMEOUT,
        settings_forwarder,
    )
    .await;
    if http_completed {
        anyhow::bail!("daemon HTTP service exited unexpectedly");
    }
    mark_clean_exit_best_effort(&run_marker);
    Ok(())
}

pub(super) async fn apply_initial_mobile_lan_target(
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

/// Start this run's crash marker. Crash visibility is best effort: a marker
/// that cannot be written is logged and the daemon starts anyway.
fn begin_marked_run(run_marker: &DaemonRunMarker, pid: u32) {
    match run_marker.begin_run(pid) {
        Ok(previous) => log_previous_crash(previous),
        Err(error) => warn!(
            error = %format!("{error:#}"),
            "could not write the daemon run marker; abnormal exit detection is unavailable for this run"
        ),
    }
}

/// Clear the crash marker on a graceful exit. Failing to remove it must not
/// turn a clean shutdown into a failed run.
fn mark_clean_exit_best_effort(run_marker: &DaemonRunMarker) {
    if let Err(error) = run_marker.mark_clean_exit() {
        warn!(
            error = %format!("{error:#}"),
            "could not clear the daemon run marker on clean exit"
        );
    }
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

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::sync::{Arc, Mutex};

    use tracing_subscriber::fmt::MakeWriter;

    use super::*;

    #[derive(Clone, Default)]
    struct Captured(Arc<Mutex<Vec<u8>>>);

    impl Write for Captured {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().expect("captured").extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> MakeWriter<'a> for Captured {
        type Writer = Captured;

        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    fn unwritable_marker() -> (tempfile::TempDir, DaemonRunMarker) {
        let root = tempfile::tempdir().expect("tempdir");
        let blocker = root.path().join("not-a-directory");
        std::fs::write(&blocker, "file").expect("blocker");
        let marker = DaemonRunMarker::new(blocker.join("data"));
        (root, marker)
    }

    fn captured_warnings(run: impl FnOnce()) -> String {
        let captured = Captured::default();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(captured.clone())
            .with_ansi(false)
            .finish();
        tracing::subscriber::with_default(subscriber, run);
        let bytes = captured.0.lock().expect("captured").clone();
        String::from_utf8(bytes).expect("utf8")
    }

    #[test]
    fn a_run_marker_that_cannot_be_written_does_not_stop_the_start() {
        let (_root, marker) = unwritable_marker();

        let output = captured_warnings(|| begin_marked_run(&marker, 4242));

        assert!(output.contains("WARN"), "{output}");
        assert!(output.contains("run marker"), "{output}");
    }

    #[test]
    fn a_clean_exit_that_cannot_be_marked_does_not_fail_the_run() {
        let root = tempfile::tempdir().expect("tempdir");
        // A directory in place of the marker file cannot be removed as a file.
        std::fs::create_dir_all(root.path().join("daemon-run.json").join("x")).expect("dir");
        let marker = DaemonRunMarker::new(root.path().to_path_buf());

        let output = captured_warnings(|| mark_clean_exit_best_effort(&marker));

        assert!(output.contains("WARN"), "{output}");
        assert!(output.contains("clean exit"), "{output}");
    }
}
