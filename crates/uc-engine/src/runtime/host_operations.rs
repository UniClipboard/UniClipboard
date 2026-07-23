use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio_util::sync::CancellationToken;
use tracing::{error, warn};
use uc_application::facade::{
    AppFacade, ClipboardHistoryError, ClipboardLiveIndexInput, ClipboardOutboundInput,
    ClipboardOutboundOutcome, ResourceFacadeError, MAX_INLINE_OUTBOUND_REPRESENTATION_BYTES,
};
use uc_core::ids::{DeviceId, FormatId, RepresentationId};
use uc_core::{
    ClipboardChangeOrigin, FileDisplayMetadata, FileDisplayMetadataEntry, MimeType,
    ObservedClipboardRepresentation, SystemClipboardSnapshot, FILE_DISPLAY_METADATA_FORMAT,
    FILE_DISPLAY_METADATA_MIME,
};

use super::{operation_error_with_code, operation_unavailable_error, ProductionRuntime};
use crate::{
    EngineError, EngineErrorCategory, ExportEntryInput, HostFileAccess, OperationResult,
    SendFilesInput, SendImageInput, SendTextInput,
};

const EXPORT_CHUNK_SIZE: usize = 64 * 1024;
const SEND_INVALID_INPUT_CODE: u32 = 1251;
const SEND_FAILED_CODE: u32 = 1252;
const SEND_SKIPPED_CODE: u32 = 1253;
const EXPORT_NOT_FOUND_CODE: u32 = 1271;
const EXPORT_INVALID_TARGET_CODE: u32 = 1272;
const EXPORT_UNAUTHORIZED_CODE: u32 = 1273;
const EXPORT_UNAVAILABLE_CODE: u32 = 1274;
const EXPORT_FAILED_CODE: u32 = 1275;

struct ImportedHostFile {
    path: PathBuf,
    storage_name: String,
    display_name: String,
}

impl ProductionRuntime {
    pub(super) async fn execute_send_text(
        &self,
        input: SendTextInput,
    ) -> Result<OperationResult, EngineError> {
        if input.text.is_empty() || input.text.len() > MAX_INLINE_OUTBOUND_REPRESENTATION_BYTES {
            return Err(send_invalid_input_error());
        }
        let snapshot = SystemClipboardSnapshot {
            ts_ms: self.clock.now_ms(),
            representations: vec![ObservedClipboardRepresentation::new(
                RepresentationId::new(),
                FormatId::from("text"),
                Some(MimeType("text/plain".into())),
                input.text.into_bytes(),
            )],
            file_content_digests: Vec::new(),
            file_set_v1_component: None,
        };
        self.send_snapshot(snapshot, input.target_devices).await
    }

    pub(super) async fn execute_send_image(
        &self,
        input: SendImageInput,
    ) -> Result<OperationResult, EngineError> {
        if input.bytes.is_empty()
            || input.bytes.len() > MAX_INLINE_OUTBOUND_REPRESENTATION_BYTES
            || !input.mime_type.starts_with("image/")
        {
            return Err(send_invalid_input_error());
        }
        let snapshot = SystemClipboardSnapshot {
            ts_ms: self.clock.now_ms(),
            representations: vec![ObservedClipboardRepresentation::new(
                RepresentationId::new(),
                FormatId::from("image"),
                Some(MimeType(input.mime_type)),
                input.bytes,
            )],
            file_content_digests: Vec::new(),
            file_set_v1_component: None,
        };
        self.send_snapshot(snapshot, input.target_devices).await
    }

