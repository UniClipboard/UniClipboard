//! Shared binary resource read operations.

use tracing::error;
use uc_application::facade::{AppFacade, ResourceFacadeError};

use crate::{
    BinaryResourceSummary, BlobResourceInput, EngineError, EngineErrorCategory,
    EntryFileResourceSummary, HistoryEntryInput, OperationResult, ThumbnailResourceInput,
};

pub const BLOB_NOT_FOUND_CODE: u32 = 1461;
pub const THUMBNAIL_NOT_FOUND_CODE: u32 = 1462;
pub const ENTRY_FILE_NOT_FOUND_CODE: u32 = 1463;
pub const RESOURCE_READ_FAILED_CODE: u32 = 1464;

pub async fn execute_read_blob(
    facade: &AppFacade,
    input: BlobResourceInput,
) -> Result<OperationResult, EngineError> {
    facade
        .resource
        .blob(&input.blob_id)
        .await
        .map(|resource| {
            OperationResult::BlobRead(BinaryResourceSummary {
                bytes: resource.bytes,
                media_type: resource.mime_type,
            })
        })
        .map_err(|error| map_resource_error(error, BLOB_NOT_FOUND_CODE))
}

pub async fn execute_read_thumbnail(
    facade: &AppFacade,
    input: ThumbnailResourceInput,
) -> Result<OperationResult, EngineError> {
    facade
        .resource
        .thumbnail(&input.representation_id)
        .await
        .map(|resource| {
            OperationResult::ThumbnailRead(BinaryResourceSummary {
                bytes: resource.bytes,
                media_type: resource.mime_type,
            })
        })
        .map_err(|error| map_resource_error(error, THUMBNAIL_NOT_FOUND_CODE))
}

pub async fn execute_read_entry_file(
    facade: &AppFacade,
    input: HistoryEntryInput,
) -> Result<OperationResult, EngineError> {
    facade
        .resource
        .entry_file(&input.entry_id)
        .await
        .map(|resource| {
            OperationResult::EntryFileRead(EntryFileResourceSummary {
                bytes: resource.bytes,
                media_type: resource.mime,
                file_name: resource.filename,
            })
        })
        .map_err(|error| map_resource_error(error, ENTRY_FILE_NOT_FOUND_CODE))
}

fn map_resource_error(error: ResourceFacadeError, not_found_code: u32) -> EngineError {
    match error {
        ResourceFacadeError::NotFound => {
            EngineError::new(not_found_code, EngineErrorCategory::NotFound, false)
        }
        ResourceFacadeError::Mismatch(_) | ResourceFacadeError::Internal(_) => {
            error!("binary resource read failed");
            EngineError::new(
                RESOURCE_READ_FAILED_CODE,
                EngineErrorCategory::Internal,
                false,
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_failures_keep_distinct_not_found_codes_without_details() {
        let blob = map_resource_error(ResourceFacadeError::NotFound, BLOB_NOT_FOUND_CODE);
        let thumbnail = map_resource_error(ResourceFacadeError::NotFound, THUMBNAIL_NOT_FOUND_CODE);
        let file = map_resource_error(ResourceFacadeError::NotFound, ENTRY_FILE_NOT_FOUND_CODE);
        let internal = map_resource_error(
            ResourceFacadeError::Internal("/private/cache/file.bin".into()),
            BLOB_NOT_FOUND_CODE,
        );

        assert_eq!(blob.code(), BLOB_NOT_FOUND_CODE);
        assert_eq!(thumbnail.code(), THUMBNAIL_NOT_FOUND_CODE);
        assert_eq!(file.code(), ENTRY_FILE_NOT_FOUND_CODE);
        assert_eq!(internal.code(), RESOURCE_READ_FAILED_CODE);
        assert!(!internal.to_string().contains("/private/cache/file.bin"));
    }
}
