use crate::clipboard::{MimeType, PayloadAvailability, PersistedClipboardRepresentation};
use crate::ids::{EventId, RepresentationId};
use crate::BlobId;
use anyhow::Result;
use async_trait::async_trait;

/// Result of a processing update CAS operation.
///
/// 处理结果更新的 CAS 操作结果。
#[derive(Debug)]
pub enum ProcessingUpdateOutcome {
    /// Successfully updated and returned the new representation.
    ///
    /// 更新成功并返回新的表示。
    Updated(PersistedClipboardRepresentation),
    /// Representation exists but state did not match expected states.
    ///
    /// 表示存在但状态不匹配预期。
    StateMismatch,
    /// Representation not found.
    ///
    /// 表示不存在。
    NotFound,
}

#[async_trait]
pub trait ClipboardRepresentationRepositoryPort: Send + Sync {
    async fn get_representation(
        &self,
        event_id: &EventId,
        representation_id: &RepresentationId,
    ) -> Result<Option<PersistedClipboardRepresentation>>;

    /// Fetch representation by id without event context.
    ///
    /// Used for recovery scans where only representation id is available.
    async fn get_representation_by_id(
        &self,
        representation_id: &RepresentationId,
    ) -> Result<Option<PersistedClipboardRepresentation>>;

    /// Fetch representation by blob_id when only blob context is available.
    ///
    /// Used for resource resolution when mapping blob id back to representation metadata.
    async fn get_representation_by_blob_id(
        &self,
        blob_id: &BlobId,
    ) -> Result<Option<PersistedClipboardRepresentation>>;

    async fn update_blob_id(
        &self,
        representation_id: &RepresentationId,
        blob_id: &BlobId,
    ) -> Result<()>;

    /// Update blob_id for a representation, but only if it's currently None.
    ///
    /// # Returns
    /// - `true` if the update was applied (blob_id was None)
    /// - `false` if blob_id was already set (no-op)
    ///
    /// # Concurrency safety
    /// This should use compare-and-set semantics at the database level:
    /// ```sql
    /// UPDATE clipboard_snapshots_representations
    /// SET blob_id = ?
    /// WHERE id = ? AND blob_id IS NULL
    /// ```
    async fn update_blob_id_if_none(
        &self,
        representation_id: &RepresentationId,
        blob_id: &BlobId,
    ) -> Result<bool>;

    /// Atomically update blob_id and payload_state with CAS semantics.
    ///
    /// # Transactional update
    /// - Single UPDATE statement sets blob_id, payload_state, last_error
    /// - WHERE clause filters by expected_states
    /// - Returns updated row or outcome if no rows matched
    ///
    /// # Concurrency safety
    /// - Only updates if current state is in expected_states
    /// - Returns state mismatch outcome if state changed by another worker
    async fn update_processing_result(
        &self,
        rep_id: &RepresentationId,
        expected_states: &[PayloadAvailability],
        blob_id: Option<&BlobId>,
        new_state: PayloadAvailability,
        last_error: Option<&str>,
    ) -> Result<ProcessingUpdateOutcome>;

    /// Update the MIME type of a representation.
    ///
    /// Used by the background blob worker to correct MIME after format conversion
    /// (e.g. image/tiff -> image/png).
    ///
    /// Default implementation is a no-op for mock/test implementations.
    async fn update_mime_type(&self, _rep_id: &RepresentationId, _mime: &MimeType) -> Result<()> {
        Ok(())
    }
}
