use uc_application::facade::mobile_sync::{
    ApplyIncomingMobileClipError, ApplyIncomingMobileClipOutcome, CheckContentAvailableError,
    GetLatestMobileSyncDocError, GetMobileSyncFileError, MobileSyncFacade, SyncClipboardItemType,
    SyncClipboardMeta,
};
use uc_core::mobile_sync::MobileDeviceId;

use crate::{
    ApplyMobileSyncDocumentInput, EngineError, EngineErrorCategory, MobileContentAvailabilityInput,
    MobileSyncDocument, MobileSyncDocumentApplyOutcome, MobileSyncFile, MobileSyncFileReadOutcome,
    MobileSyncItemType, OperationResult, ReadMobileSyncFileInput,
};

const MOBILE_CONTENT_INVALID_INPUT_CODE: u32 = 1441;
const MOBILE_CONTENT_LOOKUP_FAILED_CODE: u32 = 1442;
const MOBILE_DOCUMENT_QUERY_FAILED_CODE: u32 = 1443;
const MOBILE_DOCUMENT_APPLY_FAILED_CODE: u32 = 1444;
const MOBILE_FILE_READ_FAILED_CODE: u32 = 1445;

pub(crate) async fn execute_check_mobile_content_available(
    facade: &MobileSyncFacade,
    input: MobileContentAvailabilityInput,
) -> Result<OperationResult, EngineError> {
    if input.snapshot_hash.trim().is_empty() {
        return Err(invalid_input_error());
    }
    facade
        .check_content_available(&input.snapshot_hash)
        .await
        .map(|available| OperationResult::MobileContentAvailability { available })
        .map_err(|CheckContentAvailableError::Repository(_)| {
            internal_error(MOBILE_CONTENT_LOOKUP_FAILED_CODE)
        })
}

pub(crate) async fn execute_query_latest_mobile_sync_document(
    facade: &MobileSyncFacade,
) -> Result<OperationResult, EngineError> {
    match facade.get_latest_sync_doc().await {
        Ok(document) => Ok(OperationResult::MobileSyncDocument(Some(Box::new(
            map_document(document),
        )))),
        Err(GetLatestMobileSyncDocError::NotFound) => Ok(OperationResult::MobileSyncDocument(None)),
        Err(GetLatestMobileSyncDocError::Port(_)) => {
            Err(internal_error(MOBILE_DOCUMENT_QUERY_FAILED_CODE))
        }
    }
}

pub(crate) async fn execute_apply_mobile_sync_document(
    facade: &MobileSyncFacade,
    input: ApplyMobileSyncDocumentInput,
) -> Result<OperationResult, EngineError> {
    if input.source_device_id.trim().is_empty() {
        return Err(invalid_input_error());
    }
    let outcome = facade
        .put_sync_doc(
            unmap_document(input.document),
            MobileDeviceId::new(input.source_device_id),
        )
        .await
        .map_err(map_apply_error)?;
    Ok(OperationResult::MobileSyncDocumentApplied(
        map_apply_outcome(outcome),
    ))
}

pub(crate) async fn execute_read_mobile_sync_file(
    facade: &MobileSyncFacade,
    input: ReadMobileSyncFileInput,
) -> Result<OperationResult, EngineError> {
    if input.data_name.trim().is_empty() {
        return Err(invalid_input_error());
    }
    match facade.get_clipboard_file(&input.data_name).await {
        Ok(file) => Ok(OperationResult::MobileSyncFile(
            MobileSyncFileReadOutcome::Found(Box::new(MobileSyncFile {
                media_type: file.mime,
                bytes: file.bytes,
            })),
        )),
        Err(GetMobileSyncFileError::NotFound) => Ok(OperationResult::MobileSyncFile(
            MobileSyncFileReadOutcome::NotFound,
        )),
        Err(GetMobileSyncFileError::Port(_) | GetMobileSyncFileError::Staging(_)) => {
            Err(internal_error(MOBILE_FILE_READ_FAILED_CODE))
        }
    }
}

pub(crate) fn map_apply_outcome(
    outcome: ApplyIncomingMobileClipOutcome,
) -> MobileSyncDocumentApplyOutcome {
    match outcome {
        ApplyIncomingMobileClipOutcome::Applied {
            entry_id,
            content_id,
        } => MobileSyncDocumentApplyOutcome::Applied {
            entry_id: entry_id.to_string(),
            content_id,
        },
        ApplyIncomingMobileClipOutcome::Resurfaced {
            snapshot_hash,
            existing_entry_id,
            os_write_succeeded,
        } => MobileSyncDocumentApplyOutcome::Resurfaced {
            snapshot_hash,
            existing_entry_id: existing_entry_id.to_string(),
            os_write_succeeded,
        },
        ApplyIncomingMobileClipOutcome::DuplicateSkipped {
            snapshot_hash,
            existing_entry_id,
        } => MobileSyncDocumentApplyOutcome::DuplicateSkipped {
            snapshot_hash,
            existing_entry_id: existing_entry_id.to_string(),
        },
        ApplyIncomingMobileClipOutcome::DecodeFailed { reason } => {
            MobileSyncDocumentApplyOutcome::DecodeFailed { reason }
        }
        ApplyIncomingMobileClipOutcome::Buffered => MobileSyncDocumentApplyOutcome::Buffered,
    }
}

pub(crate) fn map_apply_error(_error: ApplyIncomingMobileClipError) -> EngineError {
    internal_error(MOBILE_DOCUMENT_APPLY_FAILED_CODE)
}

fn map_document(document: SyncClipboardMeta) -> MobileSyncDocument {
    MobileSyncDocument {
        item_type: map_item_type(document.item_type),
        text: document.text,
        data_name: document.data_name,
        has_data: document.has_data,
        size: document.size,
        hash: document.hash,
        content_id: document.content_id,
    }
}

fn unmap_document(document: MobileSyncDocument) -> SyncClipboardMeta {
    SyncClipboardMeta {
        item_type: unmap_item_type(document.item_type),
        text: document.text,
        data_name: document.data_name,
        has_data: document.has_data,
        size: document.size,
        hash: document.hash,
        content_id: document.content_id,
    }
}

fn map_item_type(item_type: SyncClipboardItemType) -> MobileSyncItemType {
    match item_type {
        SyncClipboardItemType::Text => MobileSyncItemType::Text,
        SyncClipboardItemType::Image => MobileSyncItemType::Image,
        SyncClipboardItemType::File => MobileSyncItemType::File,
        SyncClipboardItemType::Group => MobileSyncItemType::Group,
    }
}

fn unmap_item_type(item_type: MobileSyncItemType) -> SyncClipboardItemType {
    match item_type {
        MobileSyncItemType::Text => SyncClipboardItemType::Text,
        MobileSyncItemType::Image => SyncClipboardItemType::Image,
        MobileSyncItemType::File => SyncClipboardItemType::File,
        MobileSyncItemType::Group => SyncClipboardItemType::Group,
    }
}

fn invalid_input_error() -> EngineError {
    EngineError::new(
        MOBILE_CONTENT_INVALID_INPUT_CODE,
        EngineErrorCategory::InvalidInput,
        false,
    )
}

fn internal_error(code: u32) -> EngineError {
    EngineError::new(code, EngineErrorCategory::Internal, true)
}