    pub(super) async fn execute_send_files(
        &self,
        input: SendFilesInput,
        cancellation: &CancellationToken,
    ) -> Result<OperationResult, EngineError> {
        if input.files.is_empty() {
            return Err(send_invalid_input_error());
        }
        let imported = self.import_host_files(&input.files, cancellation).await?;
        let uri_list = imported
            .iter()
            .map(|file| {
                url::Url::from_file_path(&file.path)
                    .map(|url| url.to_string())
                    .map_err(|()| send_failed_error())
            })
            .collect::<Result<Vec<_>, _>>()?
            .join("\n");
        let display_metadata = FileDisplayMetadata {
            files: imported
                .iter()
                .map(|file| FileDisplayMetadataEntry {
                    storage_name: file.storage_name.clone(),
                    display_name: file.display_name.clone(),
                })
                .collect(),
        }
        .encode()
        .map_err(|error| {
            error!(error = %error, "failed to encode file display metadata");
            send_failed_error()
        })?;
        let snapshot = SystemClipboardSnapshot {
            ts_ms: self.clock.now_ms(),
            representations: vec![
                ObservedClipboardRepresentation::new(
                    RepresentationId::new(),
                    FormatId::from("files"),
                    Some(MimeType("text/uri-list".into())),
                    uri_list.into_bytes(),
                ),
                ObservedClipboardRepresentation::new(
                    RepresentationId::new(),
                    FormatId::from(FILE_DISPLAY_METADATA_FORMAT),
                    Some(MimeType(FILE_DISPLAY_METADATA_MIME.into())),
                    display_metadata,
                ),
            ],
            file_content_digests: Vec::new(),
            file_set_v1_component: None,
        };
        self.send_snapshot(snapshot, input.target_devices).await
    }

    pub(super) async fn execute_export_entry(
        &self,
        input: ExportEntryInput,
    ) -> Result<OperationResult, EngineError> {
        let facade = self.current_facade().await?;
        let bytes = load_export_bytes(&facade, &input.entry_id).await?;
        for (index, chunk) in bytes.chunks(EXPORT_CHUNK_SIZE).enumerate() {
            let offset = (index as u64) * (EXPORT_CHUNK_SIZE as u64);
            self.files
                .write_chunk(&input.destination, offset, chunk)
                .map_err(map_export_host_error)?;
            tokio::task::yield_now().await;
        }
        self.files
            .finish_write(&input.destination)
            .map_err(map_export_host_error)?;
        Ok(OperationResult::EntryExported)
    }

    async fn import_host_files(
        &self,
        handles: &[crate::HostFileHandle],
        cancellation: &CancellationToken,
    ) -> Result<Vec<ImportedHostFile>, EngineError> {
        let import_root = self.file_cache_dir.join("engine-imports");
        std::fs::create_dir_all(&import_root).map_err(|error| {
            error!(error = %error, "failed to create engine file import directory");
            send_failed_error()
        })?;
        let operation_dir = import_root.join(RepresentationId::new().to_string());
        std::fs::create_dir(&operation_dir).map_err(|error| {
            error!(error = %error, "failed to create engine file import operation directory");
            send_failed_error()
        })?;

        let mut imported = Vec::with_capacity(handles.len());
        for (index, handle) in handles.iter().enumerate() {
            if cancellation.is_cancelled() {
                cleanup_failed_import(&operation_dir);
                return Err(operation_unavailable_error());
            }
            let metadata = match self.files.metadata(handle) {
                Ok(metadata) => metadata,
                Err(error) => {
                    cleanup_failed_import(&operation_dir);
                    return Err(map_send_host_error(error));
                }
            };
            if metadata.size_bytes == 0 || !valid_host_display_name(&metadata.display_name) {
                cleanup_failed_import(&operation_dir);
                return Err(send_invalid_input_error());
            }
            let storage_name = format!("{index:08}");
            let path = operation_dir.join(&storage_name);
            if let Err(error) = copy_host_file(
                self.files.as_ref(),
                handle,
                metadata.size_bytes,
                &path,
                cancellation,
            ) {
                cleanup_failed_import(&operation_dir);
                return Err(error);
            }
            imported.push(ImportedHostFile {
                path,
                storage_name,
                display_name: metadata.display_name,
            });
            tokio::task::yield_now().await;
        }
        Ok(imported)
    }

