//! Daemon-owned Engine diagnostics and desktop archive assembly.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use chrono::Utc;
use tokio_util::sync::CancellationToken;
use uc_daemon_contract::api::dto::diagnostics::{
    DiagnosticArchiveCollectionDto, DiagnosticCaptureEndReasonDto, DiagnosticCaptureModeDto,
    DiagnosticCaptureStartRequestDto, DiagnosticCaptureStateDto, DiagnosticCaptureStopResultDto,
    DiagnosticExportPreparationDto, DiagnosticFileSourceCountsDto, DiagnosticSetupStatusDto,
    DiagnosticSignalResultDto, DiagnosticSourceCapabilityDto, DiagnosticSourceCollectionDto,
    DiagnosticSourceCoverageDto, DiagnosticSourceDto, DiagnosticStatusDto, LogExportResultDto,
    MissingLogSourceDto, NetworkArchiveCollectionDto, NetworkArchiveFileDto,
    NetworkArchiveOutcomeDto, NetworkArchiveReasonDto, NetworkArchiveRunDto,
    NetworkCaptureLinkFailureDto, NetworkCaptureModeDto, NetworkExportPreparationDto,
    NetworkLibraryStatusDto, NetworkLogAvailabilityDto, NetworkLogBudgetDto, NetworkLogCountersDto,
    NetworkLogFileDto, NetworkLogStateDto, NetworkLogStatusDto, NetworkProcessRoleDto,
};
use uc_engine::observability::{
    CaptureEndReason, DetailedCaptureRequest, HostDiagnosticAction, HostDiagnosticEvent,
    HostDiagnosticFailure, HostDiagnosticOutcome, HostDiagnosticSource, HostLifecycleState,
    LocalCaptureMode, LocalCaptureStatus, LocalDiagnosticError, LocalDiagnosticExportReport,
    LocalDiagnosticSource, LocalDiagnosticStatus, NetworkCaptureMode, NetworkLogBudget,
    NetworkLogError, NetworkLogExportReport, NetworkLogState, NetworkLogStatus, NetworkProcessRole,
    ObservabilitySetupStatus, ObservabilitySignalResult, ProcessObservabilityHandle,
    SourceCapability, SourceCollection, SourceCoverage, StopCaptureResult,
};
use uc_observability::network_archive::{
    FlushOutcome, NetworkArchiveOutcome, NetworkArchiveReason, NetworkArchiveReport,
    NetworkArchiveRequest, NetworkLive, PreparedFile,
};
use uc_observability::startup_logs::{
    export_diagnostic_logs, DiagnosticArchiveMode, DiagnosticArchiveRequest, ExportFailureCause,
    HostManifest, MissingReason,
};
use uc_webserver::api::server::{
    DaemonDiagnosticArchive, DaemonDiagnosticArchiveError, DaemonDiagnosticError,
    DaemonDiagnosticsRuntime,
};

const EXPORT_PREPARATION_DEADLINE: Duration = Duration::from_secs(1);
/// The network log flushes on its own writer thread with its own deadline, so
/// a slow or failing network log neither shortens nor fails the business flush.
const NETWORK_EXPORT_DEADLINE: Duration = Duration::from_secs(1);
const OBSERVABILITY_SHUTDOWN_DEADLINE: Duration = Duration::from_secs(2);
/// Bounds the flush that keeps the network log's tail on a normal exit.
const NETWORK_SHUTDOWN_FLUSH_DEADLINE: Duration = Duration::from_secs(1);
const SUSPENSION_CHECK_INTERVAL: Duration = Duration::from_secs(5);
const SUSPENSION_GAP: Duration = Duration::from_secs(15);

pub struct EngineDaemonDiagnostics {
    handle: ProcessObservabilityHandle,
    /// Budget the daemon configured for the network log, if it did. The Engine
    /// status does not report it.
    network_budget: Option<NetworkLogBudget>,
    /// Id of the detailed network window this daemon opened; the Engine status
    /// does not carry it. Only reported while the Engine says the window is open.
    network_capture_id: Mutex<Option<String>>,
    /// Why the network window is not following the business capture, if it is not.
    network_link_failure: Mutex<Option<NetworkCaptureLinkFailureDto>>,
    startup_token: Mutex<Option<String>>,
    runtime_started: Mutex<bool>,
}

impl EngineDaemonDiagnostics {
    /// The network log's detailed window is part of the business diagnostic
    /// capture: it opens and closes with it and has no switch of its own. A
    /// network failure never fails the business capture; it is logged and shows
    /// in the network status.
    fn start_network_with_business(&self, duration_seconds: u16) {
        if self.network_budget.is_none() {
            return;
        }
        let duration = Duration::from_secs(u64::from(duration_seconds.clamp(1, 900)));
        match self.handle.start_network_capture(duration) {
            Ok(started) => {
                *self
                    .network_capture_id
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(started.capture_id);
                self.set_network_link_failure(None);
            }
            Err(error) => {
                self.set_network_link_failure(Some(NetworkCaptureLinkFailureDto::StartFailed));
                tracing::warn!(
                    ?error,
                    "network log detailed window did not start with the diagnostic capture"
                );
            }
        }
    }

