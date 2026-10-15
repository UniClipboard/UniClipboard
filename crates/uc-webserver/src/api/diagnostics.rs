//! HTTP route handlers for local diagnostics endpoints.

use axum::extract::State;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use tracing::{info, instrument};
use uc_daemon_contract::api::dto::diagnostics::{
    DebugStatusDto, DiagnosticCaptureStartRequestDto, DiagnosticCaptureStopRequestDto,
    DiagnosticCaptureStopResultDto, DiagnosticStatusDto, LogExportRequestDto, LogExportResultDto,
    UpdateDebugModeRequestDto, UpdateDebugModeResultDto,
};
use uc_daemon_contract::api::dto::envelope::ApiEnvelope;
use uc_engine::{
    EngineError, EngineErrorCategory, Operation, OperationResult, UpdateDebugModeInput,
};
use utoipa;

use crate::api::dto::error::{log_facade_failure, ApiError};
use crate::api::server::{
    DaemonApiState, DaemonDiagnosticArchive, DaemonDiagnosticArchiveError, DaemonDiagnosticError,
    DaemonDiagnosticsRuntime,
};

pub fn router() -> Router<DaemonApiState> {
    Router::new()
        .route("/diagnostics/debug", get(get_debug_status_handler))
        .route("/diagnostics/debug", put(update_debug_mode_handler))
        .route("/diagnostics/capture", get(get_capture_status_handler))
        .route("/diagnostics/capture/start", post(start_capture_handler))
        .route("/diagnostics/capture/stop", post(stop_capture_handler))
        .route("/diagnostics/log-export", post(export_logs_handler))
}

#[utoipa::path(
    get,
    path = "/diagnostics/capture",
    tag = "system",
    operation_id = "getDiagnosticCaptureStatus",
    responses(
        (status = 200, description = "Current daemon-owned diagnostic capture state", body = DiagnosticStatusEnvelope),
        (status = 503, description = "Local Engine diagnostics unavailable", body = ApiErrorResponse)
    )
)]
#[instrument(name = "api.diagnostics.capture.get", level = "info", skip(state))]
pub async fn get_capture_status_handler(
    State(state): State<DaemonApiState>,
) -> Result<Json<ApiEnvelope<DiagnosticStatusDto>>, ApiError> {
    let runtime = diagnostics_runtime(&state)?;
    let status = runtime.status().map_err(diagnostic_runtime_error)?;
    Ok(Json(ApiEnvelope::now(status)))
}

#[utoipa::path(
    post,
    path = "/diagnostics/capture/start",
    tag = "system",
    operation_id = "startDiagnosticCapture",
    request_body = DiagnosticCaptureStartRequestDto,
    responses(
        (status = 200, description = "Actual daemon-owned diagnostic capture state", body = DiagnosticStatusEnvelope),
        (status = 400, description = "Invalid capture duration", body = ApiErrorResponse),
        (status = 503, description = "Local Engine diagnostics unavailable", body = ApiErrorResponse)
    )
)]
#[instrument(name = "api.diagnostics.capture.start", level = "info", skip(state, payload), fields(duration_seconds = payload.duration_seconds))]
pub async fn start_capture_handler(
    State(state): State<DaemonApiState>,
    Json(payload): Json<DiagnosticCaptureStartRequestDto>,
) -> Result<Json<ApiEnvelope<DiagnosticStatusDto>>, ApiError> {
    let runtime = diagnostics_runtime(&state)?;
    let status = runtime.start(payload).map_err(diagnostic_runtime_error)?;
    Ok(Json(ApiEnvelope::now(status)))
}

#[utoipa::path(
    post,
    path = "/diagnostics/capture/stop",
    tag = "system",
    operation_id = "stopDiagnosticCapture",
    request_body = DiagnosticCaptureStopRequestDto,
    responses(
        (status = 200, description = "Capture stop result", body = DiagnosticCaptureStopEnvelope),
        (status = 400, description = "Invalid capture identifier", body = ApiErrorResponse),
        (status = 503, description = "Local Engine diagnostics unavailable", body = ApiErrorResponse)
    )
)]
#[instrument(
    name = "api.diagnostics.capture.stop",
    level = "info",
    skip(state, payload)
)]
pub async fn stop_capture_handler(
    State(state): State<DaemonApiState>,
    Json(payload): Json<DiagnosticCaptureStopRequestDto>,
) -> Result<Json<ApiEnvelope<DiagnosticCaptureStopResultDto>>, ApiError> {
    let runtime = diagnostics_runtime(&state)?;
    let result = runtime
        .stop(&payload.capture_id)
        .map_err(diagnostic_runtime_error)?;
    Ok(Json(ApiEnvelope::now(result)))
}