    async fn send_snapshot(
        &self,
        snapshot: SystemClipboardSnapshot,
        target_devices: Vec<String>,
    ) -> Result<OperationResult, EngineError> {
        let (capture, live_index, outbound) = {
            let session = self.session.lock().await;
            let session = session.as_ref().ok_or_else(operation_unavailable_error)?;
            (
                Arc::clone(&session.clipboard.capture),
                Arc::clone(&session.clipboard.live_index),
                Arc::clone(&session.clipboard.outbound),
            )
        };
        let captured = capture
            .capture(snapshot.clone(), ClipboardChangeOrigin::LocalCapture, None)
            .await
            .map_err(|error| operation_error_with_code(SEND_FAILED_CODE, "capture send", error))?
            .ok_or_else(|| {
                EngineError::new(SEND_SKIPPED_CODE, EngineErrorCategory::Conflict, false)
            })?;
        if !captured.deduplicated {
            if let Err(error) = live_index
                .index_capture(ClipboardLiveIndexInput {
                    entry_id: captured.entry_id.clone(),
                    snapshot: Arc::new(snapshot.clone()),
                })
                .await
            {
                warn!(error = %error, "failed to index engine send");
            }
        }
        let target_filter = (!target_devices.is_empty()).then(|| {
            target_devices
                .into_iter()
                .map(DeviceId::new)
                .collect::<Vec<_>>()
        });
        let outcome = outbound
            .dispatch_capture_to_targets(
                ClipboardOutboundInput {
                    entry_id: captured.entry_id.clone(),
                    snapshot,
                    origin: ClipboardChangeOrigin::LocalCapture,
                },
                target_filter,
            )
            .await
            .map_err(|error| {
                operation_error_with_code(SEND_FAILED_CODE, "send clipboard", error)
            })?;
        send_report_result(captured.entry_id, outcome)
    }
}

pub(super) fn send_report_result(
    entry_id: String,
    outcome: ClipboardOutboundOutcome,
) -> Result<OperationResult, EngineError> {
    let ClipboardOutboundOutcome::Dispatched {
        snapshot_hash,
        per_target,
        accepted,
        duplicate,
        offline,
        errored,
        pending,
        at_ms,
        ..
    } = outcome
    else {
        return Err(EngineError::new(
            SEND_SKIPPED_CODE,
            EngineErrorCategory::Conflict,
            false,
        ));
    };

    Ok(OperationResult::EntrySent(crate::SendReportSummary {
        entry_id,
        snapshot_hash,
        at_ms,
        total_accepted: accepted,
        total_duplicate: duplicate,
        total_offline: offline,
        total_errored: errored,
        total_pending: pending,
        per_target: per_target
            .into_iter()
            .map(|target| crate::SendTargetSummary {
                device_id: target.device_id.as_str().to_string(),
                outcome: match target.outcome {
                    Ok(uc_core::ports::DispatchAck::Accepted) => crate::SendTargetOutcome::Accepted,
                    Ok(uc_core::ports::DispatchAck::DuplicateIgnored) => {
                        crate::SendTargetOutcome::Duplicate
                    }
                    Err(message) => crate::SendTargetOutcome::Error { message },
                },
            })
            .collect(),
    }))
}

fn valid_host_display_name(display_name: &str) -> bool {
    let name = display_name.trim();
    !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\\', '\0'])
}

fn copy_host_file(
    files: &dyn HostFileAccess,
    handle: &crate::HostFileHandle,
    size_bytes: u64,
    destination: &Path,
    cancellation: &CancellationToken,
) -> Result<(), EngineError> {
    let mut output = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(destination)
        .map_err(|error| {
            error!(error = %error, "failed to create imported host file");
            send_failed_error()
        })?;
    let mut offset = 0_u64;
    while offset < size_bytes {
        if cancellation.is_cancelled() {
            return Err(operation_unavailable_error());
        }
        let remaining = size_bytes - offset;
        let requested = remaining.min(EXPORT_CHUNK_SIZE as u64) as u32;
        let chunk = files
            .read_chunk(handle, offset, requested)
            .map_err(map_send_host_error)?;
        if chunk.is_empty() || chunk.len() > requested as usize {
            return Err(send_failed_error());
        }
        output.write_all(&chunk).map_err(|error| {
            error!(error = %error, "failed to write imported host file");
            send_failed_error()
        })?;
        offset = offset
            .checked_add(chunk.len() as u64)
            .ok_or_else(send_failed_error)?;
    }
    output.sync_all().map_err(|error| {
        error!(error = %error, "failed to sync imported host file");
        send_failed_error()
    })
}