    fn set_network_link_failure(&self, failure: Option<NetworkCaptureLinkFailureDto>) {
        *self
            .network_link_failure
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = failure;
    }

    /// Brings the network window back in line with the business capture when
    /// one of them changed on its own: a window that failed to start is retried
    /// for the time the business capture has left, and a window the business
    /// capture no longer justifies is closed.
    fn reconcile_network_with_business(&self, network: &NetworkLogStatus) -> bool {
        if network.state != NetworkLogState::Ready {
            return false;
        }
        let business = self.handle.query_local_diagnostic_status().capture;
        let business_detailed = business.mode == LocalCaptureMode::Detailed;
        let network_detailed = network.mode == NetworkCaptureMode::Detailed;
        if business_detailed && !network_detailed {
            let remaining_secs = business.remaining_ms.div_ceil(1000).clamp(1, 900);
            self.start_network_with_business(u16::try_from(remaining_secs).unwrap_or(900));
            return true;
        }
        if !business_detailed && network_detailed {
            self.stop_network_with_business();
            return true;
        }
        // Both windows are closed, so a failure of the finished capture no longer
        // describes anything that is still open.
        if !business_detailed && !network_detailed {
            self.set_network_link_failure(None);
        }
        false
    }

    fn stop_network_with_business(&self) {
        let remembered = self
            .network_capture_id
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        if let Some(capture_id) = remembered {
            // No matching window means it is already closed, the state asked for.
            if let Err(error) = self.handle.stop_network_capture(&capture_id) {
                if error == NetworkLogError::CaptureMismatch {
                    self.set_network_link_failure(None);
                    return;
                }
                self.set_network_link_failure(Some(NetworkCaptureLinkFailureDto::StopFailed));
                tracing::warn!(
                    ?error,
                    "network log detailed window did not stop with the diagnostic capture"
                );
                return;
            }
            self.set_network_link_failure(None);
        }
    }

    pub fn new(
        handle: ProcessObservabilityHandle,
        network_budget: Option<NetworkLogBudget>,
    ) -> Self {
        let _ = handle.register_host_diagnostic_source(
            HostDiagnosticSource::Application,
            SourceCapability::Partial,
        );
        let receipt = handle.record_host_diagnostic(
            HostDiagnosticSource::Application,
            HostDiagnosticEvent::Begin {
                action: HostDiagnosticAction::RuntimeStart,
            },
        );
        let startup_token = receipt.token;
        let _ = handle.record_host_diagnostic(
            HostDiagnosticSource::Application,
            HostDiagnosticEvent::Lifecycle {
                state: HostLifecycleState::Foreground,
            },
        );
        Self {
            handle,
            network_budget,
            network_capture_id: Mutex::new(None),
            network_link_failure: Mutex::new(None),
            startup_token: Mutex::new(startup_token),
            runtime_started: Mutex::new(false),
        }
    }

    pub fn mark_runtime_started(&self) {
        let token = self
            .startup_token
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        if let Some(token) = token {
            let _ = self.handle.record_host_diagnostic(
                HostDiagnosticSource::Application,
                HostDiagnosticEvent::Finish {
                    token,
                    outcome: HostDiagnosticOutcome::Completed,
                },
            );
        }
        *self
            .runtime_started
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = true;
    }

    pub fn finish_process(&self, result: &anyhow::Result<()>) {
        let started = *self
            .runtime_started
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if started {
            let receipt = self.handle.record_host_diagnostic(
                HostDiagnosticSource::Application,
                HostDiagnosticEvent::Begin {
                    action: HostDiagnosticAction::RuntimeStop,
                },
            );
            if let Some(token) = receipt.token {
                let outcome = if result.is_ok() {
                    HostDiagnosticOutcome::Completed
                } else {
                    HostDiagnosticOutcome::Failed(HostDiagnosticFailure::Unknown)
                };
                let _ = self.handle.record_host_diagnostic(
                    HostDiagnosticSource::Application,
                    HostDiagnosticEvent::Finish { token, outcome },
                );
            }
        } else {
            let token = self
                .startup_token
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take();
            if let Some(token) = token {
                let outcome = if result.is_ok() {
                    HostDiagnosticOutcome::Interrupted
                } else {
                    HostDiagnosticOutcome::Failed(HostDiagnosticFailure::Unavailable)
                };
                let _ = self.handle.record_host_diagnostic(
                    HostDiagnosticSource::Application,
                    HostDiagnosticEvent::Finish { token, outcome },
                );
            }
        }
        let _ = self.handle.record_host_diagnostic(
            HostDiagnosticSource::Application,
            HostDiagnosticEvent::OwnershipReleased,
        );
        // The Engine drains the network log on its own detached thread when it shuts
        // down, which the process may outrun on exit. By now the business Engine has
        // stopped, so this bounded flush keeps the connection records of the last
        // seconds without delaying anything the user waits for.
        if self.network_budget.is_some() {
            let _ = self.handle.force_flush(NETWORK_SHUTDOWN_FLUSH_DEADLINE);
        }
        let _ = self.handle.shutdown(OBSERVABILITY_SHUTDOWN_DEADLINE);
    }

