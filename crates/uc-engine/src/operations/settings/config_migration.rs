use crate::error_codes::*;

use std::path::Path;

use uc_application::facade::AppFacade;
use uc_core::crypto::domain::Passphrase;
use uc_core::ids::RepresentationId;
use uc_core::ports::config_migration::{ConfigMigrationError, ConfigSourceMode};

use crate::runtime::host_file::{copy_host_to_path, copy_path_to_host, HostFileCopyError};
use crate::{
    ConfigExportOutcome, ConfigImportPreviewOutcome, ConfigImportPreviewSummary,
    ConfigImportStageOutcome, ConfigSourceModeSummary, EngineError, EngineErrorCategory,
    ExportConfigInput, HostCapabilityErrorCategory, HostFileAccess, OperationResult,
    PreviewConfigImportInput, StageConfigImportInput,
};

pub(crate) async fn execute_export_config(
    facade: &AppFacade,
    files: &dyn HostFileAccess,
    temporary_root: &Path,
    input: ExportConfigInput,
) -> Result<OperationResult, EngineError> {
    let operation_dir = create_operation_dir(temporary_root, "config-export")?;
    let bundle_path = operation_dir.join("bundle.ucbundle");
    let exported = facade.config_migration.export_config(&bundle_path).await;
    let result = match exported {
        Ok(path) => copy_path_to_host(files, &input.destination, &path)
            .await
            .map_err(|error| map_copy_error(error, EXPORT_CONFIG_FAILED_CODE))
            .map(|()| OperationResult::ConfigExport(ConfigExportOutcome::Exported)),
        Err(ConfigMigrationError::Locked) => {
            Ok(OperationResult::ConfigExport(ConfigExportOutcome::Locked))
        }
        Err(ConfigMigrationError::NotInitialized) => Ok(OperationResult::ConfigExport(
            ConfigExportOutcome::NotInitialized,
        )),
        Err(_) => Err(internal_error(EXPORT_CONFIG_FAILED_CODE)),
    };
    cleanup_operation_dir(&operation_dir);
    result
}

pub(crate) async fn execute_preview_config_import(
    facade: &AppFacade,
    files: &dyn HostFileAccess,
    temporary_root: &Path,
    input: PreviewConfigImportInput,
) -> Result<OperationResult, EngineError> {
    let operation_dir = create_operation_dir(temporary_root, "config-preview")?;
    let bundle_path = operation_dir.join("bundle.ucbundle");
    let copied = copy_host_to_path(files, &input.source, &bundle_path)
        .await
        .map_err(|error| map_copy_error(error, PREVIEW_CONFIG_IMPORT_FAILED_CODE));
    let result = match copied {
        Ok(()) => {
            let password = Passphrase::new(input.password.expose().to_string());
            match facade
                .config_migration
                .preview_import(&password, &bundle_path)
                .await
            {
                Ok(preview) => Ok(OperationResult::ConfigImportPreview(
                    ConfigImportPreviewOutcome::Ready(ConfigImportPreviewSummary {
                        app_version: preview.app_version,
                        source_mode: match preview.source_mode {
                            ConfigSourceMode::Portable => ConfigSourceModeSummary::Portable,
                            ConfigSourceMode::Installed => ConfigSourceModeSummary::Installed,
                        },
                        created_at_unix_ms: preview.created_at_unix_ms,
                        profile_id: preview.profile_id.inner().clone(),
                        device_fingerprint: preview.device_fingerprint,
                    }),
                )),
                Err(ConfigMigrationError::InvalidPasswordOrCorrupt) => {
                    Ok(OperationResult::ConfigImportPreview(
                        ConfigImportPreviewOutcome::InvalidPasswordOrCorrupt,
                    ))
                }
                Err(ConfigMigrationError::IncompatibleBundle { reason }) => {
                    Ok(OperationResult::ConfigImportPreview(
                        ConfigImportPreviewOutcome::Incompatible { reason },
                    ))
                }
                Err(_) => Err(internal_error(PREVIEW_CONFIG_IMPORT_FAILED_CODE)),
            }
        }
        Err(error) => Err(error),
    };
    cleanup_operation_dir(&operation_dir);
    result
}

pub(crate) async fn execute_stage_config_import(
    facade: &AppFacade,
    files: &dyn HostFileAccess,
    temporary_root: &Path,
    input: StageConfigImportInput,
) -> Result<OperationResult, EngineError> {
    let operation_dir = create_operation_dir(temporary_root, "config-stage")?;
    let bundle_path = operation_dir.join("bundle.ucbundle");
    let copied = copy_host_to_path(files, &input.source, &bundle_path)
        .await
        .map_err(|error| map_copy_error(error, STAGE_CONFIG_IMPORT_FAILED_CODE));
    let result = match copied {
        Ok(()) => {
            let password = Passphrase::new(input.password.expose().to_string());
            match facade
                .config_migration
                .stage_import(&password, &bundle_path)
                .await
            {
                Ok(staged) => Ok(OperationResult::ConfigImportStaged(
                    ConfigImportStageOutcome::Staged {
                        unlock_required_after_apply: staged.unlock_required_after_apply,
                    },
                )),
                Err(ConfigMigrationError::InvalidPasswordOrCorrupt) => {
                    Ok(OperationResult::ConfigImportStaged(
                        ConfigImportStageOutcome::InvalidPasswordOrCorrupt,
                    ))
                }
                Err(ConfigMigrationError::IncompatibleBundle { reason }) => {
                    Ok(OperationResult::ConfigImportStaged(
                        ConfigImportStageOutcome::Incompatible { reason },
                    ))
                }
                Err(_) => Err(internal_error(STAGE_CONFIG_IMPORT_FAILED_CODE)),
            }
        }
        Err(error) => Err(error),
    };
    cleanup_operation_dir(&operation_dir);
    result
}

fn create_operation_dir(
    temporary_root: &Path,
    prefix: &str,
) -> Result<std::path::PathBuf, EngineError> {
    let directory = temporary_root.join(format!("{prefix}-{}", RepresentationId::new()));
    std::fs::create_dir_all(&directory)
        .map_err(|_| internal_error(CONFIG_FILE_UNAVAILABLE_CODE))?;
    Ok(directory)
}

fn cleanup_operation_dir(directory: &Path) {
    if let Err(error) = std::fs::remove_dir_all(directory) {
        tracing::warn!(error = %error, "failed to remove config migration temporary directory");
    }
}

fn map_copy_error(error: HostFileCopyError, fallback_code: u32) -> EngineError {
    match error {
        HostFileCopyError::SourceIo => {
            EngineError::new(fallback_code, EngineErrorCategory::Internal, true)
        }
        HostFileCopyError::Host(error) => match error.category() {
            HostCapabilityErrorCategory::InvalidHandle => EngineError::new(
                CONFIG_FILE_INVALID_HANDLE_CODE,
                EngineErrorCategory::InvalidInput,
                false,
            ),
            HostCapabilityErrorCategory::PermissionDenied => EngineError::new(
                CONFIG_FILE_UNAUTHORIZED_CODE,
                EngineErrorCategory::Unauthorized,
                false,
            ),
            HostCapabilityErrorCategory::Unavailable => EngineError::new(
                CONFIG_FILE_UNAVAILABLE_CODE,
                EngineErrorCategory::Unavailable,
                true,
            ),
            HostCapabilityErrorCategory::Io => {
                EngineError::new(fallback_code, EngineErrorCategory::Internal, true)
            }
        },
    }
}

fn internal_error(code: u32) -> EngineError {
    EngineError::new(code, EngineErrorCategory::Internal, false)
}