fn cleanup_failed_import(operation_dir: &Path) {
    if let Err(error) = std::fs::remove_dir_all(operation_dir) {
        warn!(error = %error, "failed to remove incomplete engine file import");
    }
}

fn send_invalid_input_error() -> EngineError {
    EngineError::new(
        SEND_INVALID_INPUT_CODE,
        EngineErrorCategory::InvalidInput,
        false,
    )
}

fn send_failed_error() -> EngineError {
    EngineError::new(SEND_FAILED_CODE, EngineErrorCategory::Internal, false)
}

fn map_send_host_error(error: crate::HostCapabilityError) -> EngineError {
    let (category, retryable) = match error.category() {
        crate::HostCapabilityErrorCategory::InvalidHandle => {
            (EngineErrorCategory::InvalidInput, false)
        }
        crate::HostCapabilityErrorCategory::PermissionDenied => {
            (EngineErrorCategory::Unauthorized, false)
        }
        crate::HostCapabilityErrorCategory::Unavailable
        | crate::HostCapabilityErrorCategory::Io => (EngineErrorCategory::Unavailable, true),
    };
    error!(error = %error, "host file import failed");
    EngineError::new(SEND_FAILED_CODE, category, retryable)
}

async fn load_export_bytes(facade: &AppFacade, entry_id: &str) -> Result<Vec<u8>, EngineError> {
    let resource = facade
        .clipboard_history
        .get_entry_resource(entry_id)
        .await
        .map_err(map_export_history_error)?;
    let file_list = resource.mime_type.as_deref().is_some_and(|mime| {
        mime.eq_ignore_ascii_case("text/uri-list") || mime.eq_ignore_ascii_case("file/uri-list")
    });
    if file_list {
        return facade
            .resource
            .entry_file(entry_id)
            .await
            .map(|file| file.bytes)
            .map_err(map_export_resource_error);
    }
    if let Some(bytes) = resource.inline_data {
        return Ok(bytes);
    }
    if let Some(blob_id) = resource.blob_id {
        return facade
            .resource
            .blob(&blob_id)
            .await
            .map(|blob| blob.bytes)
            .map_err(map_export_resource_error);
    }
    Err(EngineError::new(
        EXPORT_FAILED_CODE,
        EngineErrorCategory::Internal,
        false,
    ))
}

fn map_export_history_error(error: ClipboardHistoryError) -> EngineError {
    match error {
        ClipboardHistoryError::NotFound => {
            EngineError::new(EXPORT_NOT_FOUND_CODE, EngineErrorCategory::NotFound, false)
        }
        ClipboardHistoryError::UnsupportedContent => {
            EngineError::new(EXPORT_FAILED_CODE, EngineErrorCategory::Conflict, false)
        }
        ClipboardHistoryError::Internal(_) => {
            operation_error_with_code(EXPORT_FAILED_CODE, "load export entry", error)
        }
    }
}

fn map_export_resource_error(error: ResourceFacadeError) -> EngineError {
    match error {
        ResourceFacadeError::NotFound => {
            EngineError::new(EXPORT_NOT_FOUND_CODE, EngineErrorCategory::NotFound, false)
        }
        ResourceFacadeError::Mismatch(_) | ResourceFacadeError::Internal(_) => {
            operation_error_with_code(EXPORT_FAILED_CODE, "load export resource", error)
        }
    }
}

fn map_export_host_error(error: crate::HostCapabilityError) -> EngineError {
    let (code, category, retryable) = match error.category() {
        crate::HostCapabilityErrorCategory::InvalidHandle => (
            EXPORT_INVALID_TARGET_CODE,
            EngineErrorCategory::InvalidInput,
            false,
        ),
        crate::HostCapabilityErrorCategory::PermissionDenied => (
            EXPORT_UNAUTHORIZED_CODE,
            EngineErrorCategory::Unauthorized,
            false,
        ),
        crate::HostCapabilityErrorCategory::Unavailable
        | crate::HostCapabilityErrorCategory::Io => (
            EXPORT_UNAVAILABLE_CODE,
            EngineErrorCategory::Unavailable,
            true,
        ),
    };
    error!(error = %error, "host export failed");
    EngineError::new(code, category, retryable)
}