#[utoipa::path(
    get,
    path = "/diagnostics/debug",
    tag = "system",
    operation_id = "getDebugStatus",
    responses(
        (status = 200, description = "Current persistent debug-mode status", body = DebugStatusEnvelope),
        (status = 500, description = "Internal server error", body = ApiErrorResponse)
    )
)]
#[instrument(name = "api.diagnostics.debug.get", level = "info", skip(state))]
pub async fn get_debug_status_handler(
    State(state): State<DaemonApiState>,
) -> Result<Json<ApiEnvelope<DebugStatusDto>>, ApiError> {
    let result = state
        .execute(Operation::QueryDiagnostics)
        .await
        .map_err(|error| diagnostics_error_to_api("get_debug_status", error))?;
    let OperationResult::DiagnosticsStatus(status) = result else {
        return Err(ApiError::internal(
            "engine returned an unexpected diagnostics result",
        ));
    };
    Ok(Json(ApiEnvelope::now(debug_status_dto(
        status.debug_mode,
        status.effective_log_profile,
        status.restart_required,
        uc_observability::installed_log_profile(),
    ))))
}

/// Debug Mode status as the user can act on it. `debug_mode` is the persisted
/// setting; the log profile is what this process actually installed, which the
/// setting only reaches after a restart (and which an environment override can
/// replace). The installed profile wins over the Engine's computed one.
fn debug_status_dto(
    debug_mode: bool,
    engine_log_profile: String,
    restart_required: bool,
    installed_log_profile: Option<&str>,
) -> DebugStatusDto {
    DebugStatusDto {
        debug_mode,
        effective_log_profile: installed_log_profile
            .map(str::to_owned)
            .unwrap_or(engine_log_profile),
        restart_required,
    }
}

#[utoipa::path(
    put,
    path = "/diagnostics/debug",
    tag = "system",
    operation_id = "updateDebugMode",
    request_body = UpdateDebugModeRequestDto,
    responses(
        (status = 200, description = "Debug mode persisted", body = UpdateDebugModeEnvelope),
        (status = 500, description = "Internal server error", body = ApiErrorResponse)
    )
)]
#[instrument(name = "api.diagnostics.debug.update", level = "info", skip(state, payload), fields(enabled = payload.enabled))]
pub async fn update_debug_mode_handler(
    State(state): State<DaemonApiState>,
    Json(payload): Json<UpdateDebugModeRequestDto>,
) -> Result<Json<ApiEnvelope<UpdateDebugModeResultDto>>, ApiError> {
    let result = state
        .execute(Operation::UpdateDebugMode(UpdateDebugModeInput {
            enabled: payload.enabled,
        }))
        .await
        .map_err(|error| diagnostics_error_to_api("update_debug_mode", error))?;
    let OperationResult::DebugModeUpdated(result) = result else {
        return Err(ApiError::internal(
            "engine returned an unexpected debug-mode result",
        ));
    };
    info!(
        debug_mode = result.debug_mode,
        restart_required = result.restart_required,
        "debug mode updated"
    );
    Ok(Json(ApiEnvelope::now(UpdateDebugModeResultDto {
        debug_mode: result.debug_mode,
        restart_required: result.restart_required,
    })))
}

#[utoipa::path(
    post,
    path = "/diagnostics/log-export",
    tag = "system",
    operation_id = "exportLogs",
    request_body = LogExportRequestDto,
    responses(
        (status = 200, description = "Logs exported to Downloads", body = LogExportEnvelope),
        (status = 500, description = "Export failed; `details.cause` is source_read_failed, archive_write_failed or unknown", body = ApiErrorResponse),
        (status = 503, description = "Export destination or local Engine diagnostics unavailable; `details.cause` may be destination_unavailable", body = ApiErrorResponse)
    )
)]
#[instrument(name = "api.diagnostics.log_export", level = "info", skip(state, payload), fields(since_hours = payload.since_hours))]
pub async fn export_logs_handler(
    State(state): State<DaemonApiState>,
    Json(payload): Json<LogExportRequestDto>,
) -> Result<Json<ApiEnvelope<LogExportResultDto>>, ApiError> {
    let runtime = diagnostics_runtime(&state)?;
    let archive = state
        .diagnostic_archive
        .as_deref()
        .ok_or_else(|| ApiError::service_unavailable("diagnostic archive is unavailable"))?;
    let result = prepare_and_export(runtime, archive, payload.since_hours).await?;
    info!(
        included_files = result.collection.included_files.len(),
        unreadable_files = result.collection.unreadable_files.len(),
        truncated_files = result.collection.truncated_files.len(),
        flush = ?result.engine_preparation.flush,
        "logs exported"
    );
    Ok(Json(ApiEnvelope::now(result)))
}