    pub async fn watch_process_suspension(self: std::sync::Arc<Self>, cancel: CancellationToken) {
        let mut last_tick = SystemTime::now();
        let mut interval = tokio::time::interval(SUSPENSION_CHECK_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = cancel.cancelled() => return,
                _ = interval.tick() => {
                    let now = SystemTime::now();
                    if suspension_gap(last_tick, now) {
                        let _ = self.handle.record_host_diagnostic(
                            HostDiagnosticSource::Application,
                            HostDiagnosticEvent::Lifecycle { state: HostLifecycleState::Background },
                        );
                        let _ = self.handle.record_host_diagnostic(
                            HostDiagnosticSource::Application,
                            HostDiagnosticEvent::Lifecycle { state: HostLifecycleState::Foreground },
                        );
                    }
                    last_tick = now;
                }
            }
        }
    }
}

fn suspension_gap(previous: SystemTime, current: SystemTime) -> bool {
    current
        .duration_since(previous)
        .is_ok_and(|elapsed| elapsed >= SUSPENSION_GAP)
}

impl DaemonDiagnosticsRuntime for EngineDaemonDiagnostics {
    fn status(&self) -> Result<DiagnosticStatusDto, DaemonDiagnosticError> {
        Ok(status_dto(self.handle.query_local_diagnostic_status()))
    }

    fn start(
        &self,
        request: DiagnosticCaptureStartRequestDto,
    ) -> Result<DiagnosticStatusDto, DaemonDiagnosticError> {
        self.handle
            .start_local_diagnostic_capture(DetailedCaptureRequest {
                duration: Duration::from_secs(u64::from(request.duration_seconds)),
            })
            .map_err(diagnostic_error)?;
        self.start_network_with_business(request.duration_seconds);
        self.status()
    }

    fn stop(
        &self,
        capture_id: &str,
    ) -> Result<DiagnosticCaptureStopResultDto, DaemonDiagnosticError> {
        let result = self
            .handle
            .stop_local_diagnostic_capture(capture_id)
            .map(stop_result_dto)
            .map_err(diagnostic_error);
        // A different capture id means the business window was not the one the
        // caller meant; the network window stays with it.
        if matches!(
            result,
            Ok(DiagnosticCaptureStopResultDto::Stopped
                | DiagnosticCaptureStopResultDto::AlreadyStopped)
        ) {
            self.stop_network_with_business();
        }
        result
    }

    fn prepare_export(&self) -> Result<DiagnosticExportPreparationDto, DaemonDiagnosticError> {
        // The business log is prepared first and fails on its own terms; the
        // network log never turns its result into a business failure.
        let business = self
            .handle
            .prepare_local_diagnostic_export(EXPORT_PREPARATION_DEADLINE)
            .map_err(diagnostic_error)?;
        let network = self.network_export_preparation();
        Ok(export_preparation_dto(business, network))
    }

    fn network_status(&self) -> NetworkLogStatusDto {
        let mut queried = self.handle.query_network_log_status();
        if let Ok(status) = &queried {
            if self.reconcile_network_with_business(status) {
                queried = self.handle.query_network_log_status();
            }
        }
        match queried {
            Ok(status) => {
                let mut dto = network_status_dto(status, self.network_budget);
                dto.capture_link_failure = *self
                    .network_link_failure
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let mut remembered = self
                    .network_capture_id
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if dto.mode == Some(NetworkCaptureModeDto::Detailed) {
                    dto.capture_id = remembered.clone();
                } else {
                    *remembered = None;
                }
                dto
            }
            Err(NetworkLogError::NotConfigured) => not_configured_network_status(),
            // A configured log whose query fails (for example after shutdown) is
            // unavailable, not unconfigured.
            Err(_) => NetworkLogStatusDto {
                availability: NetworkLogAvailabilityDto::Configured,
                state: Some(NetworkLogStateDto::Unavailable),
                ..not_configured_network_status()
            },
        }
    }
}

impl EngineDaemonDiagnostics {
    fn network_export_preparation(&self) -> NetworkExportPreparationDto {
        match self
            .handle
            .prepare_network_log_export(NETWORK_EXPORT_DEADLINE)
        {
            Ok(report) => network_export_dto(report, self.network_budget),
            Err(NetworkLogError::NotConfigured) => NetworkExportPreparationDto {
                availability: NetworkLogAvailabilityDto::NotConfigured,
                flush: None,
                status: None,
                requested_at_utc: None,
                completed_at_utc: None,
                other_processes_flushed: false,
                files: Vec::new(),
            },
            // Configured, but the Engine refused to prepare: say so instead of
            // reporting an empty log.
            Err(_) => NetworkExportPreparationDto {
                availability: NetworkLogAvailabilityDto::Configured,
                flush: Some(DiagnosticSignalResultDto::Failed),
                status: None,
                requested_at_utc: None,
                completed_at_utc: None,
                other_processes_flushed: false,
                files: Vec::new(),
            },
        }
    }
}

