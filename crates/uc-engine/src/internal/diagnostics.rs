use std::path::Path;

use uc_application::facade::{AppFacade, DiagnosticsFacadeError};
use uc_core::ids::RepresentationId;

use crate::internal::host_file::{copy_path_to_host, HostFileCopyError};
use crate::{
    DebugModeUpdateSummary, DiagnosticLogsExportSummary, DiagnosticsStatusSummary, EngineError,
    EngineErrorCategory, ExportDiagnosticLogsInput, HostCapabilityErrorCategory, HostFileAccess,
    OperationResult, UpdateDebugModeInput,
};

pub const QUERY_DIAGNOSTICS_FAILED_CODE: u32 = 1411;
pub const UPDATE_DEBUG_MODE_FAILED_CODE: u32 = 1412;
pub const EXPORT_DIAGNOSTIC_LOGS_FAILED_CODE: u32 = 1413;
pub const EXPORT_DIAGNOSTIC_LOGS_INVALID_TARGET_CODE: u32 = 1414;
pub const EXPORT_DIAGNOSTIC_LOGS_UNAUTHORIZED_CODE: u32 = 1415;
pub const EXPORT_DIAGNOSTIC_LOGS_UNAVAILABLE_CODE: u32 = 1416;

pub(crate) async fn execute_query_diagnostics(
    facade: &AppFacade,
) -> Result<OperationResult, EngineError> {
    let status = facade
        .diagnostics
        .debug_status()
        .await
        .map_err(|_| internal_error(QUERY_DIAGNOSTICS_FAILED_CODE))?;
    Ok(OperationResult::DiagnosticsStatus(
        DiagnosticsStatusSummary {
            debug_mode: status.debug_mode,
            effective_log_profile: status.effective_log_profile,
            restart_required: status.restart_required,
        },
    ))
}

pub(crate) async fn execute_update_debug_mode(
    facade: &AppFacade,
    input: UpdateDebugModeInput,
) -> Result<OperationResult, EngineError> {
    let result = facade
        .diagnostics
        .set_debug_mode(input.enabled)
        .await
        .map_err(|_| internal_error(UPDATE_DEBUG_MODE_FAILED_CODE))?;
    Ok(OperationResult::DebugModeUpdated(DebugModeUpdateSummary {
        debug_mode: result.debug_mode,
        restart_required: result.restart_required,
    }))
}

pub(crate) async fn execute_export_diagnostic_logs(
    facade: &AppFacade,
    files: &dyn HostFileAccess,
    temporary_root: &Path,
    input: ExportDiagnosticLogsInput,
) -> Result<OperationResult, EngineError> {
    let export_dir = temporary_root.join(format!("diagnostic-export-{}", RepresentationId::new()));
    std::fs::create_dir_all(&export_dir)
        .map_err(|_| internal_error(EXPORT_DIAGNOSTIC_LOGS_FAILED_CODE))?;

    let exported = facade
        .diagnostics
        .export_logs_to_dir(input.since_hours, export_dir.clone())
        .await;
    let result = match exported {
        Ok(exported) => copy_path_to_host(files, &input.destination, Path::new(&exported.path))
            .await
            .map_err(map_copy_error)
            .map(|()| {
                OperationResult::DiagnosticLogsExported(DiagnosticLogsExportSummary {
                    included_files: exported.included_files,
                    since_unix_ms: exported.since.timestamp_millis(),
                })
            }),
        Err(error) => Err(map_diagnostics_error(error)),
    };

    if let Err(error) = std::fs::remove_dir_all(&export_dir) {
        tracing::warn!(error = %error, "failed to remove diagnostic export temporary directory");
    }
    result
}

fn map_diagnostics_error(error: DiagnosticsFacadeError) -> EngineError {
    match error {
        DiagnosticsFacadeError::DownloadsUnavailable
        | DiagnosticsFacadeError::LoadSettings(_)
        | DiagnosticsFacadeError::SaveSettings(_)
        | DiagnosticsFacadeError::Export(_) => internal_error(EXPORT_DIAGNOSTIC_LOGS_FAILED_CODE),
    }
}

fn map_copy_error(error: HostFileCopyError) -> EngineError {
    match error {
        HostFileCopyError::SourceIo => internal_error(EXPORT_DIAGNOSTIC_LOGS_FAILED_CODE),
        HostFileCopyError::Host(error) => match error.category() {
            HostCapabilityErrorCategory::InvalidHandle => EngineError::new(
                EXPORT_DIAGNOSTIC_LOGS_INVALID_TARGET_CODE,
                EngineErrorCategory::InvalidInput,
                false,
            ),
            HostCapabilityErrorCategory::PermissionDenied => EngineError::new(
                EXPORT_DIAGNOSTIC_LOGS_UNAUTHORIZED_CODE,
                EngineErrorCategory::Unauthorized,
                false,
            ),
            HostCapabilityErrorCategory::Unavailable => EngineError::new(
                EXPORT_DIAGNOSTIC_LOGS_UNAVAILABLE_CODE,
                EngineErrorCategory::Unavailable,
                true,
            ),
            HostCapabilityErrorCategory::Io => EngineError::new(
                EXPORT_DIAGNOSTIC_LOGS_FAILED_CODE,
                EngineErrorCategory::Internal,
                true,
            ),
        },
    }
}

fn internal_error(code: u32) -> EngineError {
    EngineError::new(code, EngineErrorCategory::Internal, false)
}