async fn prepare_and_export(
    runtime: &dyn DaemonDiagnosticsRuntime,
    archive: &dyn DaemonDiagnosticArchive,
    since_hours: Option<u32>,
) -> Result<LogExportResultDto, ApiError> {
    let preparation = runtime.prepare_export().map_err(diagnostic_runtime_error)?;
    archive
        .export(since_hours, preparation)
        .await
        .map_err(archive_export_error)
}

/// Stable error for a failed archive export: `code` plus the cause in
/// `details`, and one ERROR record with the same cause.
fn archive_export_error(error: DaemonDiagnosticArchiveError) -> ApiError {
    let (cause, api) = match error {
        DaemonDiagnosticArchiveError::SourceReadFailed => (
            "source_read_failed",
            ApiError::internal("diagnostic logs could not be read"),
        ),
        DaemonDiagnosticArchiveError::ArchiveWriteFailed => (
            "archive_write_failed",
            ApiError::internal("diagnostic archive could not be written"),
        ),
        DaemonDiagnosticArchiveError::DestinationUnavailable => (
            "destination_unavailable",
            ApiError::service_unavailable("diagnostic export destination is unavailable"),
        ),
        DaemonDiagnosticArchiveError::Unknown => (
            "unknown",
            ApiError::internal("diagnostic archive export failed"),
        ),
    };
    let api = api
        .with_code("diagnostic_export_failed")
        .with_details(serde_json::json!({ "cause": cause }));
    log_facade_failure("diagnostics", "log_export", cause, api.status, &api.message);
    api
}

fn diagnostics_runtime(state: &DaemonApiState) -> Result<&dyn DaemonDiagnosticsRuntime, ApiError> {
    state
        .diagnostics_runtime
        .as_deref()
        .ok_or_else(|| ApiError::service_unavailable("local Engine diagnostics are unavailable"))
}

fn diagnostic_runtime_error(error: DaemonDiagnosticError) -> ApiError {
    match error {
        DaemonDiagnosticError::InvalidInput => ApiError::bad_request("invalid diagnostic request"),
        DaemonDiagnosticError::Unavailable | DaemonDiagnosticError::AlreadyShutdown => {
            ApiError::service_unavailable("local Engine diagnostics are unavailable")
        }
    }
}