pub struct DesktopDiagnosticArchive {
    logs_dir: PathBuf,
}

impl DesktopDiagnosticArchive {
    pub fn new(logs_dir: PathBuf) -> Self {
        Self { logs_dir }
    }
}

#[async_trait::async_trait]
impl DaemonDiagnosticArchive for DesktopDiagnosticArchive {
    async fn export(
        &self,
        since_hours: Option<u32>,
        engine_preparation: DiagnosticExportPreparationDto,
    ) -> Result<LogExportResultDto, DaemonDiagnosticArchiveError> {
        let hours = since_hours.unwrap_or(24).max(1);
        let since = Utc::now() - chrono::Duration::hours(i64::from(hours));
        let directory = dirs::download_dir()
            .filter(|directory| std::fs::create_dir_all(directory).is_ok())
            .ok_or(DaemonDiagnosticArchiveError::DestinationUnavailable)?;
        let destination = directory.join(format!(
            "uniclipboard-diagnostics-{}.zip",
            Utc::now().format("%Y%m%d-%H%M%S")
        ));
        let logs_dir = self.logs_dir.clone();
        let destination_for_worker = destination.clone();
        let manifest_preparation = serde_json::to_value(&engine_preparation)
            .map_err(|_| DaemonDiagnosticArchiveError::Unknown)?;
        let network = NetworkArchiveRequest {
            live: Some(network_live(&engine_preparation.network)),
        };
        let report = tokio::task::spawn_blocking(move || {
            export_diagnostic_logs(
                &logs_dir,
                &destination_for_worker,
                DiagnosticArchiveRequest {
                    mode: DiagnosticArchiveMode::Online,
                    since: Some(since),
                    engine_preparation: Some(manifest_preparation),
                    startup_status: None,
                    host: Some(host_manifest()),
                    network,
                },
            )
        })
        .await
        .map_err(|_| DaemonDiagnosticArchiveError::Unknown)?
        .map_err(|error| match error.cause() {
            ExportFailureCause::SourceRead => DaemonDiagnosticArchiveError::SourceReadFailed,
            ExportFailureCause::ArchiveWrite => DaemonDiagnosticArchiveError::ArchiveWriteFailed,
            ExportFailureCause::NoLogs => DaemonDiagnosticArchiveError::Unknown,
        })?;
        Ok(LogExportResultDto {
            path: destination.to_string_lossy().into_owned(),
            included_files: report.included_files.clone(),
            since,
            engine_preparation,
            collection: DiagnosticArchiveCollectionDto {
                included_files: report.included_files,
                unreadable_files: report.unreadable_files,
                truncated_files: report.truncated_files,
                concurrent_writes_possible: report.concurrent_writes_possible,
                missing_sources: report
                    .missing_sources
                    .into_iter()
                    .map(|missing| MissingLogSourceDto {
                        source: missing.source.to_owned(),
                        reason: match missing.reason {
                            MissingReason::Missing => "missing",
                            MissingReason::Unreadable => "unreadable",
                        }
                        .to_owned(),
                    })
                    .collect(),
                network: network_archive_dto(report.network),
            },
        })
    }
}

/// Facts about this daemon process for the export manifest. No device, peer or
/// account identifier. The Engine's source commit travels in the Engine
/// preparation block; `build_commit` is the Desktop commit stamped by `build.rs`.
fn host_manifest() -> HostManifest {
    HostManifest {
        app_version: env!("CARGO_PKG_VERSION").to_owned(),
        build_commit: env!("UC_BUILD_COMMIT").to_owned(),
        os: std::env::consts::OS.to_owned(),
        arch: std::env::consts::ARCH.to_owned(),
        profile: uc_app_paths::resolve_profile(None),
        portable: uc_app_paths::is_portable(),
        installed_log_profile: uc_observability::installed_log_profile()
            .unwrap_or("unknown")
            .to_owned(),
        telemetry_enabled: uc_observability::is_telemetry_enabled(),
        timezone_offset_minutes: chrono::Local::now().offset().local_minus_utc() / 60,
    }
}

fn diagnostic_error(error: LocalDiagnosticError) -> DaemonDiagnosticError {
    match error {
        LocalDiagnosticError::InvalidDuration
        | LocalDiagnosticError::InvalidCaptureId
        | LocalDiagnosticError::InvalidDeadline
        | LocalDiagnosticError::InvalidHostSource => DaemonDiagnosticError::InvalidInput,
        LocalDiagnosticError::NotInstalled | LocalDiagnosticError::LocalSinkUnavailable => {
            DaemonDiagnosticError::Unavailable
        }
        LocalDiagnosticError::AlreadyShutdown => DaemonDiagnosticError::AlreadyShutdown,
    }
}

