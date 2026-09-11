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
};
use uc_engine::observability::{
    CaptureEndReason, DetailedCaptureRequest, HostDiagnosticAction, HostDiagnosticEvent,
    HostDiagnosticFailure, HostDiagnosticOutcome, HostDiagnosticSource, HostLifecycleState,
    LocalCaptureMode, LocalCaptureStatus, LocalDiagnosticError, LocalDiagnosticExportReport,
    LocalDiagnosticSource, LocalDiagnosticStatus, ObservabilitySetupStatus,
    ObservabilitySignalResult, ProcessObservabilityHandle, SourceCapability, SourceCollection,
    SourceCoverage, StopCaptureResult,
};
use uc_observability::startup_logs::{
    export_diagnostic_logs, DiagnosticArchiveMode, DiagnosticArchiveRequest,
};
use uc_webserver::api::server::{
    DaemonDiagnosticArchive, DaemonDiagnosticError, DaemonDiagnosticsRuntime,
};

const EXPORT_PREPARATION_DEADLINE: Duration = Duration::from_secs(1);
const OBSERVABILITY_SHUTDOWN_DEADLINE: Duration = Duration::from_secs(2);
const SUSPENSION_CHECK_INTERVAL: Duration = Duration::from_secs(5);
const SUSPENSION_GAP: Duration = Duration::from_secs(15);

pub struct EngineDaemonDiagnostics {
    handle: ProcessObservabilityHandle,
    startup_token: Mutex<Option<String>>,
    runtime_started: Mutex<bool>,
}

impl EngineDaemonDiagnostics {
    pub fn new(handle: ProcessObservabilityHandle) -> Self {
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
        self.status()
    }

    fn stop(
        &self,
        capture_id: &str,
    ) -> Result<DiagnosticCaptureStopResultDto, DaemonDiagnosticError> {
        self.handle
            .stop_local_diagnostic_capture(capture_id)
            .map(stop_result_dto)
            .map_err(diagnostic_error)
    }

    fn prepare_export(&self) -> Result<DiagnosticExportPreparationDto, DaemonDiagnosticError> {
        self.handle
            .prepare_local_diagnostic_export(EXPORT_PREPARATION_DEADLINE)
            .map(export_preparation_dto)
            .map_err(diagnostic_error)
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
    ) -> anyhow::Result<LogExportResultDto> {
        let hours = since_hours.unwrap_or(24).max(1);
        let since = Utc::now() - chrono::Duration::hours(i64::from(hours));
        let directory = dirs::download_dir()
            .ok_or_else(|| anyhow::anyhow!("Downloads directory is unavailable"))?;
        std::fs::create_dir_all(&directory)?;
        let destination = directory.join(format!(
            "uniclipboard-diagnostics-{}.zip",
            Utc::now().format("%Y%m%d-%H%M%S")
        ));
        let logs_dir = self.logs_dir.clone();
        let destination_for_worker = destination.clone();
        let manifest_preparation = serde_json::to_value(&engine_preparation)?;
        let report = tokio::task::spawn_blocking(move || {
            export_diagnostic_logs(
                &logs_dir,
                &destination_for_worker,
                DiagnosticArchiveRequest {
                    mode: DiagnosticArchiveMode::Online,
                    since: Some(since),
                    engine_preparation: Some(manifest_preparation),
                },
            )
        })
        .await??;
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
            },
        })
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

fn export_preparation_dto(report: LocalDiagnosticExportReport) -> DiagnosticExportPreparationDto {
    DiagnosticExportPreparationDto {
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

#[cfg(test)]
mod tests {
    use super::*;
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
        let diagnostics = EngineDaemonDiagnostics::new(handle);
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