fn diagnostics_error_to_api(op: &'static str, error: EngineError) -> ApiError {
    let (variant, api): (&'static str, ApiError) = match error.category() {
        EngineErrorCategory::InvalidInput => (
            "invalid_input",
            ApiError::bad_request("invalid diagnostic export destination"),
        ),
        EngineErrorCategory::Unauthorized => (
            "unauthorized",
            ApiError::unauthorized("diagnostic export destination is not writable"),
        ),
        EngineErrorCategory::Unavailable | EngineErrorCategory::DeadlineExceeded => (
            "unavailable",
            ApiError::service_unavailable("diagnostic export destination is unavailable"),
        ),
        EngineErrorCategory::InvalidState
        | EngineErrorCategory::NotFound
        | EngineErrorCategory::Conflict
        | EngineErrorCategory::Internal => (
            "operation_failed",
            ApiError::internal("diagnostics operation failed"),
        ),
    };
    log_facade_failure("diagnostics", op, variant, api.status, &api.message);
    api
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    use chrono::Utc;
    use uc_daemon_contract::api::dto::diagnostics::{
        DiagnosticArchiveCollectionDto, DiagnosticCaptureModeDto, DiagnosticCaptureStateDto,
        DiagnosticExportPreparationDto, DiagnosticSetupStatusDto, DiagnosticSignalResultDto,
        DiagnosticStatusDto,
    };

    use super::*;

    struct RuntimeProbe {
        prepared: Arc<AtomicBool>,
    }

    impl DaemonDiagnosticsRuntime for RuntimeProbe {
        fn status(&self) -> Result<DiagnosticStatusDto, DaemonDiagnosticError> {
            Ok(status())
        }

        fn start(
            &self,
            _request: DiagnosticCaptureStartRequestDto,
        ) -> Result<DiagnosticStatusDto, DaemonDiagnosticError> {
            Ok(status())
        }

        fn stop(
            &self,
            _capture_id: &str,
        ) -> Result<DiagnosticCaptureStopResultDto, DaemonDiagnosticError> {
            Ok(DiagnosticCaptureStopResultDto::Stopped)
        }

        fn prepare_export(&self) -> Result<DiagnosticExportPreparationDto, DaemonDiagnosticError> {
            self.prepared.store(true, Ordering::SeqCst);
            Ok(preparation())
        }
    }

    struct ArchiveProbe {
        prepared: Arc<AtomicBool>,
    }

    #[async_trait::async_trait]
    impl DaemonDiagnosticArchive for ArchiveProbe {
        async fn export(
            &self,
            _since_hours: Option<u32>,
            engine_preparation: DiagnosticExportPreparationDto,
        ) -> Result<LogExportResultDto, DaemonDiagnosticArchiveError> {
            assert!(self.prepared.load(Ordering::SeqCst));
            assert_eq!(engine_preparation.status.run_id, "run-1");
            Ok(LogExportResultDto {
                path: "/downloads/diagnostics.zip".to_string(),
                included_files: vec!["engine.2026-09-11.jsonl".to_string()],
                since: Utc::now(),
                engine_preparation,
                collection: DiagnosticArchiveCollectionDto {
                    included_files: vec!["engine.2026-09-11.jsonl".to_string()],
                    unreadable_files: Vec::new(),
                    truncated_files: Vec::new(),
                    concurrent_writes_possible: true,
                    missing_sources: Vec::new(),
                },
            })
        }
    }

    #[tokio::test]
    async fn export_prepares_engine_before_collecting_the_archive() {
        let prepared = Arc::new(AtomicBool::new(false));
        let runtime = RuntimeProbe {
            prepared: Arc::clone(&prepared),
        };
        let archive = ArchiveProbe { prepared };

        let result = prepare_and_export(&runtime, &archive, Some(24))
            .await
            .expect("export");

        assert_eq!(result.engine_preparation.status.run_id, "run-1");
        assert_eq!(result.collection.included_files.len(), 1);
    }

    #[test]
    fn debug_status_reports_the_profile_the_process_installed() {
        // Debug Mode is saved but the process still runs the production profile.
        let status = debug_status_dto(true, "debug".to_owned(), true, Some("prod"));
        assert!(status.debug_mode);
        assert_eq!(status.effective_log_profile, "prod");
        assert!(status.restart_required);

        // Without an installed profile (host logging not up) the Engine value
        // is all there is.
        let status = debug_status_dto(false, "prod".to_owned(), false, None);
        assert_eq!(status.effective_log_profile, "prod");
    }

    struct FailingArchive(DaemonDiagnosticArchiveError);

    #[async_trait::async_trait]
    impl DaemonDiagnosticArchive for FailingArchive {
        async fn export(
            &self,
            _since_hours: Option<u32>,
            _engine_preparation: DiagnosticExportPreparationDto,
        ) -> Result<LogExportResultDto, DaemonDiagnosticArchiveError> {
            Err(self.0)
        }
    }

    /// Records of every thread of this test binary, kept in one process-wide
    /// buffer behind a permanently installed subscriber. A scoped subscriber
    /// per test races with the other tests' callsite interest caches (the
    /// ERROR callsite under test is shared), so the subscriber is global and
    /// each test reads back only its own thread's lines.
    fn records_of_current_thread(run: impl FnOnce()) -> Vec<String> {
        use std::io::Write;
        use std::sync::{Mutex, OnceLock};

        static BUFFER: OnceLock<Arc<Mutex<Vec<u8>>>> = OnceLock::new();
        #[derive(Clone)]
        struct Shared(Arc<Mutex<Vec<u8>>>);
        impl Write for Shared {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.0.lock().expect("buffer").extend_from_slice(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Shared {
            type Writer = Shared;
            fn make_writer(&'a self) -> Shared {
                self.clone()
            }
        }

        let buffer = BUFFER.get_or_init(|| {
            let buffer = Arc::new(Mutex::new(Vec::new()));
            let subscriber = tracing_subscriber::fmt()
                .with_writer(Shared(Arc::clone(&buffer)))
                .with_ansi(false)
                .with_thread_ids(true)
                .finish();
            // Another test binary component may already own the global default;
            // the buffer then simply stays empty and the assertion reports it.
            let _ = tracing::subscriber::set_global_default(subscriber);
            buffer
        });
        let marker = format!("{:?}", std::thread::current().id());
        let before = buffer.lock().expect("buffer").len();
        run();
        let bytes = buffer.lock().expect("buffer")[before..].to_vec();
        String::from_utf8(bytes)
            .expect("utf8")
            .lines()
            .filter(|line| line.contains(&marker))
            .map(str::to_owned)
            .collect()
    }

    fn export_failure_cases() -> [(
        DaemonDiagnosticArchiveError,
        &'static str,
        axum::http::StatusCode,
    ); 4] {
        [
            (
                DaemonDiagnosticArchiveError::SourceReadFailed,
                "source_read_failed",
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            ),
            (
                DaemonDiagnosticArchiveError::ArchiveWriteFailed,
                "archive_write_failed",
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            ),
            (
                DaemonDiagnosticArchiveError::DestinationUnavailable,
                "destination_unavailable",
                axum::http::StatusCode::SERVICE_UNAVAILABLE,
            ),
            (
                DaemonDiagnosticArchiveError::Unknown,
                "unknown",
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            ),
        ]
    }

    #[tokio::test]
    async fn a_failed_archive_export_names_its_cause() {
        for (failure, cause, status) in export_failure_cases() {
            let runtime = RuntimeProbe {
                prepared: Arc::new(AtomicBool::new(false)),
            };

            let error = prepare_and_export(&runtime, &FailingArchive(failure), Some(24))
                .await
                .expect_err("export fails");

            assert_eq!(error.status, status);
            assert_eq!(error.code, "diagnostic_export_failed");
            assert_eq!(error.details, Some(serde_json::json!({ "cause": cause })));
        }
    }

    #[test]
    fn a_failed_archive_export_is_logged_once_with_its_cause() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        for (failure, cause, status) in export_failure_cases() {
            if !status.is_server_error() {
                continue;
            }

            let records = records_of_current_thread(|| {
                runtime.block_on(async {
                    let probe = RuntimeProbe {
                        prepared: Arc::new(AtomicBool::new(false)),
                    };
                    let _ = prepare_and_export(&probe, &FailingArchive(failure), Some(24)).await;
                });
            });

            let with_cause = records.iter().filter(|line| line.contains(cause)).count();
            assert_eq!(with_cause, 1, "one record for {cause}: {records:?}");
            assert!(
                records.iter().all(|line| !line.contains("/Users")),
                "no paths in the record: {records:?}"
            );
        }
    }

    #[test]
    fn unavailable_runtime_is_not_reported_as_an_empty_capture() {
        let error = diagnostic_runtime_error(DaemonDiagnosticError::Unavailable);
        assert_eq!(error.status, axum::http::StatusCode::SERVICE_UNAVAILABLE);
    }

    fn preparation() -> DiagnosticExportPreparationDto {
        DiagnosticExportPreparationDto {
            flush: DiagnosticSignalResultDto::Completed,
            status: status(),
            requested_at_utc: "2026-09-11T00:00:00Z".to_string(),
            completed_at_utc: "2026-09-11T00:00:01Z".to_string(),
            other_processes_flushed: false,
            files: Vec::new(),
        }
    }

    fn status() -> DiagnosticStatusDto {
        DiagnosticStatusDto {
            run_id: "run-1".to_string(),
            capture: DiagnosticCaptureStateDto {
                mode: DiagnosticCaptureModeDto::Standard,
                capture_id: None,
                remaining_ms: 0,
                started_at_utc: None,
                end_reason: None,
                last_capture_id: None,
                revision: "0".to_string(),
            },
            observed_records: "0".to_string(),
            policy_filtered_records: "0".to_string(),
            schema_rejected_records: "0".to_string(),
            correlation_limited_records: "0".to_string(),
            engine_version: "1.1.0".to_string(),
            source_commit: "abc123".to_string(),
            counter_scope: "typed_events_only".to_string(),
            sources: Vec::new(),
            local_file: DiagnosticSetupStatusDto::Ready,
            closed: false,
        }
    }
}