fn status_dto(status: LocalDiagnosticStatus) -> DiagnosticStatusDto {
    DiagnosticStatusDto {
        run_id: status.run_id,
        capture: capture_dto(status.capture),
        observed_records: status.observed_records.to_string(),
        policy_filtered_records: status.policy_filtered_records.to_string(),
        schema_rejected_records: status.schema_rejected_records.to_string(),
        correlation_limited_records: status.correlation_limited_records.to_string(),
        engine_version: status.engine_version,
        source_commit: status.source_commit,
        counter_scope: status.counter_scope.to_string(),
        sources: status
            .sources
            .into_iter()
            .map(source_coverage_dto)
            .collect(),
        local_file: setup_status_dto(status.local_file),
        closed: status.closed,
    }
}

fn capture_dto(capture: LocalCaptureStatus) -> DiagnosticCaptureStateDto {
    DiagnosticCaptureStateDto {
        mode: match capture.mode {
            LocalCaptureMode::Standard => DiagnosticCaptureModeDto::Standard,
            LocalCaptureMode::Detailed => DiagnosticCaptureModeDto::Detailed,
        },
        capture_id: capture.capture_id,
        remaining_ms: capture.remaining_ms,
        started_at_utc: capture.started_at_utc,
        end_reason: capture.end_reason.map(|reason| match reason {
            CaptureEndReason::Expired => DiagnosticCaptureEndReasonDto::Expired,
            CaptureEndReason::Requested => DiagnosticCaptureEndReasonDto::Requested,
            CaptureEndReason::SuspensionExpiryUnknown => {
                DiagnosticCaptureEndReasonDto::SuspensionExpiryUnknown
            }
            CaptureEndReason::RuntimeShutdown => DiagnosticCaptureEndReasonDto::RuntimeShutdown,
        }),
        last_capture_id: capture.last_capture_id,
        revision: capture.revision.to_string(),
    }
}

fn source_coverage_dto(source: SourceCoverage) -> DiagnosticSourceCoverageDto {
    DiagnosticSourceCoverageDto {
        source: source_dto(source.source),
        capability: match source.capability {
            SourceCapability::Supported => DiagnosticSourceCapabilityDto::Supported,
            SourceCapability::Partial => DiagnosticSourceCapabilityDto::Partial,
            SourceCapability::Unsupported => DiagnosticSourceCapabilityDto::Unsupported,
            SourceCapability::Unknown => DiagnosticSourceCapabilityDto::Unknown,
        },
        collection: match source.collection {
            SourceCollection::Enabled => DiagnosticSourceCollectionDto::Enabled,
            SourceCollection::Disabled => DiagnosticSourceCollectionDto::Disabled,
            SourceCollection::Unavailable => DiagnosticSourceCollectionDto::Unavailable,
            SourceCollection::NotRegistered => DiagnosticSourceCollectionDto::NotRegistered,
        },
        observed_count: source.observed_count.to_string(),
        policy_filtered_count: source.policy_filtered_count.to_string(),
    }
}

fn source_dto(source: LocalDiagnosticSource) -> DiagnosticSourceDto {
    match source {
        LocalDiagnosticSource::Runtime => DiagnosticSourceDto::Runtime,
        LocalDiagnosticSource::Connections => DiagnosticSourceDto::Connections,
        LocalDiagnosticSource::AddressStorage => DiagnosticSourceDto::AddressStorage,
        LocalDiagnosticSource::DnsDiscovery => DiagnosticSourceDto::DnsDiscovery,
        LocalDiagnosticSource::MdnsDiscovery => DiagnosticSourceDto::MdnsDiscovery,
        LocalDiagnosticSource::PkarrDiscovery => DiagnosticSourceDto::PkarrDiscovery,
        LocalDiagnosticSource::ConnectionPaths => DiagnosticSourceDto::ConnectionPaths,
        LocalDiagnosticSource::RelayRecovery => DiagnosticSourceDto::RelayRecovery,
        LocalDiagnosticSource::MembershipUpdates => DiagnosticSourceDto::MembershipUpdates,
        LocalDiagnosticSource::Sessions => DiagnosticSourceDto::Sessions,
        LocalDiagnosticSource::HostApplication => DiagnosticSourceDto::HostApplication,
        LocalDiagnosticSource::HostShareExtension => DiagnosticSourceDto::HostShareExtension,
        LocalDiagnosticSource::HostKeyboardExtension => DiagnosticSourceDto::HostKeyboardExtension,
        LocalDiagnosticSource::HostBackgroundService => DiagnosticSourceDto::HostBackgroundService,
    }
}

fn setup_status_dto(status: ObservabilitySetupStatus) -> DiagnosticSetupStatusDto {
    match status {
        ObservabilitySetupStatus::Disabled => DiagnosticSetupStatusDto::Disabled,
        ObservabilitySetupStatus::Ready => DiagnosticSetupStatusDto::Ready,
        ObservabilitySetupStatus::Unavailable => DiagnosticSetupStatusDto::Unavailable,
    }
}

