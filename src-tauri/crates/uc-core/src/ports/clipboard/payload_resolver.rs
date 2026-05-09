//! Clipboard Payload Resolver Port
//!
//! This port resolves persisted representations into directly usable payloads.
//!
//! **Semantic:** "resolve" = read-only access with best-effort availability

use crate::clipboard::{PayloadAvailability, PersistedClipboardRepresentation};
use crate::ids::RepresentationId;
use crate::BlobId;

/// Result of resolving a clipboard representation into a usable payload
#[derive(Debug, Clone)]
pub enum ResolvedClipboardPayload {
    /// Inline data available (small content or preview)
    Inline { mime: String, bytes: Vec<u8> },

    /// Reference to blob storage (large content)
    BlobRef { mime: String, blob_id: BlobId },
}

/// Typed errors returned by [`ClipboardPayloadResolverPort::resolve`].
///
/// These variants describe **business outcomes** of a resolve attempt, not
/// technical failures. Callers in `uc-application` can match on them to drive
/// state transitions (e.g. demote an orphaned representation to `Lost`) and to
/// translate to user-facing errors at facade / API boundaries.
#[derive(Debug, thiserror::Error)]
pub enum PayloadResolveError {
    /// `payload_state ∈ {Staged, Processing, Failed}` but bytes are no longer
    /// reachable from cache or spool. The representation became orphaned —
    /// the worker can no longer materialize a blob and the resolver can no
    /// longer return bytes. Application layer should demote the
    /// representation to `Lost`.
    #[error("payload bytes unavailable for {rep_id} (state={state:?}): cache and spool miss")]
    Orphaned {
        rep_id: RepresentationId,
        state: PayloadAvailability,
    },

    /// `payload_state == Lost`. Permanent and unrecoverable.
    #[error("payload is lost for {rep_id}: {reason}")]
    Lost {
        rep_id: RepresentationId,
        reason: String,
    },

    /// Representation metadata violates internal invariants (e.g. `Inline`
    /// state with `inline_data == None`, `BlobReady` state with
    /// `blob_id == None`, or a spool I/O error). This is a bug or data
    /// corruption — distinct from a legitimate orphan.
    #[error("payload integrity violation for {rep_id}: {reason}")]
    Integrity {
        rep_id: RepresentationId,
        reason: String,
    },
}

#[async_trait::async_trait]
pub trait ClipboardPayloadResolverPort: Send + Sync {
    /// Resolve a persisted clipboard representation into a usable payload.
    ///
    /// # Resolution rules
    /// 1. **Inline**: If payload state is Inline and inline_data exists → return `Inline`
    /// 2. **BlobReady**: If payload state is BlobReady and blob_id exists → return `BlobRef`
    /// 3. **Staged/Processing/Failed**: Best-effort return of bytes from cache/spool
    ///    - If bytes are available, return `Inline`
    ///    - Otherwise return `PayloadResolveError::Orphaned` (data not currently available)
    /// 4. **Lost**: Return `PayloadResolveError::Lost`
    ///
    /// # Notes
    /// - Resolver must be read-only; no lazy blob writes here.
    /// - Background workers are responsible for materializing blobs.
    async fn resolve(
        &self,
        representation: &PersistedClipboardRepresentation,
    ) -> Result<ResolvedClipboardPayload, PayloadResolveError>;
}
