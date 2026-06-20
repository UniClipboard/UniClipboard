//! On-demand pull ports for the cross-device active-clipboard register.
//!
//! When a device observes a peer's active-clipboard state whose content it
//! does not hold locally, it pulls that content from the reporting peer on
//! demand. Two ports model the two ends of one request/response exchange:
//!
//! - [`ActiveClipboardPullServePort`] — the holding side answers a pull: it
//!   produces a transfer-encrypted envelope of the content identified by a
//!   cross-device content hash. Producing the envelope requires an unlocked
//!   session (the content must be decrypted from at-rest form and re-encrypted
//!   for transfer), so a locked device cannot serve.
//! - [`ActiveClipboardPullClientPort`] — the requesting side asks a specific
//!   peer for the content behind a content hash and returns the transfer
//!   envelope the holder produced, ready to be decoded + stored locally.
//!
//! The envelope both ports speak in is the same transfer wire format the bulk
//! clipboard sync path uses: an opaque, transfer-encrypted byte string the
//! consumer feeds to the existing decode + store pipeline. Neither port
//! exposes plaintext.

use async_trait::async_trait;

use crate::ids::DeviceId;

/// Failure surface for serving a pull request (holding side).
#[derive(Debug, thiserror::Error)]
pub enum ActiveClipboardPullServeError {
    /// The requested content is not held locally — there is no entry whose
    /// content hash matches, or its payload can no longer be materialized.
    /// The requester treats this as "this peer cannot satisfy the pull".
    #[error("requested content is not available")]
    NotAvailable,
    /// The session is locked, so the content cannot be decrypted and
    /// re-encrypted for transfer. A locked device cannot serve a pull.
    #[error("session is locked; cannot serve pull")]
    NotUnlocked,
    /// Any other unrecoverable failure while building the transfer envelope
    /// (storage error, encode failure, transfer-cipher failure).
    #[error("internal pull-serve failure: {0}")]
    Internal(String),
}

/// Produce a transfer-encrypted envelope for content held locally, addressed
/// by its cross-device content hash.
///
/// The returned bytes are the same transfer wire format the bulk clipboard
/// sync path produces: an opaque, transfer-encrypted byte string the consumer
/// decodes + stores through the standard inbound pipeline. The envelope is
/// freshly produced per call (a fresh transfer identity), never a copy of any
/// at-rest encrypted form.
///
/// Returns [`ActiveClipboardPullServeError::NotAvailable`] when the content is
/// not held, and [`ActiveClipboardPullServeError::NotUnlocked`] when the
/// session is locked (serving requires decrypt + re-encrypt).
#[async_trait]
pub trait ActiveClipboardPullServePort: Send + Sync {
    /// Build the transfer envelope for the content identified by
    /// `snapshot_hash` (the cross-device `"blake3v1:<hex>"` string).
    async fn serve(&self, snapshot_hash: &str) -> Result<Vec<u8>, ActiveClipboardPullServeError>;
}

/// Failure surface for issuing a pull request (requesting side).
#[derive(Debug, thiserror::Error)]
pub enum ActiveClipboardPullClientError {
    /// The peer could not be reached (no address, dial failure) or did not
    /// respond within the pull deadline.
    #[error("peer offline or pull timed out")]
    Unreachable,
    /// The peer responded but reported it does not hold the content (or its
    /// session was locked, so it could not serve).
    #[error("peer cannot serve the requested content")]
    NotAvailable,
    /// Stream / protocol I/O failure during the exchange.
    #[error("pull io: {0}")]
    Io(String),
}

/// Request the transfer envelope for `snapshot_hash` from a single peer.
///
/// Returns the transfer-encrypted envelope bytes the holder produced (the
/// same format [`ActiveClipboardPullServePort::serve`] returns), ready to be
/// decoded + stored through the standard inbound pipeline. The call is bounded
/// by an implementation-defined deadline; a peer that is unreachable or slow
/// surfaces as [`ActiveClipboardPullClientError::Unreachable`], and a peer that
/// answers "I don't hold it" surfaces as
/// [`ActiveClipboardPullClientError::NotAvailable`].
#[async_trait]
pub trait ActiveClipboardPullClientPort: Send + Sync {
    /// Pull the content behind `snapshot_hash` from `peer`.
    async fn pull(
        &self,
        peer: &DeviceId,
        snapshot_hash: &str,
    ) -> Result<Vec<u8>, ActiveClipboardPullClientError>;
}