fn signal_result_dto(result: ObservabilitySignalResult) -> DiagnosticSignalResultDto {
    match result {
        ObservabilitySignalResult::Completed => DiagnosticSignalResultDto::Completed,
        ObservabilitySignalResult::Failed => DiagnosticSignalResultDto::Failed,
        ObservabilitySignalResult::TimedOut => DiagnosticSignalResultDto::TimedOut,
        ObservabilitySignalResult::AlreadyShutdown => DiagnosticSignalResultDto::AlreadyShutdown,
    }
}

fn stop_result_dto(result: StopCaptureResult) -> DiagnosticCaptureStopResultDto {
    match result {
        StopCaptureResult::Stopped => DiagnosticCaptureStopResultDto::Stopped,
        StopCaptureResult::AlreadyStopped => DiagnosticCaptureStopResultDto::AlreadyStopped,
        StopCaptureResult::DifferentCapture => DiagnosticCaptureStopResultDto::DifferentCapture,
    }
}

fn export_preparation_dto(
    report: LocalDiagnosticExportReport,
    network: NetworkExportPreparationDto,
) -> DiagnosticExportPreparationDto {
    DiagnosticExportPreparationDto {
        network,
        flush: signal_result_dto(report.flush),
        status: status_dto(report.status),
        requested_at_utc: report.requested_at_utc,
        completed_at_utc: report.completed_at_utc,
        other_processes_flushed: report.other_processes_flushed,
        files: report
            .files
            .into_iter()
            .map(|counts| DiagnosticFileSourceCountsDto {
                source: source_dto(counts.source),
                accepted_count: counts.accepted_count.to_string(),
                written_count: counts.written_count.to_string(),
                queue_dropped_count: counts.queue_dropped_count.to_string(),
                quota_dropped_count: counts.quota_dropped_count.to_string(),
                write_failed_count: counts.write_failed_count.to_string(),
                last_written_at_ms: counts.last_written_at_ms.map(|value| value.to_string()),
            })
            .collect(),
    }
}

fn not_configured_network_status() -> NetworkLogStatusDto {
    NetworkLogStatusDto {
        availability: NetworkLogAvailabilityDto::NotConfigured,
        state: None,
        run_id: None,
        role: None,
        mode: None,
        capture_id: None,
        detailed_remaining_ms: None,
        counters: None,
        libraries: Vec::new(),
        budget: None,
        capture_link_failure: None,
        sampled_at_utc: None,
    }
}

fn network_status_dto(
    status: NetworkLogStatus,
    budget: Option<NetworkLogBudget>,
) -> NetworkLogStatusDto {
    let counters = status.counters;
    NetworkLogStatusDto {
        availability: NetworkLogAvailabilityDto::Configured,
        state: Some(match status.state {
            NetworkLogState::Starting => NetworkLogStateDto::Starting,
            NetworkLogState::Ready => NetworkLogStateDto::Ready,
            NetworkLogState::Unavailable => NetworkLogStateDto::Unavailable,
            NetworkLogState::Closed => NetworkLogStateDto::Closed,
        }),
        run_id: Some(status.run_id),
        role: Some(match status.role {
            NetworkProcessRole::Daemon => NetworkProcessRoleDto::Daemon,
            NetworkProcessRole::MobileApp => NetworkProcessRoleDto::MobileApp,
            NetworkProcessRole::KeyboardExtension => NetworkProcessRoleDto::KeyboardExtension,
            NetworkProcessRole::ShareExtension => NetworkProcessRoleDto::ShareExtension,
            NetworkProcessRole::BackgroundService => NetworkProcessRoleDto::BackgroundService,
        }),
        mode: Some(match status.mode {
            NetworkCaptureMode::Standard => NetworkCaptureModeDto::Standard,
            NetworkCaptureMode::Detailed => NetworkCaptureModeDto::Detailed,
        }),
        capture_id: None,
        capture_link_failure: None,
        detailed_remaining_ms: status.detailed_remaining_ms,
        counters: Some(NetworkLogCountersDto {
            accepted: counters.accepted.to_string(),
            written: counters.written.to_string(),
            policy_filtered: counters.policy_filtered.to_string(),
            rate_limited: counters.rate_limited.to_string(),
            oversized: counters.oversized.to_string(),
            queue_dropped: counters.queue_dropped.to_string(),
            closed_dropped: counters.closed_dropped.to_string(),
            write_failed: counters.write_failed.to_string(),
            rotated_out_files: counters.rotated_out_files.to_string(),
            path_recovered: counters.path_recovered.to_string(),
            unreviewed_callsites: counters.unreviewed_callsites.to_string(),
        }),
        libraries: status
            .libraries
            .into_iter()
            .map(|library| NetworkLibraryStatusDto {
                library: library.library.to_owned(),
                version_dir: library.version_dir.map(str::to_owned),
                approved_rules: u32::try_from(library.approved_rules).unwrap_or(u32::MAX),
                observed_records: library.observed_records.to_string(),
            })
            .collect(),
        budget: budget.map(|budget| NetworkLogBudgetDto {
            record_bytes: budget.record_bytes.to_string(),
            queue_records: budget.queue_records.to_string(),
            file_bytes: budget.file_bytes.to_string(),
            run_bytes: budget.run_bytes.to_string(),
            root_bytes: budget.root_bytes.to_string(),
            retention_seconds: budget.retention.as_secs().to_string(),
        }),
        sampled_at_utc: Some(status.sampled_at_utc),
    }
}

