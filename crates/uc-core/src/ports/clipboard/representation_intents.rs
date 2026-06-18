//! Intent ports for clipboard representation persistence.
//!
//! Split by responsibility direction: a read side consumed by history /
//! restore / projection flows, and a write side consumed by background payload
//! processing. The underlying adapter implements all of them; the composition
//! root coerces the single adapter into each intent port.

use async_trait::async_trait;

use crate::clipboard::{
    ClipboardRepositoryError, MimeType, PayloadAvailability, PersistedClipboardRepresentation,
};
use crate::ids::{EventId, RepresentationId};
use crate::BlobId;

// Re-exported from the legacy aggregate trait module until that trait is
// demoted to an inner store. Callers should import it from here.
pub use super::representation_repository::ProcessingUpdateOutcome;

// ---- Read side ----------------------------------------------------------

/// Fetch a representation within the context of its owning event.
#[async_trait]
pub trait GetRepresentationPort: Send + Sync {
    /// Returns the representation, or `None` when it does not exist for the
    /// given `event_id` / `representation_id` pair.
    async fn get_representation(
        &self,
        event_id: &EventId,
        representation_id: &RepresentationId,
    ) -> Result<Option<PersistedClipboardRepresentation>, ClipboardRepositoryError>;
}

/// Fetch a representation by its id alone, without event context.
#[async_trait]
pub trait GetRepresentationByIdPort: Send + Sync {
    /// Returns the representation, or `None` when no representation with
    /// `representation_id` exists.
    async fn get_representation_by_id(
        &self,
        representation_id: &RepresentationId,
    ) -> Result<Option<PersistedClipboardRepresentation>, ClipboardRepositoryError>;
}

/// Fetch a representation by the blob it resolves to.
#[async_trait]
pub trait GetRepresentationByBlobIdPort: Send + Sync {
    /// Returns the representation whose payload maps to `blob_id`, or `None`
    /// when no representation references that blob.
    async fn get_representation_by_blob_id(
        &self,
        blob_id: &BlobId,
    ) -> Result<Option<PersistedClipboardRepresentation>, ClipboardRepositoryError>;
}

/// List every representation belonging to one event.
#[async_trait]
pub trait ListRepresentationsForEventPort: Send + Sync {
    /// Returns all representations recorded for `event_id`. An empty vector
    /// means the event has no representations.
    async fn get_representations_for_event(
        &self,
        event_id: &EventId,
    ) -> Result<Vec<PersistedClipboardRepresentation>, ClipboardRepositoryError>;
}

// ---- Write side ---------------------------------------------------------

/// Update the blob association of a representation.
#[async_trait]
pub trait UpdateRepresentationBlobIdPort: Send + Sync {
    /// Sets `blob_id` unconditionally on the representation.
    async fn update_blob_id(
        &self,
        representation_id: &RepresentationId,
        blob_id: &BlobId,
    ) -> Result<(), ClipboardRepositoryError>;

    /// Sets `blob_id` only when it is currently unset (compare-and-set).
    /// Returns `true` when the update was applied, `false` when a blob was
    /// already associated.
    async fn update_blob_id_if_none(
        &self,
        representation_id: &RepresentationId,
        blob_id: &BlobId,
    ) -> Result<bool, ClipboardRepositoryError>;
}

/// Atomically advance the processing state of a representation.
#[async_trait]
pub trait UpdateRepresentationProcessingResultPort: Send + Sync {
    /// Updates `blob_id` and `payload_state` in a single compare-and-set
    /// against `expected_states`. Returns the new representation on success,
    /// or an outcome describing why no row matched.
    async fn update_processing_result(
        &self,
        rep_id: &RepresentationId,
        expected_states: &[PayloadAvailability],
        blob_id: Option<&BlobId>,
        new_state: PayloadAvailability,
        last_error: Option<&str>,
    ) -> Result<ProcessingUpdateOutcome, ClipboardRepositoryError>;
}

/// Correct the MIME type of a representation after payload conversion.
#[async_trait]
pub trait UpdateRepresentationMimePort: Send + Sync {
    /// Sets the representation's MIME type to `mime`.
    async fn update_mime_type(
        &self,
        rep_id: &RepresentationId,
        mime: &MimeType,
    ) -> Result<(), ClipboardRepositoryError>;
}

/// Enumerate representations currently in one of the given payload states.
#[async_trait]
pub trait ListRepresentationIdsByStatePort: Send + Sync {
    /// Returns the ids of all representations whose `payload_state` matches any
    /// entry in `states`.
    async fn list_ids_by_payload_state(
        &self,
        states: &[PayloadAvailability],
    ) -> Result<Vec<RepresentationId>, ClipboardRepositoryError>;
}