fn network_export_dto(
    report: NetworkLogExportReport,
    budget: Option<NetworkLogBudget>,
) -> NetworkExportPreparationDto {
    NetworkExportPreparationDto {
        availability: NetworkLogAvailabilityDto::Configured,
        flush: Some(signal_result_dto(report.flush)),
        status: Some(network_status_dto(report.status, budget)),
        requested_at_utc: Some(report.requested_at_utc),
        completed_at_utc: Some(report.completed_at_utc),
        other_processes_flushed: report.other_processes_flushed,
        files: report
            .files
            .into_iter()
            .map(|file| NetworkLogFileDto {
                name: file.name,
                bytes: file.bytes.to_string(),
                sha256: file.sha256,
            })
            .collect(),
    }
}

/// What the archive builder needs from the Engine's network preparation: which
/// run is live, how the flush ended and which digests to hold the copy against.
/// `None` for a configured log whose preparation carries no status is mapped to
/// a failed flush with no run, never to a clean one.
fn network_live(preparation: &NetworkExportPreparationDto) -> NetworkLive {
    let status = preparation.status.as_ref();
    let counters = status.and_then(|status| status.counters.as_ref());
    let count = |value: &str| value.parse::<u64>().unwrap_or(u64::MAX);
    NetworkLive {
        configured: preparation.availability == NetworkLogAvailabilityDto::Configured,
        role: status
            .and_then(|status| status.role)
            .map(role_directory)
            .unwrap_or_default()
            .to_owned(),
        run_id: status
            .and_then(|status| status.run_id.clone())
            .unwrap_or_default(),
        flush: preparation.flush.map(|flush| match flush {
            DiagnosticSignalResultDto::Completed => FlushOutcome::Completed,
            DiagnosticSignalResultDto::Failed => FlushOutcome::Failed,
            DiagnosticSignalResultDto::TimedOut => FlushOutcome::TimedOut,
            DiagnosticSignalResultDto::AlreadyShutdown => FlushOutcome::AlreadyShutdown,
        }),
        state_ready: status.is_some_and(|status| status.state == Some(NetworkLogStateDto::Ready)),
        // Counters that mean a record was lost, as opposed to filtered by policy.
        records_dropped: counters.is_some_and(|counters| {
            [
                &counters.queue_dropped,
                &counters.closed_dropped,
                &counters.write_failed,
                &counters.oversized,
                &counters.rate_limited,
            ]
            .into_iter()
            .any(|value| count(value) > 0)
        }),
        prepared_files: preparation
            .files
            .iter()
            .map(|file| PreparedFile {
                name: file.name.clone(),
                bytes: file.bytes.parse().unwrap_or(u64::MAX),
                sha256: file.sha256.clone(),
            })
            .collect(),
    }
}

/// Directory name the Engine uses for a role (`network/<role>/<run>`).
fn role_directory(role: NetworkProcessRoleDto) -> &'static str {
    match role {
        NetworkProcessRoleDto::Daemon => "daemon",
        NetworkProcessRoleDto::MobileApp => "mobile_app",
        NetworkProcessRoleDto::KeyboardExtension => "keyboard_extension",
        NetworkProcessRoleDto::ShareExtension => "share_extension",
        NetworkProcessRoleDto::BackgroundService => "background_service",
    }
}

fn network_archive_dto(report: NetworkArchiveReport) -> NetworkArchiveCollectionDto {
    NetworkArchiveCollectionDto {
        outcome: match report.outcome {
            NetworkArchiveOutcome::Complete => NetworkArchiveOutcomeDto::Complete,
            NetworkArchiveOutcome::Partial => NetworkArchiveOutcomeDto::Partial,
            NetworkArchiveOutcome::Missing => NetworkArchiveOutcomeDto::Missing,
            NetworkArchiveOutcome::NotConfigured => NetworkArchiveOutcomeDto::NotConfigured,
            NetworkArchiveOutcome::Unknown => NetworkArchiveOutcomeDto::Unknown,
        },
        reasons: report
            .reasons
            .into_iter()
            .map(|reason| match reason {
                NetworkArchiveReason::EngineOffline => NetworkArchiveReasonDto::EngineOffline,
                NetworkArchiveReason::FlushFailed => NetworkArchiveReasonDto::FlushFailed,
                NetworkArchiveReason::FlushTimedOut => NetworkArchiveReasonDto::FlushTimedOut,
                NetworkArchiveReason::StateNotReady => NetworkArchiveReasonDto::StateNotReady,
                NetworkArchiveReason::RecordsDropped => NetworkArchiveReasonDto::RecordsDropped,
                NetworkArchiveReason::FileUnreadable => NetworkArchiveReasonDto::FileUnreadable,
                NetworkArchiveReason::FileTruncated => NetworkArchiveReasonDto::FileTruncated,
                NetworkArchiveReason::FileGrewAfterPrepare => {
                    NetworkArchiveReasonDto::FileGrewAfterPrepare
                }
                NetworkArchiveReason::FileChangedAfterPrepare => {
                    NetworkArchiveReasonDto::FileChangedAfterPrepare
                }
                NetworkArchiveReason::OtherProcessesNotFlushed => {
                    NetworkArchiveReasonDto::OtherProcessesNotFlushed
                }
                NetworkArchiveReason::EntriesIgnored => NetworkArchiveReasonDto::EntriesIgnored,
            })
            .collect(),
        runs: report
            .runs
            .into_iter()
            .map(|run| NetworkArchiveRunDto {
                role: run.role,
                run_id: run.run_id,
                current_run: run.current_run,
                files: run
                    .files
                    .into_iter()
                    .map(|file| NetworkArchiveFileDto {
                        name: file.name,
                        bytes: file.bytes.to_string(),
                        sha256: file.sha256,
                        truncated: file.truncated,
                        matches_prepared: file.matches_prepared,
                    })
                    .collect(),
                unreadable_files: run.unreadable_files,
            })
            .collect(),
        ignored_entries: report.ignored_entries,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_manifest_describes_this_process_without_identifiers() {
        let manifest = host_manifest();
        assert_eq!(manifest.app_version, env!("CARGO_PKG_VERSION"));
        assert!(!manifest.os.is_empty() && !manifest.arch.is_empty());
        assert_eq!(manifest.installed_log_profile, "unknown");
        let json = serde_json::to_string(&manifest).expect("json");
        assert!(!json.contains("device"), "{json}");
        assert!(!json.contains("peer"), "{json}");
    }

    use uc_engine::observability::{
        managed_log_files, DeploymentEnvironment, LocalLogConfig, ObservabilityConfig,
        ObservabilityResource, OperatingSystem, ProcessObservabilityRuntime,
    };

    #[test]
    fn daemon_owns_one_capture_and_reports_real_coverage() {
        let logs = tempfile::tempdir().expect("logs");
        let config = ObservabilityConfig::new(
            ObservabilityResource::new(
                "1.0.0",
                DeploymentEnvironment::Test,
                OperatingSystem::Macos,
                "test",
            )
            .expect("resource"),
        )
        .with_local_logs(LocalLogConfig::new(logs.path()));
        let handle = ProcessObservabilityRuntime::install(config)
            .expect("install")
            .handle();
        let diagnostics = EngineDaemonDiagnostics::new(handle, None);
        diagnostics.mark_runtime_started();

        let first = diagnostics
            .start(DiagnosticCaptureStartRequestDto {
                duration_seconds: 60,
            })
            .expect("first capture");
        let second = diagnostics
            .start(DiagnosticCaptureStartRequestDto {
                duration_seconds: 600,
            })
            .expect("repeated capture");
        assert_eq!(first.capture.capture_id, second.capture.capture_id);
        assert!(second.capture.remaining_ms <= first.capture.remaining_ms);

        let preparation = diagnostics.prepare_export().expect("prepare export");
        assert_eq!(preparation.status.run_id, first.run_id);
        assert_eq!(preparation.flush, DiagnosticSignalResultDto::Completed);
        assert!(!preparation.other_processes_flushed);
        assert!(preparation.status.sources.iter().any(|source| {
            source.source == DiagnosticSourceDto::HostApplication
                && source.capability == DiagnosticSourceCapabilityDto::Partial
                && source.collection == DiagnosticSourceCollectionDto::Enabled
                && source.observed_count != "0"
        }));

        let capture_id = second.capture.capture_id.expect("capture id");
        let stopped = diagnostics.stop(&capture_id).expect("stop capture");
        assert_eq!(stopped, DiagnosticCaptureStopResultDto::Stopped);
        diagnostics.finish_process(&Ok(()));

        let files = managed_log_files(logs.path()).expect("managed logs");
        let content = std::fs::read_to_string(&files[0]).expect("diagnostic content");
        assert!(content.contains("runtime_start"));
        assert!(content.contains("runtime_stop"));
        assert!(content.contains("diagnostics.run.ended"));
    }

    #[test]
    fn long_scheduler_gap_is_treated_as_a_conservative_resume() {
        let previous = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        assert!(!suspension_gap(
            previous,
            previous + SUSPENSION_GAP - Duration::from_millis(1)
        ));
        assert!(suspension_gap(previous, previous + SUSPENSION_GAP));
    }
}
